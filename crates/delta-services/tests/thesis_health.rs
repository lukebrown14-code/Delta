//! Thesis health tests, mirroring `tests/test_thesis_health.py` (the pure
//! compute cases; the TUI screen case is R3 territory).

use chrono::{Duration, NaiveDateTime};
use delta_services::thesis_health::{
    badge_text, compute_health, evidence_by_ids, state_style, thesis_fleet, EvidenceItem,
    EvidenceSide, HealthState, Thesis,
};

fn now() -> NaiveDateTime {
    NaiveDateTime::parse_from_str("2026-09-16 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap()
}

fn thesis(falsifiers: &[&str]) -> Thesis {
    Thesis {
        id: "t1".to_string(),
        claim: "Solar grows for a decade".to_string(),
        scope: String::new(),
        assumptions: Vec::new(),
        falsifiers: falsifiers.iter().map(|f| f.to_string()).collect(),
        targets: Vec::new(),
        time_horizon: String::new(),
        created_at: now() - Duration::days(30),
        status: "active".to_string(),
    }
}

fn linked(id: &str, days: i64, side: EvidenceSide, title: &str) -> (EvidenceItem, EvidenceSide) {
    (
        EvidenceItem {
            id: id.to_string(),
            target_ids: vec!["US:AAPL".to_string()],
            ts: now() - Duration::days(days),
            kind: "news".to_string(),
            title: title.to_string(),
            body: None,
            source: "test".to_string(),
            url: None,
            sentiment: None,
            raw: serde_json::Value::Null,
            quality: "secondary".to_string(),
        },
        side,
    )
}

fn s(id: &str, days: i64) -> (EvidenceItem, EvidenceSide) {
    linked(id, days, EvidenceSide::Support, "Something happened")
}

fn a(id: &str, days: i64) -> (EvidenceItem, EvidenceSide) {
    linked(id, days, EvidenceSide::Against, "Something happened")
}

#[test]
fn every_state_reachable() {
    let cases: Vec<(&str, Vec<(EvidenceItem, EvidenceSide)>)> = vec![
        ("emerging", vec![s("a", 1)]),
        ("building", (0..4).map(|i| s(&format!("s{i}"), 1)).collect()),
        (
            "mixed",
            vec![s("s1", 1), s("s2", 2), a("a1", 1), a("a2", 2)],
        ),
        (
            "weakening",
            std::iter::once(s("s1", 1))
                .chain((1..4).map(|i| a(&format!("a{i}"), i)))
                .collect(),
        ),
        (
            "challenged",
            (0..4)
                .map(|i| linked(&format!("s{i}"), 1, EvidenceSide::Support, "Guidance cut"))
                .collect(),
        ),
        ("idle", (0..4).map(|i| s(&format!("s{i}"), 30)).collect()),
    ];
    let thesis = thesis(&["guidance cut"]);
    for (expected, linked_items) in cases {
        let result = compute_health(&thesis, &linked_items, now(), 14, 3);
        assert_eq!(result.state.as_str(), expected, "{expected}: {result:?}");
    }
}

#[test]
fn falsifier_beats_tilt_but_not_coverage() {
    let thesis = thesis(&["policy reversal"]);
    let building: Vec<_> = (0..4).map(|i| s(&format!("s{i}"), 1)).collect();
    assert_eq!(
        compute_health(&thesis, &building, now(), 14, 3).state,
        HealthState::Building
    );
    let mut hit = building.clone();
    hit[0] = linked(
        "s0",
        1,
        EvidenceSide::Support,
        "Policy reversal hits demand",
    );
    assert_eq!(
        compute_health(&thesis, &hit, now(), 14, 3).state,
        HealthState::Challenged
    );
    let sparse = vec![hit[0].clone()];
    assert_eq!(
        compute_health(&thesis, &sparse, now(), 14, 3).state,
        HealthState::Emerging
    );
}

#[test]
fn one_fresh_item_prevents_idle() {
    let thesis = thesis(&[]);
    let mut linked_items = vec![s("s0", 1)];
    linked_items.extend((1..4).map(|i| s(&format!("s{i}"), 30)));
    assert_eq!(
        compute_health(&thesis, &linked_items, now(), 14, 3).state,
        HealthState::Building
    );
    let stale: Vec<_> = (0..4).map(|i| s(&format!("s{i}"), 30)).collect();
    assert_eq!(
        compute_health(&thesis, &stale, now(), 14, 3).state,
        HealthState::Idle
    );
}

#[test]
fn recency_weighting_counts_old_items_half() {
    let thesis = thesis(&[]);
    let linked_items = vec![s("s1", 1), a("a1", 30), a("a2", 40)];
    let result = compute_health(&thesis, &linked_items, now(), 14, 3);
    assert_eq!(result.tilt, 0.0);
    assert_eq!(result.state, HealthState::Mixed);
    let one_old = vec![s("s1", 1), a("a1", 30)];
    assert!(compute_health(&thesis, &one_old, now(), 14, 3).tilt > 0.25);
}

#[test]
fn drivers_capped_and_ordered() {
    let thesis = thesis(&["guidance cut"]);
    let mut linked_items = vec![linked("hit", 1, EvidenceSide::Support, "Guidance cut")];
    linked_items.extend((1..7).map(|i| s(&format!("s{i}"), i)));
    let result = compute_health(&thesis, &linked_items, now(), 14, 3);
    assert_eq!(result.drivers.len(), 5);
    assert_eq!(result.drivers[0], "hit");
}

#[test]
fn badge_text_and_styles() {
    let thesis = thesis(&[]);
    let linked_items: Vec<_> = (0..4).map(|i| s(&format!("s{i}"), 1)).collect();
    let result = compute_health(&thesis, &linked_items, now(), 14, 3);
    let badge = badge_text(&result);
    assert!(badge.contains("building"));
    assert!(badge.contains("tilt +1.00"));
    assert_eq!(state_style(HealthState::Challenged), "red");
    assert_eq!(state_style(HealthState::Building), "green");
}

/// Seed helper for the fleet tests (the `test_screen_renders_health_badge`
/// data, without the screen).
fn seeded_db() -> delta_core::db::Db {
    let mut db = delta_core::db::Db::open_memory().unwrap();
    let now = now();
    let mut items = Vec::new();
    for i in 0..4 {
        items.push(delta_core::db::StoreItem::News(
            delta_core::models::NewsItem {
                id: format!("n{i}"),
                instrument_ids: vec!["US:AAPL".to_string()],
                published: now - Duration::days(1),
                title: format!("Supporting item {i}"),
                url: format!("https://example.com/{i}"),
                body: None,
                source: "rss".to_string(),
            },
        ));
    }
    db.store_items(&items).unwrap();
    delta_services::thesis_health::ensure_tables(&db).unwrap();
    db.conn()
        .execute(
            "INSERT INTO thesis (id, claim, scope, assumptions, falsifiers, targets, time_horizon, created_at, status) \
             VALUES ('t1', 'Solar grows for a decade', '', '[]', '[]', '[\"US:AAPL\"]', '', ?1, 'active')",
            [now.format("%Y-%m-%d %H:%M:%S%.6f").to_string()],
        )
        .unwrap();
    for i in 0..4 {
        db.conn()
            .execute(
                "INSERT INTO thesis_evidence (thesis_id, evidence_id, side, note, accepted) \
                 VALUES ('t1', ?1, 'support', 'supports', 1)",
                [format!("news:n{i}")],
            )
            .unwrap();
    }
    db
}

#[test]
fn fleet_reads_theses_and_evidence() {
    let db = seeded_db();
    let by_ids = evidence_by_ids(&db, &["news:n0".to_string(), "news:nope".to_string()]).unwrap();
    assert_eq!(by_ids.len(), 1);
    assert_eq!(by_ids[0].id, "news:n0");
    assert_eq!(by_ids[0].title, "Supporting item 0");

    let fleet = thesis_fleet(&db, Some(now())).unwrap();
    assert_eq!(fleet.len(), 1);
    let entry = &fleet[0];
    assert_eq!(entry.thesis.claim, "Solar grows for a decade");
    let result = entry.result.as_ref().expect("accepted evidence exists");
    assert_eq!(result.state, HealthState::Building);
    assert!((result.tilt - 1.0).abs() < 1e-12);
    assert_eq!(result.support, 4);
    assert_eq!(result.against, 0);
    assert!(badge_text(result).contains("building"));
}

#[test]
fn fleet_most_at_risk_first_and_empty_theses_idle() {
    let db = seeded_db();
    let now = now();
    // A second thesis with no evidence lands after the healthy one; a
    // challenged one (stale + falsifier) lands first.
    db.conn()
        .execute(
            "INSERT INTO thesis (id, claim, scope, assumptions, falsifiers, targets, time_horizon, created_at, status) \
             VALUES ('t2', 'A claim with no evidence', '', '[]', '[]', '[]', '', ?1, 'active')",
            [now.format("%Y-%m-%d %H:%M:%S%.6f").to_string()],
        )
        .unwrap();
    let fleet = thesis_fleet(&db, Some(now)).unwrap();
    assert_eq!(fleet.len(), 2);
    assert_eq!(fleet[0].thesis.id, "t2"); // emerging (risk 4) before building (risk 5)
    assert_eq!(fleet[1].thesis.id, "t1");
    assert!(fleet[0].result.is_none());
    assert_eq!(fleet[0].state(), HealthState::Emerging);
}
