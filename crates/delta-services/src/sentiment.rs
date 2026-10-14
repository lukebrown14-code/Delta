//! Bull / bear / neutral news stance via the Jev decision model.
//! Port of `delta/sentiment.py`.
//!
//! One Jev choice question per news item per instrument: does the item read
//! bullish, bearish, or neutral for that stock? A judgment is keyed by
//! instrument plus evidence id, so a re-run classifies only new items, and
//! every stance stays traceable to the news row it came from.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDateTime, Utc};
use delta_core::config::AppConfig;
use delta_core::db::Db;
use delta_core::ids::stable_id;
use delta_core::models::{Instrument, NewsItem};
use delta_llm::jev::{ChoiceQuestion, JevClient, PROMPT_VERSION};
use delta_llm::router::model_for;
use serde_json::{json, Value};

use crate::error::ServiceError;

pub const SENTIMENT_TASK: &str = "sentiment";
pub const STANCES: [&str; 3] = ["bull", "bear", "neutral"];

fn stance_value(stance: &str) -> f64 {
    match stance {
        "bull" => 1.0,
        "bear" => -1.0,
        _ => 0.0,
    }
}

/// Lookback when `since` is not given; matches the extract stage's default.
pub const DEFAULT_SINCE_DAYS: i64 = 14;

/// Recency half-life for weighting: a week-old article counts half as much
/// as the same article published today, all else equal.
pub const HALF_LIFE_DAYS: f64 = 7.0;

/// The one stance question asked of every news item (`STANCE_QUESTION`).
pub fn stance_question() -> ChoiceQuestion {
    ChoiceQuestion::new(
        "stance",
        "Reading only this news item, does it read bullish or bearish for the stock?",
        vec![
            (
                "bull".to_string(),
                "Clearly positive for the stock: beats, upgrades, growth, contract wins, \
                 strong demand."
                    .to_string(),
            ),
            (
                "bear".to_string(),
                "Clearly negative for the stock: misses, guidance cuts, losses, lawsuits, \
                 weak demand."
                    .to_string(),
            ),
            (
                "neutral".to_string(),
                "Mixed, routine, or without a clear effect on the stock.".to_string(),
            ),
        ],
    )
}

/// Recency- and confidence-weighted stance over a window (`SentimentSummary`).
#[derive(Debug, Clone, PartialEq)]
pub struct SentimentSummary {
    pub instrument_id: String,
    pub days: i64,
    pub score: f64,
    pub bull: usize,
    pub bear: usize,
    pub neutral: usize,
}

/// One stored judgment (the `sentiment` table row).
#[derive(Debug, Clone, PartialEq)]
pub struct SentimentRow {
    pub id: String,
    pub instrument_id: String,
    pub evidence_id: String,
    pub ts: NaiveDateTime,
    pub stance: String,
    pub confidence: f64,
    pub probabilities: String,
    pub model: String,
    pub prompt_version: String,
}

/// The answer's choice as a stance; anything unexpected reads neutral.
pub fn stance_of(answer: &Value) -> &str {
    let choice = answer["choice"].as_str().unwrap_or("neutral");
    if STANCES.contains(&choice) {
        choice
    } else {
        "neutral"
    }
}

pub fn judgment_id(instrument_id: &str, evidence_id: &str) -> String {
    stable_id(&[instrument_id, evidence_id])
}

/// News published since `since` paired with instruments not yet judged
/// (`_unclassified`).
fn unclassified(
    db: &Db,
    since: NaiveDateTime,
) -> Result<Vec<(NewsItem, String)>, delta_core::db::DbError> {
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    {
        let mut stmt = db
            .conn()
            .prepare("SELECT instrument_id, evidence_id FROM sentiment")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (i, e) = row?;
            seen.insert((i, e));
        }
    }
    let mut news = db.news()?;
    news.retain(|item| item.published >= since);
    news.sort_by(|a, b| a.published.cmp(&b.published));

    let mut pairs = Vec::new();
    for item in news {
        for instrument_id in &item.instrument_ids {
            if seen.insert((instrument_id.clone(), item.id.clone())) {
                pairs.push((item.clone(), instrument_id.clone()));
            }
        }
    }
    Ok(pairs)
}

fn state_json(item: &NewsItem, instrument_id: &str, symbol: &str) -> Value {
    json!({
        "instrument_id": instrument_id,
        "symbol": symbol,
        "published": delta_core::time::to_utc(item.published).to_rfc3339(),
        "source": item.source,
        "title": item.title,
        "body": item.body.clone().unwrap_or_default(),
        "evidence_id": item.id,
    })
}

fn row_of(item: &NewsItem, instrument_id: &str, model: &str, answer: &Value) -> SentimentRow {
    let probabilities = &answer["probabilities"];
    let weights: BTreeMap<String, f64> = probabilities
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0)))
                .collect()
        })
        .unwrap_or_default();
    let confidence = answer["confidence"].as_f64().unwrap_or(0.0).clamp(0.0, 1.0);
    SentimentRow {
        id: judgment_id(instrument_id, &item.id),
        instrument_id: instrument_id.to_string(),
        evidence_id: item.id.clone(),
        ts: item.published,
        stance: stance_of(answer).to_string(),
        confidence,
        probabilities: serde_json::to_string(&weights).unwrap_or_else(|_| "{}".to_string()),
        model: model.to_string(),
        prompt_version: PROMPT_VERSION.to_string(),
    }
}

fn store_row(db: &mut Db, row: &SentimentRow) -> Result<(), delta_core::db::DbError> {
    db.conn().execute(
        "INSERT OR REPLACE INTO sentiment \
         (id, instrument_id, evidence_id, ts, stance, confidence, probabilities, model, prompt_version) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            row.id,
            row.instrument_id,
            row.evidence_id,
            row.ts.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
            row.stance,
            row.confidence,
            row.probabilities,
            row.model,
            row.prompt_version,
        ],
    )?;
    Ok(())
}

/// Ask Jev for a stance on every unclassified news item; persist judgments
/// (`classify_news`). The model comes from `[llm.routing].sentiment`. A
/// failed request skips its item; the next run picks it up, since only
/// classified items are excluded.
pub async fn classify_news(
    db: &mut Db,
    cfg: &AppConfig,
    openrouter_api_key: &str,
    universe: &[Instrument],
    since: Option<NaiveDateTime>,
    log: Option<&dyn Fn(&str)>,
) -> Result<Vec<SentimentRow>, ServiceError> {
    classify_impl(db, cfg, openrouter_api_key, universe, since, log, None).await
}

async fn classify_impl(
    db: &mut Db,
    cfg: &AppConfig,
    openrouter_api_key: &str,
    universe: &[Instrument],
    since: Option<NaiveDateTime>,
    log: Option<&dyn Fn(&str)>,
    base_url_override: Option<String>,
) -> Result<Vec<SentimentRow>, ServiceError> {
    let model =
        model_for(cfg, SENTIMENT_TASK, None).map_err(|e| ServiceError::invalid(e.to_string()))?;
    let since =
        since.unwrap_or_else(|| Utc::now().naive_utc() - Duration::days(DEFAULT_SINCE_DAYS));
    let pairs = unclassified(db, since)?;
    if pairs.is_empty() {
        return Ok(Vec::new());
    }
    let symbols: BTreeMap<&str, &str> = universe
        .iter()
        .map(|inst| (inst.id.as_str(), inst.symbol.as_str()))
        .collect();
    let mut client = JevClient::new(openrouter_api_key);
    if let Some(base) = base_url_override {
        client = client.with_base_url(&base);
    }
    let question = stance_question();
    let mut stored: Vec<SentimentRow> = Vec::new();
    for (item, instrument_id) in &pairs {
        let symbol = symbols
            .get(instrument_id.as_str())
            .copied()
            .unwrap_or_else(|| instrument_id.rsplit(':').next().unwrap_or(instrument_id));
        let decision = match client
            .decide_with_model(
                db,
                SENTIMENT_TASK,
                &state_json(item, instrument_id, symbol),
                std::slice::from_ref(&question),
                &model,
            )
            .await
        {
            Ok(decision) => decision,
            Err(e) => {
                log::warn!("jev stance failed for {instrument_id}/{}: {e}", item.id);
                continue;
            }
        };
        let answer = decision
            .answers
            .get("stance")
            .cloned()
            .unwrap_or_else(|| json!({}));
        stored.push(row_of(item, instrument_id, &decision.model, &answer));
    }
    for row in &stored {
        store_row(db, row)?;
    }
    if let Some(say) = log {
        say(&format!(
            "Classified {} news stances via {model} (bull/bear/neutral).",
            stored.len()
        ));
    }
    Ok(stored)
}

/// The services-level wrapper (`services.classify_sentiment`): run the
/// classification and report how many stances and instruments were covered.
pub async fn classify_sentiment(
    db: &mut Db,
    cfg: &AppConfig,
    openrouter_api_key: &str,
    universe: &[Instrument],
    since: Option<NaiveDateTime>,
    log: Option<&dyn Fn(&str)>,
) -> Result<(usize, usize), ServiceError> {
    let rows = classify_news(db, cfg, openrouter_api_key, universe, since, log).await?;
    let instruments = rows
        .iter()
        .map(|r| r.instrument_id.as_str())
        .collect::<BTreeSet<_>>()
        .len();
    Ok((rows.len(), instruments))
}

/// Test-only endpoint overrides for the wiremock suites; not part of the
/// Python-facing API.
pub mod test_hooks {
    use super::*;

    pub async fn classify_with_url(
        db: &mut Db,
        cfg: &AppConfig,
        openrouter_api_key: &str,
        universe: &[Instrument],
        since: Option<NaiveDateTime>,
        decisions_url: &str,
    ) -> Result<Vec<SentimentRow>, ServiceError> {
        classify_with_base(
            db,
            cfg,
            openrouter_api_key,
            universe,
            since,
            decisions_url,
            None,
        )
        .await
    }

    pub async fn classify_sentiment_with_url(
        db: &mut Db,
        cfg: &AppConfig,
        openrouter_api_key: &str,
        universe: &[Instrument],
        since: Option<NaiveDateTime>,
        decisions_url: &str,
        log: Option<&dyn Fn(&str)>,
    ) -> Result<(usize, usize), ServiceError> {
        let rows = classify_with_base(
            db,
            cfg,
            openrouter_api_key,
            universe,
            since,
            decisions_url,
            log,
        )
        .await?;
        let instruments = rows
            .iter()
            .map(|r| r.instrument_id.as_str())
            .collect::<BTreeSet<_>>()
            .len();
        Ok((rows.len(), instruments))
    }
}

/// Strip the Decisions path off a test server URL and classify against it.
async fn classify_with_base(
    db: &mut Db,
    cfg: &AppConfig,
    openrouter_api_key: &str,
    universe: &[Instrument],
    since: Option<NaiveDateTime>,
    decisions_url: &str,
    log: Option<&dyn Fn(&str)>,
) -> Result<Vec<SentimentRow>, ServiceError> {
    let base = decisions_url
        .rsplit_once("/api/alpha/decisions")
        .map(|(base, _)| base.to_string())
        .ok_or_else(|| {
            ServiceError::invalid(format!(
                "test url must end in /api/alpha/decisions: {decisions_url}"
            ))
        })?;
    classify_impl(
        db,
        cfg,
        openrouter_api_key,
        universe,
        since,
        log,
        Some(base),
    )
    .await
}

/// Weighted stance summary over the last `days` days; `None` with no rows
/// (`stock_sentiment`). Weight is confidence times a recency half-life, so a
/// confident fresh article moves the score more than a timid stale one. The
/// score runs -1 (all bear) to 1 (all bull); the counts are unweighted.
pub fn stock_sentiment(
    db: &Db,
    instrument_id: &str,
    days: i64,
    now: Option<NaiveDateTime>,
) -> Result<Option<SentimentSummary>, delta_core::db::DbError> {
    let now = now.unwrap_or_else(|| Utc::now().naive_utc());
    let floor = now - Duration::days(days);
    let mut stmt = db.conn().prepare(
        "SELECT ts, stance, confidence FROM sentiment \
         WHERE instrument_id = ?1 AND ts >= ?2 AND ts <= ?3",
    )?;
    let rows = stmt.query_map(
        rusqlite::params![
            instrument_id,
            floor.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
            now.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
        ],
        |row| {
            Ok((
                delta_core::db::decode_ts(&row.get::<_, String>(0)?).unwrap_or_default(),
                row.get::<_, String>(1)?,
                row.get::<_, f64>(2)?,
            ))
        },
    )?;
    let mut weight_sum = 0.0;
    let mut score_sum = 0.0;
    let mut counts: BTreeMap<String, usize> = STANCES.iter().map(|s| (s.to_string(), 0)).collect();
    let mut any = false;
    for row in rows {
        let (ts, stance, confidence) = row?;
        any = true;
        let age_days = (now - ts).num_milliseconds() as f64 / 86_400_000.0;
        let weight = 0.5f64.powf(age_days / HALF_LIFE_DAYS) * confidence.max(0.0);
        weight_sum += weight;
        score_sum += stance_value(&stance) * weight;
        if STANCES.contains(&stance.as_str()) {
            let c = counts[stance.as_str()];
            counts.insert(stance.clone(), c + 1);
        }
    }
    if !any {
        return Ok(None);
    }
    let score = if weight_sum > 0.0 {
        score_sum / weight_sum
    } else {
        0.0
    };
    Ok(Some(SentimentSummary {
        instrument_id: instrument_id.to_string(),
        days,
        score,
        bull: counts["bull"],
        bear: counts["bear"],
        neutral: counts["neutral"],
    }))
}
