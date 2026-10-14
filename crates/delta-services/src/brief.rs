//! Facts-only instrument brief assembled from the database.
//! Port of `delta/brief.py`.
//!
//! Every Phase 2 workstream either feeds one of these sections (by writing to
//! its table) or consumes the rendered brief (strategies). Each section reads
//! exactly one table so new data sources never need to touch this module.

use chrono::{Duration, NaiveDate, NaiveDateTime};

use delta_core::db::Db;
use delta_core::models::Instrument;

pub const NEWS_WINDOW_DAYS: i64 = 14;
pub const EVENT_WINDOW_DAYS: i64 = 14;
pub const NEWS_LIMIT: usize = 20;
pub const BAR_WINDOW: usize = 120;
pub const EVIDENCE_BARS: usize = 60;

/// `PRIMARY_FILING_SOURCES` in `delta/evidence.py`: ASX announcements are
/// primary disclosures too.
pub const PRIMARY_FILING_SOURCES: [&str; 2] = ["sec_edgar", "asx_announcements"];

#[derive(Debug, Clone, Default)]
pub struct Section {
    pub title: String,
    pub lines: Vec<String>,
    pub evidence_ids: Vec<String>,
}

impl Section {
    fn titled(title: &str) -> Self {
        Self {
            title: title.to_string(),
            lines: Vec::new(),
            evidence_ids: Vec::new(),
        }
    }

    pub fn render(&self) -> String {
        if self.lines.is_empty() {
            return format!("{}: none available", self.title);
        }
        self.lines.join("\n")
    }
}

#[derive(Debug, Clone)]
pub struct Brief {
    pub instrument: Instrument,
    pub as_of: NaiveDateTime,
    pub prices: Section,
    pub news: Section,
    pub events: Section,
    pub fundamentals: Section,
    pub calendar: Section,
}

impl Brief {
    /// Price lines first with no title, matching the Phase 1 brief exactly.
    pub fn sections(&self) -> [&Section; 5] {
        [
            &self.prices,
            &self.fundamentals,
            &self.events,
            &self.calendar,
            &self.news,
        ]
    }

    pub fn render(&self) -> String {
        self.sections()
            .iter()
            .map(|s| s.render())
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Ordered union of every section's evidence ids.
    pub fn evidence_ids(&self) -> Vec<String> {
        let mut seen = std::collections::BTreeSet::new();
        let mut ids = Vec::new();
        for section in self.sections() {
            for id in &section.evidence_ids {
                if seen.insert(id.clone()) {
                    ids.push(id.clone());
                }
            }
        }
        ids
    }
}

/// Assemble a brief for one instrument; `None` when there are no bars
/// (`build_brief`).
pub fn build_brief(db: &Db, inst: &Instrument, as_of: NaiveDateTime) -> Option<Brief> {
    let prices = prices(db, inst, as_of)?;
    Some(Brief {
        instrument: inst.clone(),
        as_of,
        prices,
        news: news(db, inst, as_of),
        events: events(db, inst, as_of),
        fundamentals: fundamentals(db, inst, as_of),
        calendar: calendar(db, inst, as_of),
    })
}

fn prices(db: &Db, inst: &Instrument, as_of: NaiveDateTime) -> Option<Section> {
    // Raw SQL: the `bar:{id}` evidence ids need the autoincrement row ids the
    // `Bar` model does not carry.
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, close FROM bar WHERE instrument_id = ?1 AND ts <= ?2 \
             ORDER BY ts DESC LIMIT ?3",
        )
        .ok()?;
    let mut bars: Vec<(i64, f64)> = stmt
        .query_map(
            rusqlite::params![
                inst.id,
                as_of.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
                BAR_WINDOW
            ],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, f64>(1)?)),
        )
        .ok()?
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    if bars.is_empty() {
        return None;
    }
    bars.reverse();

    let closes: Vec<f64> = bars.iter().map(|(_, c)| *c).collect();
    let evidence_start = bars.len().saturating_sub(EVIDENCE_BARS);
    let evidence_ids: Vec<String> = bars[evidence_start..]
        .iter()
        .map(|(id, _)| format!("bar:{id}"))
        .collect();

    let latest = *closes.last().unwrap();
    let change_20 = if closes.len() > 21 {
        (latest / closes[closes.len() - 21] - 1.0) * 100.0
    } else {
        0.0
    };
    let sma20 = closes[closes.len().saturating_sub(20)..]
        .iter()
        .sum::<f64>()
        / 20.min(closes.len()) as f64;
    let sma50 = closes[closes.len().saturating_sub(50)..]
        .iter()
        .sum::<f64>()
        / 50.min(closes.len()) as f64;
    let momentum = if closes.len() > 1 {
        (latest / closes[0] - 1.0) * 100.0
    } else {
        0.0
    };
    let tail = &closes[closes.len().saturating_sub(EVIDENCE_BARS)..];
    let high = tail.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let low = tail.iter().cloned().fold(f64::INFINITY, f64::min);

    let mut section = Section::titled("Prices");
    section.lines = vec![
        format!("Latest close: {:.2} {}", latest, inst.currency),
        format!("20-day return: {:+.2}%", change_20),
        format!("Return over window: {:+.2}%", momentum),
        format!("20-day SMA: {:.2}, 50-day SMA: {:.2}", sma20, sma50),
        format!("60-day range: {:.2} - {:.2}", low, high),
    ];
    if let Some(sector) = &inst.sector {
        section.lines.push(format!("Sector: {sector}"));
    }
    section.evidence_ids = evidence_ids;
    Some(section)
}

fn news(db: &Db, inst: &Instrument, as_of: NaiveDateTime) -> Section {
    let since = as_of - Duration::days(NEWS_WINDOW_DAYS);
    // Window filter in SQL; instrument membership (a JSON array column) and
    // the newest-first + limit stay in Rust.
    let mut stmt = match db.conn().prepare(
        "SELECT id, instrument_ids, published, title, source FROM newsitem \
         WHERE published >= ?1 AND published <= ?2 ORDER BY published DESC",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Section::titled("News"),
    };
    let rows: Vec<(String, String, NaiveDateTime, String, String)> = match stmt.query_map(
        rusqlite::params![
            since.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
            as_of.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                delta_core::db::decode_ts(&row.get::<_, String>(2)?).unwrap_or_default(),
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
            ))
        },
    ) {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect(),
        Err(_) => Vec::new(),
    };
    let mut section = Section::titled("News");
    for (id, instrument_ids, published, title, source) in rows {
        let ids: Vec<String> = delta_core::json::from_json(Some(&instrument_ids));
        if !ids.iter().any(|id| id == &inst.id) {
            continue;
        }
        section.lines.push(format!(
            "{} [{}] {}",
            published.format("%Y-%m-%d"),
            source,
            title
        ));
        let kind = if PRIMARY_FILING_SOURCES.contains(&source.as_str()) {
            "filing"
        } else {
            "news"
        };
        section.evidence_ids.push(format!("{kind}:{id}"));
        if section.lines.len() >= NEWS_LIMIT {
            break;
        }
    }
    section
}

fn event_rows(
    db: &Db,
    inst: &Instrument,
    from: NaiveDateTime,
    to: NaiveDateTime,
    newest_first: bool,
) -> Vec<(String, NaiveDateTime, String, String, f64)> {
    let order = if newest_first { "DESC" } else { "ASC" };
    let Ok(mut stmt) = db.conn().prepare(&format!(
        "SELECT id, ts, kind, summary, sentiment FROM event \
         WHERE instrument_id = ?1 AND ts >= ?2 AND ts <= ?3 ORDER BY ts {order}",
    )) else {
        return Vec::new();
    };
    let rows = match stmt.query_map(
        rusqlite::params![
            inst.id,
            from.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
            to.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
        ],
        |row| {
            Ok((
                row.get::<_, String>(0)?,
                delta_core::db::decode_ts(&row.get::<_, String>(1)?).unwrap_or_default(),
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, f64>(4)?,
            ))
        },
    ) {
        Ok(mapped) => mapped.filter_map(|r| r.ok()).collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };
    rows
}

fn events(db: &Db, inst: &Instrument, as_of: NaiveDateTime) -> Section {
    let since = as_of - Duration::days(EVENT_WINDOW_DAYS);
    let mut section = Section::titled("Events");
    for (id, ts, kind, summary, sentiment) in event_rows(db, inst, since, as_of, true) {
        section.lines.push(format!(
            "{} {}: {} (sentiment {:+.1})",
            ts.format("%Y-%m-%d"),
            kind,
            summary,
            sentiment
        ));
        section.evidence_ids.push(format!("event:{id}"));
    }
    section
}

fn fundamentals(db: &Db, inst: &Instrument, as_of: NaiveDateTime) -> Section {
    // Raw SQL for the `fundamental:{id}` evidence ids (model drops the row id).
    let mut rows: Vec<(i64, NaiveDate, String, f64, String)> = Vec::new();
    if let Ok(mut stmt) = db.conn().prepare(
        "SELECT id, as_of, metric, value, source FROM fundamental \
         WHERE instrument_id = ?1 AND as_of <= ?2 ORDER BY as_of DESC",
    ) {
        if let Ok(mapped) = stmt.query_map(
            rusqlite::params![inst.id, as_of.date().format("%Y-%m-%d").to_string()],
            |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    NaiveDate::parse_from_str(&row.get::<_, String>(1)?, "%Y-%m-%d")
                        .unwrap_or_default(),
                    row.get::<_, String>(2)?,
                    row.get::<_, f64>(3)?,
                    row.get::<_, String>(4)?,
                ))
            },
        ) {
            rows = mapped.filter_map(|r| r.ok()).collect();
        }
    }
    let mut section = Section::titled("Fundamentals");
    let mut seen = std::collections::BTreeSet::new();
    for (id, as_of, metric, value, source) in rows {
        if !seen.insert(metric.clone()) {
            continue;
        }
        section.lines.push(format!(
            "{}: {} (as of {}, {})",
            metric,
            fmt_value(value),
            as_of.format("%Y-%m-%d"),
            source
        ));
        section.evidence_ids.push(format!("fundamental:{id}"));
    }
    section
}

fn calendar(db: &Db, inst: &Instrument, as_of: NaiveDateTime) -> Section {
    let far_future = as_of + Duration::days(3650);
    let mut section = Section::titled("Upcoming events");
    for (id, ts, kind, summary, _sentiment) in event_rows(db, inst, as_of, far_future, false) {
        section
            .lines
            .push(format!("{} {}: {}", ts.format("%Y-%m-%d"), kind, summary));
        section.evidence_ids.push(format!("event:{id}"));
    }
    section
}

/// Readable magnitudes for the model: 2.66e11 -> 265.60B, 7.46 -> 7.46
/// (`_fmt_value`). Matches Python's `{:,.2f}` thousands grouping.
pub fn fmt_value(value: f64) -> String {
    let magnitude = value.abs();
    for (threshold, suffix) in [(1e12, "T"), (1e9, "B"), (1e6, "M")] {
        if magnitude >= threshold {
            return format!(
                "{}{}",
                delta_core::format::grouped(value / threshold, 2),
                suffix
            );
        }
    }
    delta_core::format::grouped(value, 2)
}

/// `services.brief_for`: the rendered brief for one universe instrument.
pub fn brief_for(
    db: &Db,
    universe: &[Instrument],
    instrument_id: &str,
    as_of: NaiveDateTime,
) -> Option<String> {
    let inst = universe.iter().find(|i| i.id == instrument_id)?;
    build_brief(db, inst, as_of).map(|b| b.render())
}
