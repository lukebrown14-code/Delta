//! Unified read model over the source tables: one pool of cited evidence.
//! Port of `delta/evidence.py`.
//!
//! The existing tables (`bar`, `newsitem`, `event`, `fundamental`) keep their
//! write paths and data plugins untouched; this module flattens them into
//! [`EvidenceItem`] rows that reports and chat read. Also home of
//! `evidence_by_ids` (moved here from `thesis_health.rs`, its only R2 caller).

use chrono::NaiveDateTime;
use serde_json::{json, Value};

use delta_core::db::Db;
use delta_core::time::parse_date;

/// `FILING_SOURCE` remains for compatibility with existing callers. New
/// consumers should use [`PRIMARY_FILING_SOURCES`]: ASX announcements are
/// primary disclosures too.
pub const FILING_SOURCE: &str = "sec_edgar";

pub const PRIMARY_FILING_SOURCES: [&str; 2] = ["sec_edgar", "asx_announcements"];

/// Alias used by configurable-source consumers. A source is primary only when
/// it is a recognised direct disclosure feed; installed RSS sources stay
/// secondary evidence.
pub const PRIMARY_DISCLOSURE_SOURCES: [&str; 2] = PRIMARY_FILING_SOURCES;

pub fn source_quality(source: &str) -> &'static str {
    if PRIMARY_DISCLOSURE_SOURCES.contains(&source) {
        "primary"
    } else {
        "secondary"
    }
}

/// True when any falsifier `terms` (already casefolded) appear in the item.
///
/// The single matcher shared by the review queue and thesis health: it
/// searches kind, title and body together so the two callers can never
/// disagree about what counts as a hit. Empty terms never match.
pub fn falsifier_hit(item: &EvidenceItem, terms: &[String]) -> bool {
    if terms.is_empty() {
        return false;
    }
    let haystack = format!(
        "{} {} {}",
        item.kind,
        item.title,
        item.body.as_deref().unwrap_or("")
    )
    .to_lowercase();
    terms.iter().any(|term| haystack.contains(term))
}

/// One sourced fact, normalised from any source table (`EvidenceItem`).
#[derive(Debug, Clone)]
pub struct EvidenceItem {
    pub id: String,
    pub target_ids: Vec<String>,
    pub ts: NaiveDateTime,
    pub kind: String,
    pub title: String,
    pub body: Option<String>,
    pub source: String,
    pub url: Option<String>,
    pub sentiment: Option<f64>,
    pub raw: Value,
    pub quality: String,
}

/// Deterministic cite text: `[source] title <url>`, url omitted when absent.
pub fn cite(item: &EvidenceItem) -> String {
    let mut text = format!("[{}] {}", item.source, item.title);
    if let Some(url) = &item.url {
        text.push_str(&format!(" <{url}>"));
    }
    text
}

/// Read evidence across all source tables (`evidence`).
///
/// `target` keeps items whose `target_ids` contain it; `since` is an
/// inclusive `YYYY-MM-DD` floor on `ts`; `kind` filters the mapped kind.
/// `search` matches title, body and source case-insensitively before
/// limiting. Results are ordered `ts` desc then `id` asc, truncated to
/// `limit`.
#[allow(clippy::too_many_arguments)]
pub fn evidence(
    db: &Db,
    target: Option<&str>,
    since: Option<&str>,
    kind: Option<&str>,
    limit: usize,
    search: Option<&str>,
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    // Search must see older matches before the final result limit.
    let read_limit: Option<usize> = if search.is_none() { Some(limit) } else { None };
    let mut items: Vec<EvidenceItem> = Vec::new();
    if kind.is_none() || kind == Some("bar") {
        items.extend(read_bars(db, target, since, read_limit)?);
    }
    if kind.is_none() || kind == Some("news") || kind == Some("filing") {
        items.extend(read_news(db, target, since, kind, read_limit)?);
    }
    if kind.is_none() || kind == Some("event") {
        items.extend(read_events(db, target, since, read_limit)?);
    }
    if kind.is_none() || kind == Some("fundamental") {
        items.extend(read_fundamentals(db, target, since, read_limit)?);
    }
    let floor = since.and_then(parse_date);
    let mut matched: Vec<EvidenceItem> = items
        .into_iter()
        .filter(|item| {
            target.is_none_or(|t| item.target_ids.iter().any(|id| id == t))
                && floor.is_none_or(|f| item.ts >= f)
                && kind.is_none_or(|k| item.kind == k)
        })
        .collect();
    if let Some(needle) = search {
        let needle = needle.to_lowercase();
        matched.retain(|item| {
            format!(
                "{} {} {}",
                item.title,
                item.body.as_deref().unwrap_or(""),
                item.source
            )
            .to_lowercase()
            .contains(&needle)
        });
    }
    let mut matched = ordered(matched);
    matched.truncate(limit);
    Ok(matched)
}

/// Evidence items for the given ids, newest first; unknown ids are skipped
/// (`evidence_by_ids`).
///
/// [`evidence`] truncates to the newest `limit` items, so callers holding
/// specific ids (thesis links, report cites) read them through here instead:
/// an item that has aged out of the pool window is still found.
pub fn evidence_by_ids(
    db: &Db,
    ids: &[String],
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let mut by_prefix: std::collections::BTreeMap<String, Vec<String>> = Default::default();
    for id in ids {
        if let Some((prefix, rest)) = id.split_once(':') {
            if !rest.is_empty() {
                by_prefix
                    .entry(prefix.to_string())
                    .or_default()
                    .push(rest.to_string());
            }
        }
    }
    let mut items: Vec<EvidenceItem> = Vec::new();
    if let Some(bar_ids) = by_prefix.get("bar") {
        items.extend(query_bars(db, bar_ids)?);
    }
    if let Some(event_ids) = by_prefix.get("event") {
        items.extend(query_events(db, event_ids)?);
    }
    if let Some(fundamental_ids) = by_prefix.get("fundamental") {
        items.extend(query_fundamentals(db, fundamental_ids)?);
    }
    // news and filing are the same table, split only by source.
    let mut news_ids: Vec<String> = by_prefix.get("news").cloned().unwrap_or_default();
    news_ids.extend(by_prefix.get("filing").cloned().unwrap_or_default());
    if !news_ids.is_empty() {
        items.extend(query_news(db, &news_ids)?);
    }
    Ok(ordered(items))
}

/// Newest first; items sharing a timestamp keep ascending id order.
pub(crate) fn ordered(mut items: Vec<EvidenceItem>) -> Vec<EvidenceItem> {
    items.sort_by(|a, b| a.id.cmp(&b.id));
    items.sort_by(|a, b| b.ts.cmp(&a.ts));
    items
}

fn in_clause(ids: &[String]) -> String {
    // Parameter placeholders, never interpolated values (SQL safety).
    ids.iter().map(|_| "?").collect::<Vec<_>>().join(", ")
}

fn limit_clause(limit: Option<usize>) -> String {
    limit.map(|n| format!(" LIMIT {n}")).unwrap_or_default()
}

/// The `since` floor as a stored datetime string (`parse_date` yields a
/// midnight naive datetime, the inclusive floor).
fn encode_floor(floor: NaiveDateTime) -> String {
    floor.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

/// Bind parameters for the shared `(target, floor)` filter pair.
fn filter_params(
    target: Option<&str>,
    floor: Option<NaiveDateTime>,
    encode: fn(NaiveDateTime) -> String,
) -> Vec<String> {
    let mut params: Vec<String> = Vec::new();
    if let Some(t) = target {
        params.push(t.to_string());
    }
    if let Some(f) = floor {
        params.push(encode(f));
    }
    params
}

fn run_mapped(
    db: &Db,
    sql: &str,
    params: Vec<String>,
    map: fn(&rusqlite::Row<'_>) -> Result<EvidenceItem, rusqlite::Error>,
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let mut stmt = db.conn().prepare(sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(params.iter()), map)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn read_bars(
    db: &Db,
    target: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let mut sql = String::from(
        "SELECT id, instrument_id, ts, open, high, low, close, volume, source FROM bar WHERE 1=1",
    );
    if target.is_some() {
        sql.push_str(" AND instrument_id = ?");
    }
    let floor = since.and_then(parse_date);
    if floor.is_some() {
        sql.push_str(" AND ts >= ?");
    }
    sql.push_str(&format!(" ORDER BY ts DESC, id ASC{}", limit_clause(limit)));
    run_mapped(
        db,
        &sql,
        filter_params(target, floor, encode_floor),
        bar_item,
    )
}

fn read_news(
    db: &Db,
    target: Option<&str>,
    since: Option<&str>,
    kind: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let mut sql = String::from(
        "SELECT n.id, n.instrument_ids, n.published, n.title, n.body, n.source, n.url FROM newsitem n",
    );
    if target.is_some() {
        // Indexed join through the news_instrument mapping instead of a LIKE
        // scan over the unindexed `instrument_ids` JSON column.
        sql.push_str(" JOIN news_instrument ni ON ni.news_id = n.id WHERE ni.instrument_id = ?");
    } else {
        sql.push_str(" WHERE 1=1");
    }
    let floor = since.and_then(parse_date);
    if floor.is_some() {
        sql.push_str(" AND n.published >= ?");
    }
    if kind == Some("news") {
        sql.push_str(" AND n.source NOT IN ('sec_edgar', 'asx_announcements')");
    } else if kind == Some("filing") {
        sql.push_str(" AND n.source IN ('sec_edgar', 'asx_announcements')");
    }
    sql.push_str(&format!(
        " ORDER BY n.published DESC, n.id ASC{}",
        limit_clause(limit)
    ));
    run_mapped(
        db,
        &sql,
        filter_params(target, floor, encode_floor),
        news_item,
    )
}

fn read_events(
    db: &Db,
    target: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let mut sql = String::from(
        "SELECT id, instrument_id, ts, kind, summary, sentiment, evidence_ids, extracted_by, prompt_version FROM event WHERE 1=1",
    );
    if target.is_some() {
        sql.push_str(" AND instrument_id = ?");
    }
    let floor = since.and_then(parse_date);
    if floor.is_some() {
        sql.push_str(" AND ts >= ?");
    }
    sql.push_str(&format!(" ORDER BY ts DESC, id ASC{}", limit_clause(limit)));
    run_mapped(
        db,
        &sql,
        filter_params(target, floor, encode_floor),
        event_item,
    )
}

fn read_fundamentals(
    db: &Db,
    target: Option<&str>,
    since: Option<&str>,
    limit: Option<usize>,
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let mut sql = String::from(
        "SELECT id, instrument_id, as_of, metric, value, source FROM fundamental WHERE 1=1",
    );
    if target.is_some() {
        sql.push_str(" AND instrument_id = ?");
    }
    let floor = since.and_then(parse_date);
    if floor.is_some() {
        // The column is a plain DATE; the floor compares as its date.
        sql.push_str(" AND as_of >= ?");
    }
    sql.push_str(&format!(
        " ORDER BY as_of DESC, id ASC{}",
        limit_clause(limit)
    ));
    fn encode_date_floor(floor: NaiveDateTime) -> String {
        floor.format("%Y-%m-%d").to_string()
    }
    run_mapped(
        db,
        &sql,
        filter_params(target, floor, encode_date_floor),
        fundamental_item,
    )
}

fn bar_item(row: &rusqlite::Row<'_>) -> Result<EvidenceItem, rusqlite::Error> {
    let id: i64 = row.get(0)?;
    let instrument_id: String = row.get(1)?;
    let ts: String = row.get(2)?;
    let (open, high, low, close, volume): (f64, f64, f64, f64, f64) = (
        row.get(3)?,
        row.get(4)?,
        row.get(5)?,
        row.get(6)?,
        row.get(7)?,
    );
    let source: String = row.get(8)?;
    Ok(EvidenceItem {
        id: format!("bar:{id}"),
        target_ids: vec![instrument_id.clone()],
        ts: delta_core::db::decode_ts(&ts).unwrap_or_default(),
        kind: "bar".to_string(),
        title: format!("{instrument_id} close {close:.2}"),
        body: None,
        source,
        url: None,
        sentiment: None,
        raw: json!({
            "open": open,
            "high": high,
            "low": low,
            "close": close,
            "volume": volume,
        }),
        quality: "secondary".to_string(),
    })
}

fn news_item(row: &rusqlite::Row<'_>) -> Result<EvidenceItem, rusqlite::Error> {
    let id: String = row.get(0)?;
    let instrument_ids = delta_core::json::from_json(Some(&row.get::<_, String>(1)?));
    let published: String = row.get(2)?;
    let title: String = row.get(3)?;
    let body: Option<String> = row.get(4)?;
    let source: String = row.get(5)?;
    let url: Option<String> = row.get(6)?;
    let kind = if PRIMARY_FILING_SOURCES.contains(&source.as_str()) {
        "filing"
    } else {
        "news"
    };
    let quality = source_quality(&source).to_string();
    Ok(EvidenceItem {
        id: format!("{kind}:{id}"),
        target_ids: instrument_ids.clone(),
        ts: delta_core::db::decode_ts(&published).unwrap_or_default(),
        kind: kind.to_string(),
        title,
        body,
        source,
        url,
        sentiment: None,
        raw: json!({ "instrument_ids": instrument_ids }),
        quality,
    })
}

fn event_item(row: &rusqlite::Row<'_>) -> Result<EvidenceItem, rusqlite::Error> {
    let id: String = row.get(0)?;
    let instrument_id: String = row.get(1)?;
    let ts: String = row.get(2)?;
    let kind: String = row.get(3)?;
    let summary: String = row.get(4)?;
    let sentiment: f64 = row.get(5)?;
    let evidence_ids = delta_core::json::from_json(Some(&row.get::<_, String>(6)?));
    let extracted_by: String = row.get(7)?;
    let prompt_version: String = row.get(8)?;
    Ok(EvidenceItem {
        id: format!("event:{id}"),
        target_ids: vec![instrument_id],
        ts: delta_core::db::decode_ts(&ts).unwrap_or_default(),
        kind: "event".to_string(),
        title: format!("{kind}: {summary}"),
        body: None,
        source: extracted_by.clone(),
        url: None,
        sentiment: Some(sentiment),
        raw: json!({
            "kind": kind,
            "summary": summary,
            "evidence_ids": evidence_ids,
            "extracted_by": extracted_by,
            "prompt_version": prompt_version,
        }),
        quality: "secondary".to_string(),
    })
}

fn fundamental_item(row: &rusqlite::Row<'_>) -> Result<EvidenceItem, rusqlite::Error> {
    let id: i64 = row.get(0)?;
    let instrument_id: String = row.get(1)?;
    let as_of: String = row.get(2)?;
    let metric: String = row.get(3)?;
    let value: f64 = row.get(4)?;
    let source: String = row.get(5)?;
    Ok(EvidenceItem {
        id: format!("fundamental:{id}"),
        target_ids: vec![instrument_id],
        ts: parse_date(&as_of).unwrap_or_default(),
        kind: "fundamental".to_string(),
        title: format!("{metric}: {value:.2} ({as_of}, {source})"),
        body: None,
        source: source.clone(),
        url: None,
        sentiment: None,
        raw: json!({
            "metric": metric,
            "value": value,
            "as_of": as_of,
        }),
        quality: "secondary".to_string(),
    })
}

fn query_bars(db: &Db, ids: &[String]) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let sql = format!(
        "SELECT id, instrument_id, ts, open, high, low, close, volume, source FROM bar WHERE id IN ({})",
        in_clause(ids)
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), bar_item)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn query_news(db: &Db, ids: &[String]) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    // Same column order as `read_news`, so one mapper serves both.
    let sql = format!(
        "SELECT id, instrument_ids, published, title, body, source, url FROM newsitem WHERE id IN ({})",
        in_clause(ids)
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), news_item)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn query_events(db: &Db, ids: &[String]) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let sql = format!(
        "SELECT id, instrument_id, ts, kind, summary, sentiment, evidence_ids, extracted_by, prompt_version FROM event WHERE id IN ({})",
        in_clause(ids)
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), event_item)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

fn query_fundamentals(
    db: &Db,
    ids: &[String],
) -> Result<Vec<EvidenceItem>, delta_core::db::DbError> {
    let sql = format!(
        "SELECT id, instrument_id, as_of, metric, value, source FROM fundamental WHERE id IN ({})",
        in_clause(ids)
    );
    let mut stmt = db.conn().prepare(&sql)?;
    let rows = stmt.query_map(rusqlite::params_from_iter(ids), fundamental_item)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}
