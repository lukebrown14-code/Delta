//! Port of `tests/test_evidence.py` (the unified evidence read model).

use chrono::NaiveDateTime;
use delta_core::db::{Db, StoreItem};
use delta_core::models::{Event, EventKind, Fundamental, NewsItem};
use delta_services::evidence::{
    cite, evidence, evidence_by_ids, falsifier_hit, source_quality, EvidenceItem, FILING_SOURCE,
    PRIMARY_FILING_SOURCES,
};

const INST: &str = "US:AAPL";
const OTHER: &str = "US:MSFT";
const NEWS_URL: &str = "https://example.com/news-1";
const FILING_URL: &str = "https://example.com/filing-1";

/// 5 daily bars per instrument from 2026-03-18 (autoincrement ids 1-5 AAPL,
/// 6-10 MSFT), one filing, one two-instrument news item, one event, one
/// fundamental. "bar:10" sorts before "bar:5" because ids order as strings.
const EXPECTED_ORDER: [&str; 14] = [
    "bar:10",
    "bar:5",
    "filing:filing-1",
    "bar:4",
    "bar:9",
    "news:news-1",
    "bar:3",
    "bar:8",
    "event:event-1",
    "bar:2",
    "bar:7",
    "bar:1",
    "bar:6",
    "fundamental:1",
];

fn ts(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn db() -> Db {
    Db::open_memory().unwrap()
}

/// `conftest.seed_bars`: price rises 0.5/day from 100.0, source "test".
fn seed() -> Db {
    let mut db = db();
    let start = ts("2026-03-18 00:00:00").date();
    let mut items: Vec<StoreItem> = Vec::new();
    for instrument_id in [INST, OTHER] {
        for i in 0..5 {
            let price = 100.0 + i as f64 * 0.5;
            items.push(
                delta_core::models::Bar {
                    instrument_id: instrument_id.to_string(),
                    ts: (start + chrono::Duration::days(i))
                        .and_hms_opt(0, 0, 0)
                        .unwrap(),
                    open: price,
                    high: price + 1.0,
                    low: price - 1.0,
                    close: price,
                    volume: 1000.0,
                    source: "test".to_string(),
                }
                .into(),
            );
        }
    }
    db.store_items(&items).unwrap();
    db.store_items(&[
        NewsItem {
            id: "news-1".to_string(),
            instrument_ids: vec![INST.to_string(), OTHER.to_string()],
            published: ts("2026-03-20 12:00:00"),
            title: "Apple and Microsoft sign cloud deal".to_string(),
            url: NEWS_URL.to_string(),
            body: Some("Both companies announced a partnership.".to_string()),
            source: "rss".to_string(),
        }
        .into(),
        NewsItem {
            id: "filing-1".to_string(),
            instrument_ids: vec![INST.to_string()],
            published: ts("2026-03-21 12:00:00"),
            title: "Apple 10-K filed".to_string(),
            url: FILING_URL.to_string(),
            body: None,
            source: "sec_edgar".to_string(),
        }
        .into(),
        Event {
            id: "event-1".to_string(),
            instrument_id: INST.to_string(),
            ts: ts("2026-03-19 12:00:00"),
            kind: EventKind::Earnings,
            summary: "Reported EPS above consensus".to_string(),
            sentiment: 0.4,
            evidence_ids: vec!["news-1".to_string()],
            extracted_by: "test/model".to_string(),
            prompt_version: "extract_v1".to_string(),
        }
        .into(),
        Fundamental {
            instrument_id: INST.to_string(),
            as_of: chrono::NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
            metric: "eps".to_string(),
            value: 1.0,
            source: "edgar".to_string(),
        }
        .into(),
    ])
    .unwrap();
    db
}

fn ids(items: &[EvidenceItem]) -> Vec<String> {
    items.iter().map(|item| item.id.clone()).collect()
}

fn by_id<'a>(items: &'a [EvidenceItem], id: &str) -> &'a EvidenceItem {
    items.iter().find(|item| item.id == id).unwrap()
}

#[test]
fn mixed_sources_unify_with_kinds() {
    let db = seed();
    let items = evidence(&db, None, None, None, 200, None).unwrap();
    let expected: std::collections::BTreeSet<&str> = EXPECTED_ORDER.into_iter().collect();
    let got: std::collections::BTreeSet<&str> = items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(got, expected);
    let kinds: std::collections::BTreeSet<&str> = items.iter().map(|i| i.kind.as_str()).collect();
    assert_eq!(
        kinds,
        ["bar", "news", "filing", "event", "fundamental"]
            .into_iter()
            .collect()
    );

    let bar = by_id(&items, "bar:5");
    assert_eq!(bar.target_ids, vec![INST]);
    assert_eq!(bar.ts, ts("2026-03-22 00:00:00"));
    assert_eq!(bar.title, "US:AAPL close 102.00");
    assert_eq!(bar.source, "test");
    assert_eq!(bar.url, None);
    assert_eq!(bar.sentiment, None);
    assert_eq!(bar.body, None);
    assert_eq!(bar.raw["close"], 102.0);

    let news = by_id(&items, "news:news-1");
    assert_eq!(news.kind, "news");
    assert_eq!(news.target_ids, vec![INST, OTHER]);
    assert_eq!(news.ts, ts("2026-03-20 12:00:00"));
    assert_eq!(
        news.body.as_deref(),
        Some("Both companies announced a partnership.")
    );
    assert_eq!(news.url.as_deref(), Some(NEWS_URL));

    let event = by_id(&items, "event:event-1");
    assert_eq!(event.kind, "event");
    assert_eq!(event.sentiment, Some(0.4));
    assert_eq!(event.source, "test/model");
    assert_eq!(event.title, "earnings: Reported EPS above consensus");

    let fundamental = by_id(&items, "fundamental:1");
    assert_eq!(fundamental.kind, "fundamental");
    assert_eq!(fundamental.title, "eps: 1.00 (2026-01-01, edgar)");
    assert_eq!(fundamental.ts, ts("2026-01-01 00:00:00"));
    assert_eq!(fundamental.target_ids, vec![INST]);
}

#[test]
fn deterministic_order_ts_desc_then_id_asc() {
    let db = seed();
    let expected: Vec<String> = EXPECTED_ORDER.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        ids(&evidence(&db, None, None, None, 200, None).unwrap()),
        expected
    );
    let again = evidence(&db, None, None, None, 200, None).unwrap();
    assert_eq!(ids(&again), expected);
}

#[test]
fn target_filter_matches_any_target_id() {
    let db = seed();
    let aapl = evidence(&db, Some(INST), None, None, 200, None).unwrap();
    let msft = evidence(&db, Some(OTHER), None, None, 200, None).unwrap();
    assert_eq!(
        ids(&msft),
        ["bar:10", "bar:9", "news:news-1", "bar:8", "bar:7", "bar:6"]
    );
    let excluded = ["bar:6", "bar:7", "bar:8", "bar:9", "bar:10"];
    let aapl_ids: std::collections::BTreeSet<&str> = aapl.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(
        aapl_ids,
        EXPECTED_ORDER
            .iter()
            .copied()
            .filter(|id| !excluded.contains(id))
            .collect()
    );
    assert!(aapl.iter().any(|i| i.id == "news:news-1"));
    assert!(aapl.iter().all(|i| i.target_ids.iter().any(|t| t == INST)));
    assert!(msft.iter().all(|i| i.target_ids.iter().any(|t| t == OTHER)));
    assert!(evidence(&db, Some("US:NVDA"), None, None, 200, None)
        .unwrap()
        .is_empty());
}

#[test]
fn since_filter_is_inclusive() {
    let db = seed();
    let since = |day: &str| -> std::collections::BTreeSet<String> {
        evidence(&db, None, Some(day), None, 200, None)
            .unwrap()
            .iter()
            .map(|i| i.id.clone())
            .collect()
    };
    assert_eq!(
        since("2026-03-20"),
        [
            "bar:3",
            "bar:4",
            "bar:5",
            "bar:8",
            "bar:9",
            "bar:10",
            "news:news-1",
            "filing:filing-1"
        ]
        .map(str::to_string)
        .into_iter()
        .collect()
    );
    assert_eq!(
        evidence(&db, None, Some("2026-03-22"), None, 200, None)
            .map(|i| ids(&i))
            .unwrap(),
        ["bar:10", "bar:5"]
    );
    assert_eq!(
        evidence(&db, None, Some("2026-01-01"), None, 200, None)
            .map(|i| ids(&i))
            .unwrap(),
        EXPECTED_ORDER
    );
}

#[test]
fn kind_filters() {
    let db = seed();
    let kind = |k: &str| evidence(&db, None, None, Some(k), 200, None).unwrap();
    assert_eq!(ids(&kind("news")), ["news:news-1"]);
    assert_eq!(ids(&kind("filing")), ["filing:filing-1"]);
    assert_eq!(ids(&kind("event")), ["event:event-1"]);
    assert_eq!(ids(&kind("fundamental")), ["fundamental:1"]);
    assert_eq!(
        ids(&kind("bar")),
        EXPECTED_ORDER
            .iter()
            .filter(|id| id.starts_with("bar:"))
            .map(|s| s.to_string())
            .collect::<Vec<_>>()
    );
    assert!(kind("web").is_empty());
    let combined = evidence(&db, Some(INST), Some("2026-03-20"), Some("bar"), 200, None).unwrap();
    assert_eq!(ids(&combined), ["bar:5", "bar:4", "bar:3"]);
}

#[test]
fn filings_detected_from_sec_edgar_source() {
    let mut db = db();
    db.store_items(&[
        NewsItem {
            id: "n1".to_string(),
            instrument_ids: vec![INST.to_string()],
            published: ts("2026-03-20 12:00:00"),
            title: "Item n1".to_string(),
            url: "https://example.com/n1".to_string(),
            body: None,
            source: "rss".to_string(),
        }
        .into(),
        NewsItem {
            id: "f1".to_string(),
            instrument_ids: vec![INST.to_string()],
            published: ts("2026-03-20 12:00:00"),
            title: "Item f1".to_string(),
            url: "https://example.com/f1".to_string(),
            body: None,
            source: "sec_edgar".to_string(),
        }
        .into(),
    ])
    .unwrap();
    let items = evidence(&db, None, None, None, 200, None).unwrap();
    let kinds: std::collections::BTreeMap<&str, &str> = items
        .iter()
        .map(|i| (i.id.as_str(), i.kind.as_str()))
        .collect();
    assert_eq!(kinds.get("news:n1"), Some(&"news"));
    assert_eq!(kinds.get("filing:f1"), Some(&"filing"));
    assert!(items
        .iter()
        .filter(|i| i.kind == "filing")
        .all(|i| i.source == "sec_edgar"));
    assert_eq!(ids(&kind_all(&db, "news")), ["news:n1"]);
}

fn kind_all(db: &Db, kind: &str) -> Vec<EvidenceItem> {
    evidence(db, None, None, Some(kind), 200, None).unwrap()
}

#[test]
fn asx_announcements_are_primary_filings() {
    let mut db = db();
    db.store_items(&[NewsItem {
        id: "asx-1".to_string(),
        instrument_ids: vec![INST.to_string()],
        published: ts("2026-03-21 00:00:00"),
        title: "Price sensitive announcement".to_string(),
        url: "https://example.com/asx-1".to_string(),
        body: None,
        source: "asx_announcements".to_string(),
    }
    .into()])
        .unwrap();

    let item = &kind_all(&db, "filing")[0];
    assert_eq!(item.id, "filing:asx-1");
    assert_eq!(item.quality, "primary");
}

#[test]
fn cite_is_deterministic_and_omits_url() {
    let db = seed();
    let items = evidence(&db, None, None, None, 200, None).unwrap();
    assert_eq!(
        cite(by_id(&items, "news:news-1")),
        "[rss] Apple and Microsoft sign cloud deal <https://example.com/news-1>"
    );
    assert_eq!(
        cite(by_id(&items, "filing:filing-1")),
        "[sec_edgar] Apple 10-K filed <https://example.com/filing-1>"
    );
    assert_eq!(cite(by_id(&items, "bar:5")), "[test] US:AAPL close 102.00");
    assert_eq!(
        cite(by_id(&items, "event:event-1")),
        "[test/model] earnings: Reported EPS above consensus"
    );
    assert!(!cite(by_id(&items, "bar:5")).contains('<'));
    let again = evidence(&db, None, None, None, 200, None).unwrap();
    let first: Vec<String> = items.iter().map(cite).collect();
    let second: Vec<String> = again.iter().map(cite).collect();
    assert_eq!(first, second);
}

#[test]
fn limit_is_respected() {
    let db = seed();
    assert_eq!(
        evidence(&db, None, None, None, 200, None).unwrap().len(),
        14
    );
    assert_eq!(
        ids(&evidence(&db, None, None, None, 3, None).unwrap()),
        ["bar:10", "bar:5", "filing:filing-1"]
    );
    assert!(evidence(&db, None, None, None, 0, None).unwrap().is_empty());
    assert_eq!(
        evidence(&db, None, None, None, 200, None).unwrap().len(),
        14
    );
}

#[test]
fn search_matches_title_body_and_source_case_insensitively() {
    let db = seed();
    // Body match ("partnership" only appears in the body), case-insensitive.
    let items = evidence(&db, None, None, None, 200, Some("PARTNERSHIP")).unwrap();
    assert_eq!(ids(&items), ["news:news-1"]);
    // Source match (both the filing's `sec_edgar` and the fundamental's
    // `edgar` contain the needle).
    let items = evidence(&db, None, None, None, 200, Some("edgar")).unwrap();
    assert_eq!(ids(&items), ["filing:filing-1", "fundamental:1"]);
    assert!(evidence(&db, None, None, None, 200, Some("no-such-needle"))
        .unwrap()
        .is_empty());
}

#[test]
fn evidence_by_ids_reaches_past_the_pool_window() {
    let db = seed();
    let recent = ids(&evidence(&db, None, None, None, 2, None).unwrap());
    let aged = "fundamental:1";
    assert!(!recent.iter().any(|id| id == aged));

    let found = evidence_by_ids(
        &db,
        &[
            aged.to_string(),
            "news:news-1".to_string(),
            "bar:1".to_string(),
            "event:event-1".to_string(),
        ],
    )
    .unwrap();

    assert_eq!(ids(&found), ["news:news-1", "event:event-1", "bar:1", aged]);
}

#[test]
fn evidence_by_ids_skips_unknown_and_malformed_ids() {
    let db = seed();
    assert!(evidence_by_ids(&db, &[]).unwrap().is_empty());
    assert!(
        evidence_by_ids(&db, &["no-prefix".to_string(), "news:nope".to_string()])
            .unwrap()
            .is_empty()
    );
}

#[test]
fn news_instrument_mapping_drives_target_filtering() {
    let db = seed();
    // B9: the seeded news rows are mirrored into the indexed join table.
    let mut stmt = db
        .conn()
        .prepare("SELECT news_id, instrument_id FROM news_instrument")
        .unwrap();
    let links: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert!(links.contains(&("news-1".to_string(), INST.to_string())));
    assert!(links.contains(&("news-1".to_string(), OTHER.to_string())));

    let aapl = evidence(&db, Some(INST), None, Some("news"), 200, None).unwrap();
    assert_eq!(ids(&aapl), ["news:news-1"]);
    let msft = evidence(&db, Some(OTHER), None, Some("news"), 200, None).unwrap();
    assert_eq!(ids(&msft), ["news:news-1"]);
    assert!(
        evidence(&db, Some("US:NONE"), None, Some("news"), 200, None)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn news_instrument_mapping_backfilled_at_init() {
    // Existing JSON ``instrument_ids`` are mirrored into the join table on
    // startup.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.db");
    {
        let mut db = Db::open(&path).unwrap();
        db.store_items(&[NewsItem {
            id: "old-1".to_string(),
            instrument_ids: vec![INST.to_string(), OTHER.to_string()],
            published: ts("2026-01-01 00:00:00"),
            title: "Old".to_string(),
            url: NEWS_URL.to_string(),
            body: None,
            source: "rss".to_string(),
        }
        .into()])
            .unwrap();
        db.conn()
            .execute("DELETE FROM news_instrument", [])
            .unwrap();
    }
    // A fresh handle over the same file backfills the mapping idempotently.
    let db = Db::open(&path).unwrap();
    let mut stmt = db
        .conn()
        .prepare("SELECT news_id, instrument_id FROM news_instrument")
        .unwrap();
    let links: Vec<(String, String)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .filter_map(|r| r.ok())
        .collect();
    assert_eq!(
        links,
        vec![
            ("old-1".to_string(), INST.to_string()),
            ("old-1".to_string(), OTHER.to_string()),
        ]
    );
}

#[test]
fn constants_and_shared_falsifier_matcher() {
    assert_eq!(FILING_SOURCE, "sec_edgar");
    assert_eq!(PRIMARY_FILING_SOURCES, ["sec_edgar", "asx_announcements"]);
    assert_eq!(source_quality("sec_edgar"), "primary");
    assert_eq!(source_quality("asx_announcements"), "primary");
    assert_eq!(source_quality("rss"), "secondary");

    // The shared matcher searches kind, title and body together.
    let item = EvidenceItem {
        id: "news:1".to_string(),
        target_ids: vec![],
        ts: ts("2026-01-01 00:00:00"),
        kind: "news".to_string(),
        title: "Steady quarter".to_string(),
        body: Some("Guidance CUT for the year".to_string()),
        source: "rss".to_string(),
        url: None,
        sentiment: None,
        raw: serde_json::Value::Null,
        quality: "secondary".to_string(),
    };
    let terms = vec!["guidance cut".to_string()];
    assert!(falsifier_hit(&item, &terms));
    // Case-insensitive via casefold-style matching.
    assert!(falsifier_hit(&item, &["GUIDANCE CUT".to_lowercase()]));
    // Kind counts too.
    assert!(falsifier_hit(&item, &["news".to_string()]));
    assert!(!falsifier_hit(&item, &[]));
    assert!(!falsifier_hit(&item, &["nothing matches".to_string()]));
}
