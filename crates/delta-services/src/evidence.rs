//! Unified cited evidence read model (port of `delta/evidence.py`).

use delta_core::db::Db;
use delta_core::time::parse_date;
use rusqlite::params;

use crate::error::ServiceError;
use crate::thesis_health::{evidence_by_ids, EvidenceItem};

/// Stable citation text shown in reports and chat.
pub fn cite(item: &EvidenceItem) -> String {
    match &item.url {
        Some(url) => format!("[{}] {} <{url}>", item.source, item.title),
        None => format!("[{}] {}", item.source, item.title),
    }
}

/// Resolve a stored citation (or a verified web citation) to a browser URL.
pub fn source_url(db: &Db, id: &str) -> Result<Option<String>, ServiceError> {
    let url = if id.starts_with("http://") || id.starts_with("https://") {
        Some(id.to_string())
    } else {
        evidence_by_ids(db, &[id.to_string()])?
            .into_iter()
            .next()
            .and_then(|item| item.url)
    };
    Ok(url
        .and_then(|url| reqwest::Url::parse(&url).ok())
        .filter(|url| matches!(url.scheme(), "http" | "https"))
        .map(|url| url.to_string()))
}

/// Read the newest evidence for an instrument, or across the entire pool.
/// The per-table cap mirrors Python's read before the final merged limit.
pub fn evidence(
    db: &Db,
    target: Option<&str>,
    limit: usize,
) -> Result<Vec<EvidenceItem>, ServiceError> {
    evidence_filtered(db, target, None, None, limit, None)
}

/// Filter before limiting, including older text matches and primary filings.
pub fn evidence_filtered(
    db: &Db,
    target: Option<&str>,
    since: Option<&str>,
    kind: Option<&str>,
    limit: usize,
    search: Option<&str>,
) -> Result<Vec<EvidenceItem>, ServiceError> {
    if limit == 0 {
        return Ok(Vec::new());
    }
    let floor = since
        .map(|raw| {
            parse_date(raw).ok_or_else(|| ServiceError::invalid(format!("bad since date {raw:?}")))
        })
        .transpose()?;
    let cap = if search.is_some_and(|s| !s.is_empty()) {
        -1
    } else {
        i64::try_from(limit).unwrap_or(i64::MAX)
    };
    let mut ids = Vec::new();
    for (table, timestamp, prefix) in [
        ("bar", "ts", "bar"),
        ("event", "ts", "event"),
        ("fundamental", "as_of", "fundamental"),
    ] {
        if kind.is_some_and(|wanted| wanted != prefix) {
            continue;
        }
        let sql = format!(
            "SELECT CAST(id AS TEXT) FROM {table} WHERE (?1 IS NULL OR instrument_id = ?1) \
             AND (?2 IS NULL OR {timestamp} >= ?2) ORDER BY {timestamp} DESC, id ASC LIMIT ?3"
        );
        let mut stmt = db
            .conn()
            .prepare(&sql)
            .map_err(delta_core::db::DbError::from)?;
        let rows = stmt
            .query_map(params![target, since, cap], |row| row.get::<_, String>(0))
            .map_err(delta_core::db::DbError::from)?;
        for row in rows {
            ids.push(format!(
                "{prefix}:{}",
                row.map_err(delta_core::db::DbError::from)?
            ));
        }
    }
    if kind.is_none_or(|wanted| matches!(wanted, "news" | "filing")) {
        let mut stmt = db
            .conn()
            .prepare(
                "SELECT n.id, n.source FROM newsitem n \
             WHERE (?1 IS NULL OR EXISTS (SELECT 1 FROM news_instrument ni \
             WHERE ni.news_id = n.id AND ni.instrument_id = ?1)) \
             AND (?2 IS NULL OR n.published >= ?2) \
             AND (?3 IS NULL OR (?3 = 'filing' AND n.source IN ('sec_edgar','asx_announcements')) \
             OR (?3 = 'news' AND n.source NOT IN ('sec_edgar','asx_announcements'))) \
             ORDER BY n.published DESC, n.id ASC LIMIT ?4",
            )
            .map_err(delta_core::db::DbError::from)?;
        let rows = stmt
            .query_map(params![target, since, kind, cap], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(delta_core::db::DbError::from)?;
        for row in rows {
            let (id, source) = row.map_err(delta_core::db::DbError::from)?;
            let prefix = if crate::brief::PRIMARY_FILING_SOURCES.contains(&source.as_str()) {
                "filing"
            } else {
                "news"
            };
            ids.push(format!("{prefix}:{id}"));
        }
    }
    let mut items = Vec::new();
    for chunk in ids.chunks(400) {
        items.extend(evidence_by_ids(db, chunk)?);
    }
    items.retain(|item| floor.is_none_or(|date| item.ts >= date));
    if let Some(needle) = search.filter(|s| !s.is_empty()) {
        let needle = needle.to_lowercase();
        items.retain(|item| {
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
    items.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.id.cmp(&b.id)));
    items.truncate(limit);
    Ok(items)
}
