//! The evidence pipeline: ingest -> extract -> sentiment (`ingest`,
//! `extract`, `gather` from `delta/services.py` and `delta/extract.py`).
//!
//! Sentiment classification (Jev) is not ported yet — see findings #3.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDateTime, Utc};
use delta_core::db::{Db, StoreItem};
use delta_core::ids::stable_id;
use delta_core::models::{Event, EventKind, Instrument, NewsItem};
use delta_core::time::parse_date;
use delta_llm::providers::Message;
use serde_json::json;

use crate::error::ServiceError;

/// Default lookback for a first ingest (`ingest`'s `since` default).
pub fn default_since(now: NaiveDateTime) -> String {
    (to_naive_utc(now) - Duration::days(365))
        .format("%Y-%m-%d")
        .to_string()
}

fn to_naive_utc(ts: NaiveDateTime) -> NaiveDateTime {
    ts
}

/// The newest stored bar date, overlapping one day, or `default` when empty
/// (`_latest_bar_floor`): bars are the only source rebuilt each run, and the
/// one-day overlap re-fetches any partial last bar.
fn latest_bar_floor(db: &Db, default: &str) -> Result<String, ServiceError> {
    let latest = db
        .bars_all_max_ts()?
        .map(|ts| to_utc(ts) - Duration::days(1));
    Ok(match latest {
        Some(ts) => ts.format("%Y-%m-%d").to_string(),
        None => default.to_string(),
    })
}

fn to_utc(ts: NaiveDateTime) -> NaiveDateTime {
    ts
}

/// Ingest result: rows actually inserted per table (`IngestResult`).
#[derive(Debug, Clone, Default)]
pub struct IngestResult {
    pub counts: BTreeMap<String, usize>,
}

/// Run every enabled data plugin over the (optionally narrowed) universe.
///
/// Bars are fetched from the newest stored bar with a one-day overlap rather
/// than the full lookback (B2). Independent sources run concurrently, capped
/// by `[plugins].ingest_workers` (default 4).
#[allow(clippy::too_many_arguments)] // the Python signature it ports is the same shape
pub async fn ingest(
    db: &mut Db,
    plugins: &[std::sync::Arc<dyn delta_plugins::DataPlugin>],
    universe: &[Instrument],
    market: Option<&str>,
    tickers: Option<&str>,
    since: Option<&str>,
    workers: usize,
    mut log: impl FnMut(&str),
) -> Result<IngestResult, ServiceError> {
    let now = Utc::now().naive_utc();
    let since = since
        .map(str::to_string)
        .unwrap_or_else(|| default_since(now));
    let since_ts = parse_date(&since)
        .ok_or_else(|| ServiceError::invalid(format!("bad since date {since:?}")))?;

    let mut instruments: Vec<Instrument> = universe.to_vec();
    if let Some(market) = market {
        instruments.retain(|i| i.market == market);
    }
    if let Some(wanted) = tickers {
        let wanted: std::collections::BTreeSet<&str> = wanted.split(',').collect();
        instruments.retain(|i| wanted.contains(i.symbol.as_str()));
    }

    let bar_since = latest_bar_floor(db, &since)?;
    let workers = workers.max(1);
    let semaphore = std::sync::Arc::new(tokio::sync::Semaphore::new(workers));

    let mut handles = Vec::new();
    for plugin in plugins {
        let target: Vec<Instrument> = {
            let filtered: Vec<Instrument> = instruments
                .iter()
                .filter(|i| plugin.market().is_none_or(|m| i.market == m))
                .cloned()
                .collect();
            plugin.universe(&filtered)
        };
        if target.is_empty() {
            continue;
        }
        let name = plugin.name().to_string();
        // B2: bars are the only source rebuilt from scratch each run.
        let fetch_since = if name == "yfinance" {
            parse_date(&bar_since).ok_or_else(|| ServiceError::invalid("bad bar_since"))?
        } else {
            since_ts
        };
        let permit = semaphore.clone().acquire_owned();
        let plugin = plugin.clone();
        let target = target.clone();
        handles.push(async move {
            let _permit = permit.await.ok();
            let result =
                plugin
                    .fetch(&target, fetch_since)
                    .await
                    .map_err(|e| ServiceError::Plugin {
                        plugin: name.clone(),
                        message: e.to_string(),
                    });
            (name, result)
        });
    }

    let mut total = IngestResult::default();
    for (name, result) in futures::future::join_all(handles).await {
        let rows: Vec<StoreItem> = result?;
        log(&format!("Ingesting via {name} (stored below)..."));
        let counts = db.store_items(&rows)?;
        for (table, count) in &counts {
            if *count > 0 {
                log(&format!("  stored {count} {table}"));
            }
            *total.counts.entry(table.clone()).or_insert(0) += count;
        }
    }
    Ok(total)
}

/// `stable_id(instrument_id, kind, summary)` — the extract id contract.
pub fn event_id(instrument_id: &str, kind: &str, summary: &str) -> String {
    stable_id(&[instrument_id, kind, summary])
}

/// Extract result (`ExtractResult`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExtractResult {
    pub events: usize,
    pub instruments: usize,
}

/// Events extracted and persisted from news published on or after `since`
/// (`delta/extract.py::extract_events`).
///
/// Items already cited by an event are not re-sent; an event citing none of
/// its provided items is dropped, because it must be traceable to them.
#[allow(clippy::too_many_arguments)]
pub async fn extract_events(
    db: &mut Db,
    client: &delta_llm::client::LlmClient,
    model: &str,
    universe: &[Instrument],
    since: NaiveDateTime,
    batch_size: usize,
) -> Result<Vec<Event>, ServiceError> {
    let covered = covered_evidence(db)?;
    let items: Vec<NewsItem> = db
        .news()?
        .into_iter()
        .filter(|n| n.published >= since && !covered.contains(&n.id))
        .collect();

    let symbols: BTreeMap<String, String> = universe
        .iter()
        .map(|i| (i.id.clone(), i.symbol.clone()))
        .collect();

    // Batch the uncovered items per instrument.
    let mut by_instrument: BTreeMap<String, Vec<NewsItem>> = BTreeMap::new();
    for item in items {
        for instrument_id in &item.instrument_ids {
            by_instrument
                .entry(instrument_id.clone())
                .or_default()
                .push(item.clone());
        }
    }

    let mut existing_ids: BTreeSet<String> = db.event_ids()?;
    let mut stored = Vec::new();

    for (instrument_id, rows) in &by_instrument {
        let symbol = symbols.get(instrument_id).cloned().unwrap_or_else(|| {
            instrument_id
                .split(':')
                .next_back()
                .unwrap_or(instrument_id)
                .to_string()
        });
        for batch in rows.chunks(batch_size.max(1)) {
            let by_id: BTreeMap<String, &NewsItem> =
                batch.iter().map(|r| (r.id.clone(), r)).collect();
            let vars = json!({
                "symbol": symbol,
                "instrument_id": instrument_id,
                "items": batch.iter().map(|r| json!({
                    "id": r.id,
                    "published": format_iso(r.published),
                    "source": r.source,
                    "title": r.title,
                    "body": r.body.clone().unwrap_or_default(),
                })).collect::<Vec<_>>(),
            });
            let (draft, _result) = match delta_llm::structured::structured::<EventBatch>(
                client,
                db,
                "extract",
                model,
                "extract_v1.j2",
                &vars,
                None,
            )
            .await
            {
                Ok(pair) => pair,
                Err(err) => {
                    log::warn!("invalid extract output for {instrument_id}: {err}; skipping batch");
                    continue;
                }
            };

            let mut new_events: Vec<StoreItem> = Vec::new();
            for ev in draft.events {
                let evidence: Vec<String> = ev
                    .evidence_ids
                    .iter()
                    .filter(|e| by_id.contains_key(*e))
                    .cloned()
                    .collect();
                if evidence.is_empty() {
                    // An event must be traceable to the items it was extracted from.
                    log::warn!("event for {instrument_id} cites no provided items; dropped");
                    continue;
                }
                let eid = event_id(instrument_id, ev.kind.as_str(), &ev.summary);
                if existing_ids.contains(&eid) {
                    continue;
                }
                existing_ids.insert(eid.clone());
                let ts = evidence
                    .iter()
                    .filter_map(|e| by_id.get(e))
                    .map(|r| r.published)
                    .max()
                    .unwrap_or_default();
                new_events.push(StoreItem::Event(Event {
                    id: eid,
                    instrument_id: instrument_id.clone(),
                    ts,
                    kind: ev.kind,
                    summary: ev.summary.clone(),
                    sentiment: ev.sentiment,
                    evidence_ids: evidence,
                    extracted_by: model.to_string(),
                    prompt_version: "extract_v1".to_string(),
                }));
            }
            // Persist per batch so a failure later in the run keeps what was
            // already paid for.
            if !new_events.is_empty() {
                db.store_items(&new_events)?;
                for item in new_events {
                    if let StoreItem::Event(event) = item {
                        stored.push(event);
                    }
                }
            }
        }
    }
    Ok(stored)
}

fn format_iso(ts: NaiveDateTime) -> String {
    // Python `to_utc(...).isoformat()` on an aware UTC datetime.
    let micros = ts.and_utc().timestamp_subsec_micros();
    if micros == 0 {
        format!("{}+00:00", ts.format("%Y-%m-%dT%H:%M:%S"))
    } else {
        format!("{}.{micros:06}+00:00", ts.format("%Y-%m-%dT%H:%M:%S"))
    }
}

/// Evidence ids cited by any existing event (`_uncovered_items`' covered set).
fn covered_evidence(db: &Db) -> Result<BTreeSet<String>, ServiceError> {
    Ok(db
        .events_all()?
        .into_iter()
        .flat_map(|e| e.evidence_ids)
        .collect())
}

/// The validated extract payload (`EventBatch` / `EventDraft`).
#[derive(Debug, serde::Deserialize)]
pub struct EventBatch {
    #[serde(default)]
    pub events: Vec<EventDraft>,
}

#[derive(Debug, serde::Deserialize)]
pub struct EventDraft {
    pub kind: EventKind,
    pub summary: String,
    #[serde(default)]
    pub sentiment: f64,
    #[cfg_attr(test, allow(dead_code))]
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

/// The full evidence pipeline minus sentiment (`gather`); see findings #3.
#[allow(clippy::too_many_arguments)]
pub async fn gather(
    db: &mut Db,
    plugins: &[std::sync::Arc<dyn delta_plugins::DataPlugin>],
    client: &delta_llm::client::LlmClient,
    model: &str,
    universe: &[Instrument],
    market: Option<&str>,
    tickers: Option<&str>,
    since: Option<&str>,
    workers: usize,
    mut log: impl FnMut(&str),
) -> Result<(IngestResult, ExtractResult), ServiceError> {
    let ingested = ingest(
        db, plugins, universe, market, tickers, since, workers, &mut log,
    )
    .await?;
    let now = Utc::now().naive_utc();
    let since_ts = match since {
        Some(s) => {
            parse_date(s).ok_or_else(|| ServiceError::invalid(format!("bad since {s:?}")))?
        }
        None => to_naive_utc(now) - Duration::days(14),
    };
    let events = extract_events(db, client, model, universe, since_ts, 20).await?;
    let instruments = events
        .iter()
        .map(|e| e.instrument_id.clone())
        .collect::<BTreeSet<_>>()
        .len();
    Ok((
        ingested,
        ExtractResult {
            events: events.len(),
            instruments,
        },
    ))
}

/// One chat turn's messages helper (the `[system, user]` pair).
pub fn chat_messages(system: &str, user: &str) -> Vec<Message> {
    vec![Message::new("system", system), Message::new("user", user)]
}
