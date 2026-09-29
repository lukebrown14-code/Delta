//! R2 parity tests: the analytics read-side must match the Python
//! implementation's output on identically seeded data. The goldens below were
//! produced by running `delta.services` over this exact seed.

use std::collections::BTreeMap;

use chrono::{NaiveDate, NaiveDateTime};
use delta_core::db::{Db, StoreItem};
use delta_core::models::{Event, EventKind, NewsItem};
use delta_services::analytics::{
    data_health, latest_headline, llm_costs, pulse, recent_closes, total_spend, upcoming_events,
};
use delta_services::{CostRow, FILING_SOURCE};

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

/// Seed identical to the Python golden run (see the findings file for the
/// script): the fixture DB plus four news rows, one upcoming event, and three
/// llmcall rows.
fn seeded_db() -> Db {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("svc.db");
    std::fs::copy(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/delta.db"),
        &path,
    )
    .unwrap();
    let mut db = Db::open(&path).unwrap();
    let news = |id: &str, ids: &[&str], published: &str, source: &str| {
        StoreItem::News(NewsItem {
            id: id.to_string(),
            instrument_ids: ids.iter().map(|s| s.to_string()).collect(),
            published: ts(published),
            title: format!("title {id}"),
            url: format!("https://x/{id}"),
            body: None,
            source: source.to_string(),
        })
    };
    db.store_items(&[
        news("a1", &["US:AAPL"], "2026-09-10 10:00:00", "rss"),
        news("a2", &["US:AAPL"], "2026-09-12 10:00:00", "rss"),
        news("f1", &["ASX:BHP"], "2026-09-14 09:00:00", FILING_SOURCE),
        news("old", &[], "2026-08-01 00:00:00", "rss"),
        StoreItem::Event(Event {
            id: "u1".to_string(),
            instrument_id: "US:AAPL".to_string(),
            ts: ts("2026-10-05 00:00:00"),
            kind: EventKind::Earnings,
            summary: "Q4 expected".to_string(),
            sentiment: 0.0,
            evidence_ids: vec![],
            extracted_by: "m".to_string(),
            prompt_version: "v1".to_string(),
        }),
    ])
    .unwrap();
    for (id, when, task, model, cost, cached) in [
        ("c1", "2026-09-10 00:00:00", "extract", "m1", 0.01, false),
        ("c2", "2026-09-11 00:00:00", "extract", "m1", 0.02, true),
        ("c3", "2026-08-01 00:00:00", "chat", "m2", 0.5, false),
    ] {
        db.llm_store_call(&delta_core::models::LlmCall {
            id: id.to_string(),
            ts: ts(when),
            task: task.to_string(),
            model: model.to_string(),
            prompt_version: "v1".to_string(),
            prompt_hash: format!("h{id}"),
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: cost,
            latency_ms: 100,
            cached,
            response: None,
        })
        .unwrap();
    }
    // Leak the tempdir so the DB outlives the helper (tests are short-lived).
    std::mem::forget(dir);
    db
}

const NOW: &str = "2026-09-29 12:00:00";

#[test]
fn data_health_matches_python() {
    let db = seeded_db();
    let health = data_health(&db).unwrap();
    let mut counts: BTreeMap<String, usize> = health.counts.clone();
    let expected: BTreeMap<String, usize> = BTreeMap::from([
        ("bar".into(), 2),
        ("event".into(), 2),
        ("fundamental".into(), 1),
        ("llmcall".into(), 3),
        ("newsitem".into(), 5),
    ]);
    counts.retain(|k, _| expected.contains_key(k));
    assert_eq!(counts, expected);
    assert_eq!(
        health.latest_bar.get("US:AAPL").copied(),
        ts("2026-01-03 21:00:00").into()
    );
    assert_eq!(health.last_llm, Some(ts("2026-09-11 00:00:00")));
}

#[test]
fn recent_closes_match_python() {
    let db = seeded_db();
    assert_eq!(
        recent_closes(&db, "US:AAPL", 40).unwrap(),
        vec![10.25, 11.75]
    );
    assert_eq!(
        recent_closes(&db, "US:MSFT", 40).unwrap(),
        Vec::<f64>::new()
    );
}

#[test]
fn llm_costs_match_python() {
    let db = seeded_db();
    let since = NaiveDate::from_ymd_opt(2026, 9, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    assert_eq!(
        llm_costs(&db, Some(since)).unwrap(),
        vec![CostRow {
            task: "extract".into(),
            model: "m1".into(),
            calls: 2,
            cost_usd: 0.03
        }]
    );
    assert!((total_spend(&db, None) - 0.53).abs() < 1e-9);
}

#[test]
fn pulse_matches_python() {
    let db = seeded_db();
    let now = ts(NOW);
    let since = NaiveDate::from_ymd_opt(2026, 9, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let p = pulse(
        &db,
        &["US:AAPL".to_string(), "ASX:BHP".to_string()],
        since,
        30,
        now,
    )
    .unwrap();
    assert_eq!(p.articles, 2);
    assert_eq!(p.filings, 1);
    assert_eq!(
        p.events, 0,
        "the one stored event is future-dated, so excluded"
    );
    assert_eq!(p.total(), 3);
    assert_eq!(p.busiest, Some(("US:AAPL".to_string(), 2)));
    assert_eq!(p.quietest, Some(("ASX:BHP".to_string(), 1)));

    // A 3-day window sees nothing: no busiest to name.
    let short = pulse(
        &db,
        &["US:AAPL".to_string(), "ASX:BHP".to_string()],
        since,
        3,
        now,
    )
    .unwrap();
    assert_eq!(short.daily, vec![0, 0, 0]);
    assert_eq!(short.busiest, None);
    assert_eq!(short.quietest, None);
}

#[test]
fn upcoming_events_match_python() {
    let db = seeded_db();
    let rows = upcoming_events(&db, &["US:AAPL".to_string()], 3, ts(NOW)).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].instrument_id, "US:AAPL");
    assert_eq!(rows[0].ts, ts("2026-10-05 00:00:00"));
    assert_eq!(rows[0].kind, "earnings");
    assert_eq!(rows[0].summary, "Q4 expected");
    // Empty id list asks for nothing (an empty IN () is a SQL error in Python).
    assert!(upcoming_events(&db, &[], 3, ts(NOW)).unwrap().is_empty());
}

#[test]
fn latest_headline_matches_python() {
    let db = seeded_db();
    let headline = latest_headline(&db, &["US:AAPL".to_string()], 50)
        .unwrap()
        .unwrap();
    assert_eq!(headline.title, "title a2");
    assert_eq!(headline.ts, ts("2026-09-12 10:00:00"));
    assert_eq!(headline.instrument_ids, vec!["US:AAPL".to_string()]);
    // Any item when the id list is empty.
    assert!(latest_headline(&db, &[], 50).unwrap().is_some());
}
