//! SQLite engine, tables and idempotent ingest. Port of `delta/core/db.py`.
//!
//! Opens Python-created `delta.db` files: schema, column types and the
//! datetime encoding (`YYYY-MM-DD HH:MM:SS.ffffff`, naive UTC — SQLModel's
//! storage format) are byte-compatible with the Python app.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::{NaiveDate, NaiveDateTime};
use rusqlite::{params, Connection, OptionalExtension};

use crate::json::{from_json, to_json};
use crate::models::{Bar, Event, Fundamental, LlmCall, NewsItem};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Datetime storage format used by SQLAlchemy's SQLite DATETIME type.
fn encode_ts(ts: NaiveDateTime) -> String {
    ts.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

/// Parse the stored datetime format (used by the services crate's raw SQL).
pub fn decode_ts(raw: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(raw, "%Y-%m-%d %H:%M:%S%.f").ok()
}

fn encode_date(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

fn decode_date(raw: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").ok()
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS bar (
    id INTEGER PRIMARY KEY,
    instrument_id VARCHAR NOT NULL,
    ts DATETIME NOT NULL,
    open FLOAT NOT NULL,
    high FLOAT NOT NULL,
    low FLOAT NOT NULL,
    close FLOAT NOT NULL,
    volume FLOAT NOT NULL,
    source VARCHAR NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_bar_instrument_id ON bar (instrument_id);
CREATE INDEX IF NOT EXISTS ix_bar_ts ON bar (ts);
CREATE UNIQUE INDEX IF NOT EXISTS uq_bar_instrument_ts ON bar (instrument_id, ts);

CREATE TABLE IF NOT EXISTS newsitem (
    id VARCHAR NOT NULL PRIMARY KEY,
    instrument_ids VARCHAR NOT NULL,
    published DATETIME NOT NULL,
    title VARCHAR NOT NULL,
    url VARCHAR NOT NULL,
    body VARCHAR,
    source VARCHAR NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_newsitem_published ON newsitem (published);

CREATE TABLE IF NOT EXISTS news_instrument (
    id INTEGER PRIMARY KEY,
    news_id VARCHAR NOT NULL,
    instrument_id VARCHAR NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_news_instrument_news_id ON news_instrument (news_id);
CREATE INDEX IF NOT EXISTS ix_news_instrument_instrument_id ON news_instrument (instrument_id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_news_instrument ON news_instrument (news_id, instrument_id);

CREATE TABLE IF NOT EXISTS event (
    id VARCHAR NOT NULL PRIMARY KEY,
    instrument_id VARCHAR NOT NULL,
    ts DATETIME NOT NULL,
    kind VARCHAR NOT NULL,
    summary VARCHAR NOT NULL,
    sentiment FLOAT NOT NULL,
    evidence_ids VARCHAR NOT NULL,
    extracted_by VARCHAR NOT NULL,
    prompt_version VARCHAR NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_event_instrument_id ON event (instrument_id);
CREATE INDEX IF NOT EXISTS ix_event_ts ON event (ts);

CREATE TABLE IF NOT EXISTS fundamental (
    id INTEGER PRIMARY KEY,
    instrument_id VARCHAR NOT NULL,
    as_of DATE NOT NULL,
    metric VARCHAR NOT NULL,
    value FLOAT NOT NULL,
    source VARCHAR NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_fundamental_instrument_id ON fundamental (instrument_id);
CREATE UNIQUE INDEX IF NOT EXISTS uq_fundamental_key
    ON fundamental (instrument_id, as_of, metric, source);

CREATE TABLE IF NOT EXISTS llmcall (
    id VARCHAR NOT NULL PRIMARY KEY,
    ts DATETIME NOT NULL,
    task VARCHAR NOT NULL,
    model VARCHAR NOT NULL,
    prompt_version VARCHAR NOT NULL,
    prompt_hash VARCHAR NOT NULL,
    input_tokens INTEGER NOT NULL,
    output_tokens INTEGER NOT NULL,
    cost_usd FLOAT NOT NULL,
    latency_ms INTEGER NOT NULL,
    cached BOOLEAN NOT NULL,
    response VARCHAR
);
CREATE INDEX IF NOT EXISTS ix_llmcall_prompt_hash ON llmcall (prompt_hash);
CREATE INDEX IF NOT EXISTS ix_llmcall_cached ON llmcall (cached);

CREATE TABLE IF NOT EXISTS sentiment (
    id VARCHAR NOT NULL PRIMARY KEY,
    instrument_id VARCHAR NOT NULL,
    evidence_id VARCHAR NOT NULL,
    ts DATETIME NOT NULL,
    stance VARCHAR NOT NULL,
    confidence FLOAT NOT NULL,
    probabilities VARCHAR NOT NULL,
    model VARCHAR NOT NULL,
    prompt_version VARCHAR NOT NULL
);
CREATE INDEX IF NOT EXISTS ix_sentiment_instrument_id ON sentiment (instrument_id);
CREATE INDEX IF NOT EXISTS ix_sentiment_evidence_id ON sentiment (evidence_id);
CREATE INDEX IF NOT EXISTS ix_sentiment_ts ON sentiment (ts);
";

/// A database handle. Cheap to clone is *not* — pass `&mut` for writes.
pub struct Db {
    conn: Connection,
}

/// One ingest payload row (`store_items` input).
#[derive(Debug, Clone)]
pub enum StoreItem {
    Bar(Bar),
    News(NewsItem),
    Event(Event),
    Fundamental(Fundamental),
}

impl From<Bar> for StoreItem {
    fn from(v: Bar) -> Self {
        StoreItem::Bar(v)
    }
}
impl From<NewsItem> for StoreItem {
    fn from(v: NewsItem) -> Self {
        StoreItem::News(v)
    }
}
impl From<Event> for StoreItem {
    fn from(v: Event) -> Self {
        StoreItem::Event(v)
    }
}
impl From<Fundamental> for StoreItem {
    fn from(v: Fundamental) -> Self {
        StoreItem::Fundamental(v)
    }
}

impl Db {
    /// Open (creating directories and tables when needed) with WAL pragmas,
    /// post-release unique-index migration and the news_instrument backfill.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, DbError> {
        if let Some(parent) = path.as_ref().parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        // WAL lets readers proceed during writes; synchronous=NORMAL keeps WAL
        // durable against process crash while avoiding a per-commit fsync;
        // busy_timeout makes concurrent access wait instead of erroring.
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "busy_timeout", 5000)?;
        conn.execute_batch(SCHEMA)?;
        let db = Self { conn };
        db.migrate()?;
        db.backfill_news_instrument()?;
        Ok(db)
    }

    /// In-memory DB, for tests.
    pub fn open_memory() -> Result<Self, DbError> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(SCHEMA)?;
        let db = Self { conn };
        db.migrate()?;
        db.backfill_news_instrument()?;
        Ok(db)
    }

    /// Unique indexes added after Phase 1: pre-existing Phase 1 databases
    /// created before the model declared the constraint get it here, with
    /// duplicate rows dropped (lowest id kept), matching `_migrate`.
    fn migrate(&self) -> Result<(), DbError> {
        for (name, table, cols) in [
            ("uq_bar_instrument_ts", "bar", vec!["instrument_id", "ts"]),
            (
                "uq_fundamental_key",
                "fundamental",
                vec!["instrument_id", "as_of", "metric", "source"],
            ),
        ] {
            if Self::has_unique_index(&self.conn, table, &cols)? {
                continue; // fresh DB: the schema above already created one
            }
            let key = cols.join(", ");
            self.conn.execute(
                &format!("DELETE FROM {table} WHERE id NOT IN (SELECT MIN(id) FROM {table} GROUP BY {key})"),
                [],
            )?;
            self.conn.execute(
                &format!("CREATE UNIQUE INDEX IF NOT EXISTS {name} ON {table} ({key})"),
                [],
            )?;
        }
        Ok(())
    }

    fn has_unique_index(conn: &Connection, table: &str, cols: &[&str]) -> Result<bool, DbError> {
        let names: Vec<String> = conn
            .prepare(&format!("PRAGMA index_list({table})"))?
            .query_map([], |row| {
                Ok((row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
            })?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .filter(|(_, unique)| *unique == 1)
            .map(|(name, _)| name)
            .collect();
        for name in names {
            let indexed: Vec<String> = conn
                .prepare(&format!("PRAGMA index_info({name})"))?
                .query_map([], |row| row.get::<_, String>(2))?
                .collect::<Result<Vec<_>, _>>()?;
            if indexed.iter().map(String::as_str).collect::<Vec<_>>() == cols {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Insert one `news_instrument` row per instrument_id for every newsitem,
    /// idempotent via the unique constraint.
    fn backfill_news_instrument(&self) -> Result<(), DbError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, instrument_ids FROM newsitem")?;
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<Result<Vec<_>, _>>()?;
        for (news_id, raw) in rows {
            for instrument_id in from_json(Some(&raw)) {
                self.conn.execute(
                    "INSERT OR IGNORE INTO news_instrument (news_id, instrument_id) VALUES (?, ?)",
                    params![news_id, instrument_id],
                )?;
            }
        }
        Ok(())
    }

    /// Persist data-plugin output idempotently. Returns rows actually inserted
    /// per table (matching `store_items`).
    pub fn store_items(&mut self, items: &[StoreItem]) -> Result<BTreeMap<String, usize>, DbError> {
        let mut counts = BTreeMap::new();
        let tx = self.conn.transaction()?;

        let mut bar =
            tx.prepare("INSERT OR IGNORE INTO bar (instrument_id, ts, open, high, low, close, volume, source) VALUES (?, ?, ?, ?, ?, ?, ?, ?)")?;
        let mut n = 0;
        for item in items {
            if let StoreItem::Bar(b) = item {
                n += bar.execute(params![
                    b.instrument_id,
                    encode_ts(b.ts),
                    b.open,
                    b.high,
                    b.low,
                    b.close,
                    b.volume,
                    b.source
                ])? as usize;
            }
        }
        counts.insert("bar".to_string(), n);

        let mut news = tx.prepare(
            "INSERT OR IGNORE INTO newsitem (id, instrument_ids, published, title, url, body, source) VALUES (?, ?, ?, ?, ?, ?, ?)",
        )?;
        let mut n = 0;
        for item in items {
            if let StoreItem::News(i) = item {
                n += news.execute(params![
                    i.id,
                    to_json(&i.instrument_ids),
                    encode_ts(i.published),
                    i.title,
                    i.url,
                    i.body,
                    i.source
                ])? as usize;
            }
        }
        counts.insert("newsitem".to_string(), n);
        if n > 0 {
            let mut link = tx.prepare(
                "INSERT OR IGNORE INTO news_instrument (news_id, instrument_id) VALUES (?, ?)",
            )?;
            for item in items {
                if let StoreItem::News(i) = item {
                    for iid in &i.instrument_ids {
                        link.execute(params![i.id, iid])?;
                    }
                }
            }
        }

        let mut event = tx.prepare(
            "INSERT OR IGNORE INTO event (id, instrument_id, ts, kind, summary, sentiment, evidence_ids, extracted_by, prompt_version) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )?;
        let mut n = 0;
        for item in items {
            if let StoreItem::Event(e) = item {
                n += event.execute(params![
                    e.id,
                    e.instrument_id,
                    encode_ts(e.ts),
                    e.kind.as_str(),
                    e.summary,
                    e.sentiment,
                    to_json(&e.evidence_ids),
                    e.extracted_by,
                    e.prompt_version
                ])? as usize;
            }
        }
        counts.insert("event".to_string(), n);

        let mut fund = tx.prepare(
            "INSERT OR IGNORE INTO fundamental (instrument_id, as_of, metric, value, source) VALUES (?, ?, ?, ?, ?)",
        )?;
        let mut n = 0;
        for item in items {
            if let StoreItem::Fundamental(f) = item {
                n += fund.execute(params![
                    f.instrument_id,
                    encode_date(f.as_of),
                    f.metric,
                    f.value,
                    f.source
                ])? as usize;
            }
        }
        counts.insert("fundamental".to_string(), n);

        drop(bar);
        drop(news);
        drop(event);
        drop(fund);
        tx.commit()?;
        Ok(counts)
    }

    // ---- readers used for parity tests and services ----

    pub fn bars(&self, instrument_id: &str) -> Result<Vec<Bar>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT instrument_id, ts, open, high, low, close, volume, source \
             FROM bar WHERE instrument_id = ? ORDER BY ts",
        )?;
        let rows = stmt
            .query_map(params![instrument_id], |row| {
                Ok(Bar {
                    instrument_id: row.get(0)?,
                    ts: decode_ts(&row.get::<_, String>(1)?).unwrap_or_default(),
                    open: row.get(2)?,
                    high: row.get(3)?,
                    low: row.get(4)?,
                    close: row.get(5)?,
                    volume: row.get(6)?,
                    source: row.get(7)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn news(&self) -> Result<Vec<NewsItem>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, instrument_ids, published, title, url, body, source FROM newsitem ORDER BY published",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(NewsItem {
                    id: row.get(0)?,
                    instrument_ids: from_json(row.get::<_, Option<String>>(1)?.as_deref()),
                    published: decode_ts(&row.get::<_, String>(2)?).unwrap_or_default(),
                    title: row.get(3)?,
                    url: row.get(4)?,
                    body: row.get(5)?,
                    source: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn news_for_instrument(&self, instrument_id: &str) -> Result<Vec<NewsItem>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT n.id, n.instrument_ids, n.published, n.title, n.url, n.body, n.source \
             FROM newsitem n JOIN news_instrument ni ON ni.news_id = n.id \
             WHERE ni.instrument_id = ? ORDER BY n.published",
        )?;
        let rows = stmt
            .query_map(params![instrument_id], |row| {
                Ok(NewsItem {
                    id: row.get(0)?,
                    instrument_ids: from_json(row.get::<_, Option<String>>(1)?.as_deref()),
                    published: decode_ts(&row.get::<_, String>(2)?).unwrap_or_default(),
                    title: row.get(3)?,
                    url: row.get(4)?,
                    body: row.get(5)?,
                    source: row.get(6)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn events(&self, instrument_id: &str) -> Result<Vec<Event>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, instrument_id, ts, kind, summary, sentiment, evidence_ids, extracted_by, prompt_version \
             FROM event WHERE instrument_id = ? ORDER BY ts",
        )?;
        let rows = stmt
            .query_map(params![instrument_id], |row| {
                Ok(Event {
                    id: row.get(0)?,
                    instrument_id: row.get(1)?,
                    ts: decode_ts(&row.get::<_, String>(2)?).unwrap_or_default(),
                    kind: crate::models::EventKind::parse(&row.get::<_, String>(3)?)
                        .unwrap_or(crate::models::EventKind::Other),
                    summary: row.get(4)?,
                    sentiment: row.get(5)?,
                    evidence_ids: from_json(row.get::<_, Option<String>>(6)?.as_deref()),
                    extracted_by: row.get(7)?,
                    prompt_version: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn fundamentals(&self, instrument_id: &str) -> Result<Vec<Fundamental>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT instrument_id, as_of, metric, value, source \
             FROM fundamental WHERE instrument_id = ? ORDER BY as_of",
        )?;
        let rows = stmt
            .query_map(params![instrument_id], |row| {
                Ok(Fundamental {
                    instrument_id: row.get(0)?,
                    as_of: decode_date(&row.get::<_, String>(1)?).unwrap_or_default(),
                    metric: row.get(2)?,
                    value: row.get(3)?,
                    source: row.get(4)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The first cached row for `prompt_hash`, or None when absent or empty.
    /// A row with no `response` is not a usable cache entry (it was a failed
    /// or partial persist), so it is treated as a miss.
    pub fn llm_lookup(&self, prompt_hash: &str) -> Result<Option<LlmCall>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, task, model, prompt_version, prompt_hash, input_tokens, output_tokens, cost_usd, latency_ms, cached, response \
             FROM llmcall WHERE prompt_hash = ? ORDER BY rowid LIMIT 1",
        )?;
        let row = stmt
            .query_row(params![prompt_hash], |row| {
                Ok(LlmCall {
                    id: row.get(0)?,
                    ts: decode_ts(&row.get::<_, String>(1)?).unwrap_or_default(),
                    task: row.get(2)?,
                    model: row.get(3)?,
                    prompt_version: row.get(4)?,
                    prompt_hash: row.get(5)?,
                    input_tokens: row.get(6)?,
                    output_tokens: row.get(7)?,
                    cost_usd: row.get(8)?,
                    latency_ms: row.get(9)?,
                    cached: row.get::<_, i64>(10)? != 0,
                    response: row.get(11)?,
                })
            })
            .optional()?;
        Ok(row.filter(|call| call.response.is_some()))
    }

    /// Persist one `llmcall` row: a live call or a replayed cache hit.
    pub fn llm_store_call(&mut self, call: &LlmCall) -> Result<(), DbError> {
        self.conn.execute(
            "INSERT INTO llmcall (id, ts, task, model, prompt_version, prompt_hash, input_tokens, output_tokens, cost_usd, latency_ms, cached, response) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                call.id,
                encode_ts(call.ts),
                call.task,
                call.model,
                call.prompt_version,
                call.prompt_hash,
                call.input_tokens,
                call.output_tokens,
                call.cost_usd,
                call.latency_ms,
                call.cached as i64,
                call.response,
            ],
        )?;
        Ok(())
    }

    /// All llmcall rows, oldest first (tests and cost reporting).
    pub fn llm_lookup_all(&self) -> Result<Vec<LlmCall>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, task, model, prompt_version, prompt_hash, input_tokens, output_tokens, cost_usd, latency_ms, cached, response \
             FROM llmcall ORDER BY rowid",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(LlmCall {
                    id: row.get(0)?,
                    ts: decode_ts(&row.get::<_, String>(1)?).unwrap_or_default(),
                    task: row.get(2)?,
                    model: row.get(3)?,
                    prompt_version: row.get(4)?,
                    prompt_hash: row.get(5)?,
                    input_tokens: row.get(6)?,
                    output_tokens: row.get(7)?,
                    cost_usd: row.get(8)?,
                    latency_ms: row.get(9)?,
                    cached: row.get::<_, i64>(10)? != 0,
                    response: row.get(11)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Read access to the connection for crate-local analytics SQL.
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    /// Every stored event (read-side analytics).
    pub fn events_all(&self) -> Result<Vec<Event>, DbError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, instrument_id, ts, kind, summary, sentiment, evidence_ids, extracted_by, prompt_version              FROM event",
        )?;
        let rows = stmt
            .query_map([], |row| {
                Ok(Event {
                    id: row.get(0)?,
                    instrument_id: row.get(1)?,
                    ts: decode_ts(&row.get::<_, String>(2)?).unwrap_or_default(),
                    kind: crate::models::EventKind::parse(&row.get::<_, String>(3)?)
                        .unwrap_or(crate::models::EventKind::Other),
                    summary: row.get(4)?,
                    sentiment: row.get(5)?,
                    evidence_ids: from_json(row.get::<_, Option<String>>(6)?.as_deref()),
                    extracted_by: row.get(7)?,
                    prompt_version: row.get(8)?,
                })
            })?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Every event id (extract's dedupe set).
    pub fn event_ids(&self) -> Result<std::collections::BTreeSet<String>, DbError> {
        let mut stmt = self.conn.prepare("SELECT id FROM event")?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(rows.into_iter().collect())
    }

    /// The newest stored bar timestamp (`MAX(ts)`), or None when empty.
    pub fn bars_all_max_ts(&self) -> Result<Option<NaiveDateTime>, DbError> {
        // An empty bar table yields one row with NULL; decode through a plain
        // nullable Value so that is a None, not a type error.
        let raw: rusqlite::types::Value = self
            .conn
            .query_row("SELECT MAX(ts) FROM bar", [], |row| row.get(0))
            .optional()?
            .unwrap_or(rusqlite::types::Value::Null);
        Ok(match raw {
            rusqlite::types::Value::Text(text) => decode_ts(&text),
            _ => None,
        })
    }

    pub fn table_count(&self, table: &str) -> Result<usize, DbError> {
        // table names come from our own code, never user input.
        let n: i64 = self
            .conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))?;
        Ok(n as usize)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::EventKind;

    fn bar(ts: &str, close: f64) -> Bar {
        Bar {
            instrument_id: "US:AAPL".to_string(),
            ts: decode_ts(ts).unwrap(),
            open: close - 1.0,
            high: close + 1.0,
            low: close - 2.0,
            close,
            volume: 1000.0,
            source: "yahoo".to_string(),
        }
    }

    #[test]
    fn store_is_idempotent() {
        let mut db = Db::open_memory().unwrap();
        let items: Vec<StoreItem> = vec![
            bar("2026-01-02 00:00:00", 10.0).into(),
            NewsItem {
                id: "n1".to_string(),
                instrument_ids: vec!["US:AAPL".to_string()],
                published: decode_ts("2026-01-02 12:00:00").unwrap(),
                title: "t".to_string(),
                url: "u".to_string(),
                body: None,
                source: "rss".to_string(),
            }
            .into(),
            Event {
                id: "e1".to_string(),
                instrument_id: "US:AAPL".to_string(),
                ts: decode_ts("2026-01-02 12:00:00").unwrap(),
                kind: EventKind::Earnings,
                summary: "s".to_string(),
                sentiment: 0.5,
                evidence_ids: vec!["ev1".to_string()],
                extracted_by: "m".to_string(),
                prompt_version: "v1".to_string(),
            }
            .into(),
            Fundamental {
                instrument_id: "US:AAPL".to_string(),
                as_of: NaiveDate::from_ymd_opt(2026, 1, 2).unwrap(),
                metric: "pe".to_string(),
                value: 25.0,
                source: "yahoo".to_string(),
            }
            .into(),
        ];
        let counts = db.store_items(&items).unwrap();
        assert_eq!(counts["bar"], 1);
        assert_eq!(counts["newsitem"], 1);
        assert_eq!(counts["event"], 1);
        assert_eq!(counts["fundamental"], 1);

        // Re-store the same items: nothing inserted.
        let counts = db.store_items(&items).unwrap();
        assert_eq!(counts["bar"], 0);
        assert_eq!(counts["newsitem"], 0);
        assert_eq!(counts["event"], 0);
        assert_eq!(counts["fundamental"], 0);

        assert_eq!(db.bars("US:AAPL").unwrap().len(), 1);
        assert_eq!(db.news_for_instrument("US:AAPL").unwrap().len(), 1);
        assert_eq!(db.events("US:AAPL").unwrap().len(), 1);
        assert_eq!(db.fundamentals("US:AAPL").unwrap().len(), 1);
    }

    #[test]
    fn news_instrument_backfilled_from_json() {
        let mut db = Db::open_memory().unwrap();
        db.store_items(&[StoreItem::News(NewsItem {
            id: "n1".to_string(),
            instrument_ids: vec!["US:AAPL".to_string(), "US:MSFT".to_string()],
            published: decode_ts("2026-01-02 12:00:00").unwrap(),
            title: "t".to_string(),
            url: "u".to_string(),
            body: None,
            source: "rss".to_string(),
        })])
        .unwrap();
        // Simulate a pre-mapping database by wiping the join table; reopening
        // must backfill it.
        db.conn.execute("DELETE FROM news_instrument", []).unwrap();
        drop(db);
        // reopen: use a file so open() runs again
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.db");
        let mut db2 = Db::open(&path).unwrap();
        db2.store_items(&[StoreItem::News(NewsItem {
            id: "n1".to_string(),
            instrument_ids: vec!["US:AAPL".to_string()],
            published: decode_ts("2026-01-02 12:00:00").unwrap(),
            title: "t".to_string(),
            url: "u".to_string(),
            body: None,
            source: "rss".to_string(),
        })])
        .unwrap();
        db2.conn.execute("DELETE FROM news_instrument", []).unwrap();
        drop(db2);
        let db3 = Db::open(&path).unwrap();
        assert_eq!(db3.table_count("news_instrument").unwrap(), 1);
    }
}
