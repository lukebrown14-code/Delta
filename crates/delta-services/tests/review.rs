//! Port of `tests/test_review.py` (read-only evidence quality and the
//! review queue). The Python `Rig` becomes explicit arguments.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDateTime};

use delta_core::db::{Db, StoreItem};
use delta_core::models::{Bar, Instrument, NewsItem};
use delta_services::review::{
    evidence_audit, primary_sources_for, review_queue, PluginInfo, EVIDENCE_WINDOW,
    MIN_NON_PRICE_ITEMS, MIN_SOURCES, NEWS_STALE_AFTER, PRICE_STALE_AFTER, PRIMARY_STALE_AFTER,
};

const NOW_STR: &str = "2026-01-31 00:00:00";

fn now() -> NaiveDateTime {
    NaiveDateTime::parse_from_str(NOW_STR, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn inst(id: &str, market: &str, symbol: &str) -> Instrument {
    Instrument {
        id: id.to_string(),
        market: market.to_string(),
        symbol: symbol.to_string(),
        name: None,
        currency: "USD".to_string(),
        sector: None,
        asset_class: delta_core::models::AssetClass::Equity,
        watchlists: Vec::new(),
        tags: Default::default(),
        industry: None,
        meta: Default::default(),
    }
}

fn acme() -> Instrument {
    inst("US:ACME", "us", "ACME")
}

fn asx() -> Instrument {
    inst("ASX:BHP", "asx", "BHP")
}

fn db() -> Db {
    Db::open_memory().unwrap()
}

fn bar(inst_id: &str, at: NaiveDateTime) -> StoreItem {
    Bar {
        instrument_id: inst_id.to_string(),
        ts: at,
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        volume: 1.0,
        source: "yfinance".to_string(),
    }
    .into()
}

fn news(id: &str, inst_id: &str, at: NaiveDateTime, source: &str, title: &str) -> StoreItem {
    NewsItem {
        id: id.to_string(),
        instrument_ids: vec![inst_id.to_string()],
        published: at,
        title: title.to_string(),
        body: if title == "Company guidance cut" {
            Some("guidance cut".to_string())
        } else {
            None
        },
        url: format!("https://example.test/{id}"),
        source: source.to_string(),
    }
    .into()
}

fn sec_plugins() -> BTreeMap<String, PluginInfo> {
    BTreeMap::from([(
        "sec_edgar".to_string(),
        PluginInfo {
            enabled: true,
            market: None, // falls back to the sec_edgar -> us mapping
        },
    )])
}

#[test]
fn audit_marks_primary_not_configured_instead_of_absent() {
    let mut db = db();
    let now = now();
    db.store_items(&[
        bar("US:ACME", now - Duration::days(1)),
        news("n1", "US:ACME", now - Duration::days(1), "rss", "News"),
    ])
    .unwrap();
    let audit = evidence_audit(&db, "US:ACME", now, &Default::default(), None).unwrap();
    assert_eq!(audit.primary_coverage, "not_configured");
    assert!(audit
        .warnings
        .contains(&"no primary disclosure source configured".to_string()));
}

#[test]
fn audit_tracks_stale_data_and_recent_source_diversity() {
    let mut db = db();
    let now = now();
    db.store_items(&[
        bar("US:ACME", now - Duration::days(4)),
        news("n1", "US:ACME", now - Duration::days(15), "rss", "News"),
        news(
            "f1",
            "US:ACME",
            now - Duration::days(31),
            "sec_edgar",
            "Filing",
        ),
    ])
    .unwrap();
    let sources = BTreeSet::from(["sec_edgar".to_string()]);
    let audit = evidence_audit(&db, "US:ACME", now, &sources, None).unwrap();
    assert_eq!(audit.primary_coverage, "stale");
    assert_eq!(audit.non_price_items, 1);
    assert_eq!(audit.source_count, 1);
    assert!(audit
        .warnings
        .contains(&"latest price is 4 days old".to_string()));
    assert!(audit
        .warnings
        .contains(&"latest news is 15 days old".to_string()));
    assert!(audit.warnings.contains(&"thin recent evidence".to_string()));
    assert!(audit.warnings.contains(&"low source diversity".to_string()));
}

#[test]
fn primary_source_scope_and_asx_filing_classification() {
    let mut db = db();
    let universe = vec![acme(), asx()];
    let plugins = BTreeMap::from([
        (
            "sec_edgar".to_string(),
            PluginInfo {
                enabled: true,
                market: None,
            },
        ),
        (
            "asx_announcements".to_string(),
            PluginInfo {
                enabled: true,
                market: None,
            },
        ),
    ]);
    assert_eq!(
        primary_sources_for(&plugins, &universe, "US:ACME"),
        BTreeSet::from(["sec_edgar".to_string()])
    );
    assert_eq!(
        primary_sources_for(&plugins, &universe, "ASX:BHP"),
        BTreeSet::from(["asx_announcements".to_string()])
    );
    let now = now();
    db.store_items(&[news(
        "asx1",
        "ASX:BHP",
        now - Duration::days(1),
        "asx_announcements",
        "Announcement",
    )])
    .unwrap();
    let sources = primary_sources_for(&plugins, &universe, "ASX:BHP");
    let audit = evidence_audit(&db, "ASX:BHP", now, &sources, None).unwrap();
    assert_eq!(audit.primary_coverage, "fresh");
}

#[test]
fn queue_ranks_disclosures_then_falsifiers_then_coverage() {
    let mut db = db();
    let universe = vec![acme()];
    let plugins = sec_plugins();
    let now = now();
    let thesis = delta_services::theses::create_thesis(
        &db,
        "Revenue keeps rising",
        "",
        &[],
        &["guidance cut".to_string()],
        &["US:ACME".to_string()],
        "",
        None,
    )
    .unwrap();
    db.store_items(&[
        bar("US:ACME", now - Duration::days(5)),
        news(
            "filing",
            "US:ACME",
            now - Duration::days(1),
            "sec_edgar",
            "10-K filed",
        ),
        news(
            "match",
            "US:ACME",
            now - Duration::days(1),
            "rss",
            "Company guidance cut",
        ),
        news(
            "future",
            "US:ACME",
            now + Duration::days(1),
            "sec_edgar",
            "Future filing",
        ),
    ])
    .unwrap();
    let queue = review_queue(
        &db,
        &plugins,
        &universe,
        &[],
        Some(now - Duration::days(2)),
        now,
    )
    .unwrap();
    let top: Vec<(&str, Option<&String>)> = queue[..2]
        .iter()
        .map(|item| (item.kind, item.evidence_id.as_ref()))
        .collect();
    assert_eq!(
        top,
        [
            ("primary_disclosure", Some(&"filing:filing".to_string())),
            ("falsifier", Some(&"news:match".to_string())),
        ]
    );
    assert_eq!(queue[1].thesis_id.as_deref(), Some(thesis.id.as_str()));
    assert!(!queue
        .iter()
        .any(|item| item.evidence_id.as_deref() == Some("filing:future")));
    assert_eq!(
        queue[2..].iter().map(|item| item.kind).collect::<Vec<_>>(),
        ["stale", "thin_evidence"]
    );
}

#[test]
fn queue_is_deterministic_and_deduplicates_each_navigation_target() {
    let mut db = db();
    let universe = vec![acme()];
    let plugins = sec_plugins();
    let now = now();
    db.store_items(&[
        news("b", "US:ACME", now - Duration::days(1), "sec_edgar", "B"),
        news("a", "US:ACME", now - Duration::days(1), "sec_edgar", "A"),
    ])
    .unwrap();
    let args = (
        &db,
        &plugins,
        &universe,
        Vec::<String>::new(),
        Some(now - Duration::days(2)),
        now,
    );
    let first = review_queue(args.0, args.1, args.2, &args.3, args.4, args.5).unwrap();
    let second = review_queue(args.0, args.1, args.2, &args.3, args.4, args.5).unwrap();
    assert_eq!(first, second);
    let primary: Vec<Option<&String>> = first
        .iter()
        .filter(|item| item.kind == "primary_disclosure")
        .map(|item| item.evidence_id.as_ref())
        .collect();
    assert_eq!(
        primary,
        [Some(&"filing:a".to_string()), Some(&"filing:b".to_string())]
    );
}

#[test]
fn constants_match_python() {
    assert_eq!(PRICE_STALE_AFTER, Duration::days(3));
    assert_eq!(NEWS_STALE_AFTER, Duration::days(14));
    assert_eq!(PRIMARY_STALE_AFTER, Duration::days(30));
    assert_eq!(EVIDENCE_WINDOW, Duration::days(30));
    assert_eq!(MIN_NON_PRICE_ITEMS, 3);
    assert_eq!(MIN_SOURCES, 2);
}
