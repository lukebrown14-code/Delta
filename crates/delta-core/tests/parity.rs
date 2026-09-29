//! R1a done-criterion: open a Python-created `delta.db`
//! (`fixtures/delta.db`, seeded by `delta.core.db`) and round-trip every
//! table — read, idempotent write, and the news_instrument join.

use chrono::{NaiveDate, NaiveDateTime};
use delta_core::db::{Db, StoreItem};
use delta_core::models::{EventKind, NewsItem};

fn fixture_path() -> std::path::PathBuf {
    // crates/delta-core/tests -> repo fixtures/
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/delta.db")
}

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

#[test]
fn opens_python_db_and_reads_every_table() {
    let db = Db::open(fixture_path()).unwrap();

    let bars = db.bars("US:AAPL").unwrap();
    assert_eq!(bars.len(), 2);
    assert_eq!(bars[0].instrument_id, "US:AAPL");
    assert_eq!(bars[0].ts, ts("2026-01-02 21:00:00"));
    assert!((bars[0].close - 10.25).abs() < 1e-9);
    assert_eq!(bars[0].source, "yahoo");

    let news = db.news().unwrap();
    assert_eq!(news.len(), 1);
    assert_eq!(news[0].instrument_ids, vec!["US:AAPL".to_string()]);
    assert_eq!(news[0].published, ts("2026-01-03 12:30:00"));
    // Join-table mapping backfilled from the JSON column.
    assert_eq!(db.news_for_instrument("US:AAPL").unwrap().len(), 1);

    let events = db.events("US:AAPL").unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].kind, EventKind::Earnings);
    assert!((events[0].sentiment - 0.62).abs() < 1e-9);
    assert_eq!(events[0].evidence_ids, vec!["n1".to_string()]);

    let funds = db.fundamentals("US:AAPL").unwrap();
    assert_eq!(funds.len(), 1);
    assert_eq!(funds[0].as_of, NaiveDate::from_ymd_opt(2026, 1, 3).unwrap());
    assert!((funds[0].value - 28.4).abs() < 1e-9);
}

#[test]
fn round_trip_write_into_python_db_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("copy.db");
    std::fs::copy(fixture_path(), &path).unwrap();
    let mut db = Db::open(&path).unwrap();

    let duplicate_news = StoreItem::News(NewsItem {
        id: "n1".to_string(), // same id as the Python-seeded row
        instrument_ids: vec!["US:AAPL".to_string()],
        published: ts("2026-01-03 12:30:00"),
        title: "Apple ships thing".to_string(),
        url: "https://example.com/1".to_string(),
        body: None,
        source: "rss".to_string(),
    });
    let counts = db.store_items(&[duplicate_news]).unwrap();
    assert_eq!(
        counts["newsitem"], 0,
        "existing Python row must not re-insert"
    );
    assert_eq!(db.table_count("newsitem").unwrap(), 1);
    // Writing back what we read is a no-op for every table.
    let items: Vec<StoreItem> = db
        .bars("US:AAPL")
        .unwrap()
        .into_iter()
        .map(StoreItem::Bar)
        .chain(db.news().unwrap().into_iter().map(StoreItem::News))
        .chain(
            db.events("US:AAPL")
                .unwrap()
                .into_iter()
                .map(StoreItem::Event),
        )
        .chain(
            db.fundamentals("US:AAPL")
                .unwrap()
                .into_iter()
                .map(StoreItem::Fundamental),
        )
        .collect();
    let counts = db.store_items(&items).unwrap();
    assert_eq!(counts["bar"], 0);
    assert_eq!(counts["event"], 0);
    assert_eq!(counts["fundamental"], 0);
}
