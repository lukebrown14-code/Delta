//! Read-side analytics over the store: data health, pulse, costs, closes,
//! headlines, upcoming events, report stamps. Ports of the matching functions
//! in `delta/services.py`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDate, NaiveDateTime};
use delta_core::db::Db;
use delta_core::json::from_json;
use delta_core::time::to_utc;

/// `FILING_SOURCE` in `delta/evidence.py`.
pub const FILING_SOURCE: &str = "sec_edgar";

/// Table row counts, the newest bar per instrument, and the newest LLM call
/// (`data_health`).
#[derive(Debug, Clone, Default)]
pub struct DataHealth {
    pub counts: BTreeMap<String, usize>,
    pub latest_bar: BTreeMap<String, NaiveDateTime>,
    pub last_llm: Option<NaiveDateTime>,
}

pub fn data_health(db: &Db) -> Result<DataHealth, delta_core::db::DbError> {
    let mut health = DataHealth::default();
    for table in ["bar", "newsitem", "event", "fundamental", "llmcall"] {
        health
            .counts
            .insert(table.to_string(), db.table_count(table)?);
    }
    health.latest_bar = latest_bars(db)?;
    health.last_llm = db.llm_lookup_all()?.iter().map(|c| c.ts).max();
    Ok(health)
}

fn latest_bars(db: &Db) -> Result<BTreeMap<String, NaiveDateTime>, delta_core::db::DbError> {
    let mut stmt = db
        .conn()
        .prepare("SELECT instrument_id, MAX(ts) FROM bar GROUP BY instrument_id")?;
    let rows = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                delta_core::db::decode_ts(&row.get::<_, String>(1)?).unwrap_or_default(),
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows.into_iter().collect())
}

/// Most recent close prices, oldest first, for sparklines (`recent_closes`).
pub fn recent_closes(
    db: &Db,
    instrument_id: &str,
    limit: usize,
) -> Result<Vec<f64>, delta_core::db::DbError> {
    let mut bars = db.bars(instrument_id)?;
    bars.sort_by(|a, b| a.ts.cmp(&b.ts));
    let start = bars.len().saturating_sub(limit);
    Ok(bars[start..].iter().map(|b| b.close).collect())
}

/// Calls and spend per (task, model) (`llm_costs` / `CostRow`).
#[derive(Debug, Clone, PartialEq)]
pub struct CostRow {
    pub task: String,
    pub model: String,
    pub calls: usize,
    pub cost_usd: f64,
}

pub fn llm_costs(
    db: &Db,
    since: Option<NaiveDateTime>,
) -> Result<Vec<CostRow>, delta_core::db::DbError> {
    let mut grouped: BTreeMap<(String, String), (usize, f64)> = BTreeMap::new();
    for call in db.llm_lookup_all()? {
        if let Some(since) = since {
            if call.ts < since {
                continue;
            }
        }
        let entry = grouped
            .entry((call.task.clone(), call.model.clone()))
            .or_insert((0, 0.0));
        entry.0 += 1;
        entry.1 += call.cost_usd;
    }
    Ok(grouped
        .into_iter()
        .map(|((task, model), (calls, cost_usd))| CostRow {
            task,
            model,
            calls,
            cost_usd,
        })
        .collect())
}

/// Total LLM spend, optionally since a date (`total_spend`).
pub fn total_spend(db: &Db, since: Option<NaiveDateTime>) -> f64 {
    llm_costs(db, since)
        .map(|rows| rows.iter().map(|r| r.cost_usd).sum())
        .unwrap_or(0.0)
}

/// What has arrived since the user last looked, plus recent activity
/// (`pulse` / `Pulse`).
#[derive(Debug, Clone)]
pub struct Pulse {
    pub since: NaiveDateTime,
    pub articles: usize,
    pub filings: usize,
    pub events: usize,
    pub daily: Vec<usize>,
    /// (instrument id, count), the busiest and quietest over the whole window.
    pub busiest: Option<(String, usize)>,
    pub quietest: Option<(String, usize)>,
}

impl Pulse {
    pub fn total(&self) -> usize {
        self.articles + self.filings + self.events
    }
}

/// Counts since `since`, a daily histogram, and a per-instrument tally.
///
/// Dates are publish/event time, not ingest time. Future-dated rows are
/// excluded (the calendar writes upcoming earnings into `event.ts`);
/// `upcoming_events` is the mirror taking exactly those rows.
pub fn pulse(
    db: &Db,
    instrument_ids: &[String],
    since: NaiveDateTime,
    days: usize,
    now: NaiveDateTime,
) -> Result<Pulse, delta_core::db::DbError> {
    let since = to_utc(since).naive_utc();
    let window_start = (to_utc(now) - Duration::days(days as i64 - 1)).naive_utc();
    // Python reads both tables with `>= floor`, where floor = min(since, window_start);
    // here the per-row filters below cover the same span without the extra variable.
    let _floor = since.min(window_start);

    let mut buckets: BTreeMap<NaiveDate, usize> = BTreeMap::new();
    let mut tally: BTreeMap<String, usize> =
        instrument_ids.iter().cloned().map(|id| (id, 0)).collect();
    let (mut articles, mut filings, mut events) = (0usize, 0usize, 0usize);

    for news in db.news()? {
        let ts = to_utc(news.published).naive_utc();
        if ts >= window_start {
            *buckets.entry(ts.date()).or_insert(0) += 1;
            // Tallied over the whole window, not just since the last visit.
            for instrument_id in &news.instrument_ids {
                if let Some(count) = tally.get_mut(instrument_id) {
                    *count += 1;
                }
            }
        }
        if ts < since {
            continue;
        }
        if news.source == FILING_SOURCE {
            filings += 1;
        } else {
            articles += 1;
        }
    }

    for event in db.events_all()? {
        let ts = to_utc(event.ts).naive_utc();
        if ts > now {
            // Future-dated rows are upcoming_events' side of `now`, not history.
            continue;
        }
        if ts >= window_start {
            *buckets.entry(ts.date()).or_insert(0) += 1;
        }
        if ts >= since {
            events += 1;
        }
    }

    let daily = (0..days)
        .map(|n| {
            let day = (to_utc(window_start) + Duration::days(n as i64))
                .naive_utc()
                .date();
            buckets.get(&day).copied().unwrap_or(0)
        })
        .collect();
    // Most articles first; ties break on id (Python's `(-count, id)` sort).
    let ranked: Vec<(String, usize)> = {
        let mut pairs: Vec<(String, usize)> = tally.into_iter().collect();
        pairs.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        pairs
    };
    // With nothing at all in the window there is no busiest to name.
    let quiet = ranked.is_empty() || ranked[0].1 == 0;
    Ok(Pulse {
        since: to_utc(since).naive_utc(),
        articles,
        filings,
        events,
        daily,
        busiest: if quiet { None } else { Some(ranked[0].clone()) },
        quietest: if quiet || ranked.len() < 2 {
            None
        } else {
            Some(ranked[ranked.len() - 1].clone())
        },
    })
}

/// A scheduled event that has not happened yet (`Upcoming`).
#[derive(Debug, Clone, PartialEq)]
pub struct Upcoming {
    pub instrument_id: String,
    pub ts: NaiveDateTime,
    pub kind: String,
    pub summary: String,
}

/// The next scheduled events for `instrument_ids`, soonest first
/// (`upcoming_events`; the mirror of [`pulse`]).
pub fn upcoming_events(
    db: &Db,
    instrument_ids: &[String],
    limit: usize,
    now: NaiveDateTime,
) -> Result<Vec<Upcoming>, delta_core::db::DbError> {
    if instrument_ids.is_empty() {
        return Ok(Vec::new()); // nothing to ask for
    }
    let mut rows: Vec<Upcoming> = db
        .events_all()?
        .into_iter()
        .filter(|e| instrument_ids.contains(&e.instrument_id) && e.ts > now)
        .map(|e| Upcoming {
            instrument_id: e.instrument_id,
            ts: e.ts,
            kind: e.kind.as_str().to_string(),
            summary: e.summary,
        })
        .collect();
    rows.sort_by(|a, b| a.ts.cmp(&b.ts));
    rows.truncate(limit);
    Ok(rows)
}

/// The newest news title, for the Home overview (`latest_headline` /
/// `Headline`).
#[derive(Debug, Clone, PartialEq)]
pub struct Headline {
    pub title: String,
    pub ts: NaiveDateTime,
    pub instrument_ids: Vec<String>,
}

/// Newest news item touching `instrument_ids` (any item when empty).
///
/// Reads the newest `scan` rows by the indexed `published` column and filters
/// in memory: the JSON column is unindexed.
pub fn latest_headline(
    db: &Db,
    instrument_ids: &[String],
    scan: usize,
) -> Result<Option<Headline>, delta_core::db::DbError> {
    let mut news = db.news()?;
    news.sort_by(|a, b| b.published.cmp(&a.published));
    let wanted: std::collections::BTreeSet<&String> = instrument_ids.iter().collect();
    for item in news.into_iter().take(scan) {
        let ids = from_json(Some(&delta_core::json::to_json(&item.instrument_ids)));
        if wanted.is_empty() || ids.iter().any(|id| wanted.contains(id)) {
            return Ok(Some(Headline {
                title: item.title,
                ts: item.published,
                instrument_ids: ids,
            }));
        }
    }
    Ok(None)
}

/// The newest report written for a company, and the moment it describes
/// (`ReportStamp` / `latest_report`).
///
/// `as_of` comes from the `.json` sidecar when there is one; without it the
/// filename (`YYYY-MM-DD.md`) is the only date on offer, and a name that is
/// not a date leaves `as_of` None rather than inventing an age.
#[derive(Debug, Clone)]
pub struct ReportStamp {
    pub path: PathBuf,
    pub as_of: Option<NaiveDateTime>,
}

/// The newest report on disk for `company`, or None when there is none.
pub fn latest_report(reports_dir: &Path, company: &str) -> Option<ReportStamp> {
    let dir = reports_dir.join(company);
    let mut paths: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map(|e| e == "md").unwrap_or(false))
        .collect();
    paths.sort();
    let newest = paths.pop()?;
    let sidecar = newest.with_extension("json");
    let sidecar_as_of = std::fs::read_to_string(&sidecar)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
        .and_then(|value| {
            value
                .get("as_of")
                .and_then(|v| v.as_str())
                .map(str::to_string)
        })
        .and_then(|as_of| chrono::DateTime::parse_from_rfc3339(&as_of).ok())
        .map(|parsed| parsed.naive_utc());
    if let Some(as_of) = sidecar_as_of {
        return Some(ReportStamp {
            path: newest,
            as_of: Some(as_of),
        });
    }
    // Sidecar absent/unreadable: fall back to the filename date.
    if let Ok(date) = NaiveDate::parse_from_str(&newest.file_stem()?.to_string_lossy(), "%Y-%m-%d")
    {
        return Some(ReportStamp {
            path: newest,
            as_of: date.and_hms_opt(0, 0, 0),
        });
    }
    Some(ReportStamp {
        path: newest,
        as_of: None,
    })
}
