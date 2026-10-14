//! Tests for the facts-only brief builder, mirroring `tests/test_brief.py`.
//! The Phase 1 price-section golden is captured from the same 80-bar seed
//! (100.0 rising 0.5/day from 2026-01-01).

use chrono::{Duration, NaiveDate, NaiveDateTime};
use delta_core::db::{Db, StoreItem};
use delta_core::models::{AssetClass, Event, EventKind, Fundamental, Instrument, NewsItem};
use delta_services::brief::{brief_for, build_brief};

fn ts(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn as_of() -> NaiveDateTime {
    ts("2026-03-22 00:00:00")
}

fn inst() -> Instrument {
    Instrument {
        id: "US:AAPL".to_string(),
        market: "us".to_string(),
        symbol: "AAPL".to_string(),
        name: None,
        currency: "USD".to_string(),
        sector: Some("Technology".to_string()),
        asset_class: AssetClass::Equity,
        watchlists: Vec::new(),
        tags: Default::default(),
        industry: None,
        meta: Default::default(),
    }
}

fn other() -> Instrument {
    Instrument {
        sector: None,
        ..inst()
    }
}

fn other_mut() -> Instrument {
    let mut o = other();
    o.id = "US:MSFT".to_string();
    o.symbol = "MSFT".to_string();
    o
}

const PHASE1_PRICE_SECTION: &str = "Latest close: 139.50 USD\n\
     20-day return: +7.72%\n\
     Return over window: +39.50%\n\
     20-day SMA: 134.75, 50-day SMA: 127.25\n\
     60-day range: 110.00 - 139.50\n\
     Sector: Technology";

/// `tests/conftest.py::seed_bars` parity: n daily bars from `start`, prices
/// rising 0.5/day from 100.0.
fn seed_bars(db: &mut Db, instrument_id: &str, start: NaiveDateTime, n: usize) {
    let items: Vec<StoreItem> = (0..n)
        .map(|i| {
            let price = 100.0 + i as f64 * 0.5;
            StoreItem::Bar(delta_core::models::Bar {
                instrument_id: instrument_id.to_string(),
                ts: start + Duration::days(i as i64),
                open: price,
                high: price,
                low: price,
                close: price,
                volume: 1000.0,
                source: "test".to_string(),
            })
        })
        .collect();
    db.store_items(&items).unwrap();
}

fn news(id: &str, instrument_ids: &[&str], published: NaiveDateTime, title: &str) -> StoreItem {
    StoreItem::News(NewsItem {
        id: id.to_string(),
        instrument_ids: instrument_ids.iter().map(|s| s.to_string()).collect(),
        published,
        title: title.to_string(),
        url: format!("https://example.com/{id}"),
        body: None,
        source: "rss".to_string(),
    })
}

fn seed_everything(db: &mut Db) {
    let as_of = as_of();
    seed_bars(db, "US:AAPL", ts("2026-01-01 00:00:00"), 80);
    db.store_items(&[
        news(
            "news-1",
            &["US:AAPL"],
            as_of - Duration::days(2),
            "Apple announces results",
        ),
        news(
            "news-2",
            &["US:MSFT"],
            as_of - Duration::days(1),
            "Microsoft news",
        ),
        news(
            "news-3",
            &["US:AAPL"],
            as_of - Duration::days(30),
            "Old news",
        ),
        StoreItem::Event(Event {
            id: "event-past".to_string(),
            instrument_id: "US:AAPL".to_string(),
            ts: as_of - Duration::days(3),
            kind: EventKind::Earnings,
            summary: "Reported EPS above consensus".to_string(),
            sentiment: 0.4,
            evidence_ids: vec!["news-1".to_string()],
            extracted_by: "test/model".to_string(),
            prompt_version: "extract_v1".to_string(),
        }),
        StoreItem::Event(Event {
            id: "event-future".to_string(),
            instrument_id: "US:AAPL".to_string(),
            ts: as_of + Duration::days(10),
            kind: EventKind::Dividend,
            summary: "Ex-dividend 2026-04-01".to_string(),
            sentiment: 0.0,
            evidence_ids: vec![],
            extracted_by: "yfinance".to_string(),
            prompt_version: "n/a".to_string(),
        }),
        StoreItem::Fundamental(Fundamental {
            instrument_id: "US:AAPL".to_string(),
            as_of: NaiveDate::from_ymd_opt(2025, 12, 31).unwrap(),
            metric: "eps".to_string(),
            value: 6.1,
            source: "edgar".to_string(),
        }),
        // older value of the same metric, must be superseded
        StoreItem::Fundamental(Fundamental {
            instrument_id: "US:AAPL".to_string(),
            as_of: NaiveDate::from_ymd_opt(2025, 9, 30).unwrap(),
            metric: "eps".to_string(),
            value: 5.9,
            source: "edgar".to_string(),
        }),
    ])
    .unwrap();
}

#[test]
fn price_section_matches_phase1() {
    let mut db = Db::open_memory().unwrap();
    seed_bars(&mut db, "US:AAPL", ts("2026-01-01 00:00:00"), 80);
    let brief = build_brief(&db, &inst(), as_of()).unwrap();
    assert_eq!(brief.prices.render(), PHASE1_PRICE_SECTION);
    assert_eq!(brief.prices.evidence_ids.len(), 60);
    assert_eq!(
        brief.render(),
        format!(
            "{}\n{}\n{}\n{}\n{}",
            PHASE1_PRICE_SECTION,
            "Fundamentals: none available",
            "Events: none available",
            "Upcoming events: none available",
            "News: none available"
        )
    );
}

#[test]
fn all_sections_render_with_evidence() {
    let mut db = Db::open_memory().unwrap();
    seed_everything(&mut db);
    let brief = build_brief(&db, &inst(), as_of()).unwrap();

    assert_eq!(
        brief.news.lines,
        vec!["2026-03-20 [rss] Apple announces results".to_string()]
    );
    assert_eq!(brief.news.evidence_ids, vec!["news:news-1".to_string()]);

    assert_eq!(
        brief.events.lines,
        vec!["2026-03-19 earnings: Reported EPS above consensus (sentiment +0.4)".to_string()]
    );
    assert_eq!(
        brief.events.evidence_ids,
        vec!["event:event-past".to_string()]
    );

    assert_eq!(
        brief.calendar.lines,
        vec!["2026-04-01 dividend: Ex-dividend 2026-04-01".to_string()]
    );
    assert_eq!(
        brief.calendar.evidence_ids,
        vec!["event:event-future".to_string()]
    );

    assert_eq!(
        brief.fundamentals.lines,
        vec!["eps: 6.10 (as of 2025-12-31, edgar)".to_string()]
    );
    assert_eq!(brief.fundamentals.evidence_ids.len(), 1);

    let ids = brief.evidence_ids();
    assert!(ids.contains(&"news:news-1".to_string()));
    assert!(ids.contains(&"event:event-past".to_string()));
    assert!(ids.contains(&"event:event-future".to_string()));
    assert!(ids.iter().any(|i| i.starts_with("fundamental:")));
    assert!(ids.iter().any(|i| i.starts_with("bar:")));
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(ids.len(), sorted.len(), "evidence ids must be unique");

    let text = brief.render();
    assert!(!text.contains("none available"));
    assert!(text.starts_with(PHASE1_PRICE_SECTION));
}

#[test]
fn no_bars_returns_none() {
    let db = Db::open_memory().unwrap();
    assert!(build_brief(&db, &inst(), as_of()).is_none());
}

/// `services.brief_for`: unknown instruments have no brief.
#[test]
fn brief_for_finds_the_instrument_or_none() {
    let mut db = Db::open_memory().unwrap();
    seed_bars(&mut db, "US:AAPL", ts("2026-01-01 00:00:00"), 80);
    let universe = vec![inst(), other_mut()];
    assert!(brief_for(&db, &universe, "US:AAPL", as_of()).is_some());
    assert!(brief_for(&db, &universe, "US:NOPE", as_of()).is_none());
}

/// `_fmt_value` magnitudes, including Python's `{:,.2f}` comma grouping.
#[test]
fn fmt_value_magnitudes() {
    use delta_services::brief::fmt_value;
    assert_eq!(fmt_value(2.656e11), "265.60B");
    assert_eq!(fmt_value(7.46), "7.46");
    assert_eq!(fmt_value(12345.678), "12,345.68");
    assert_eq!(fmt_value(1.5e12), "1.50T");
    assert_eq!(fmt_value(2.5e6), "2.50M");
}
