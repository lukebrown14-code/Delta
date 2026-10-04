use chrono::NaiveDate;
use delta_core::{
    db::{Db, StoreItem},
    models::NewsItem,
};
use delta_services::evidence_filtered;

#[test]
fn search_and_kind_filters_find_older_items_before_limiting() {
    let mut db = Db::open_memory().unwrap();
    let rows = [
        ("old", 1, "sec_edgar", "Annual filing"),
        ("new", 2, "rss", "New headline"),
    ];
    for (id, day, source, title) in rows {
        db.store_items(&[StoreItem::News(NewsItem {
            id: id.into(),
            instrument_ids: vec!["US:AAPL".into()],
            published: NaiveDate::from_ymd_opt(2026, 10, day)
                .unwrap()
                .and_hms_opt(0, 0, 0)
                .unwrap(),
            title: title.into(),
            url: "https://example.test".into(),
            body: Some("Stored text".into()),
            source: source.into(),
        })])
        .unwrap();
    }
    let found = evidence_filtered(&db, Some("US:AAPL"), None, None, 1, Some("annual")).unwrap();
    assert_eq!(found[0].id, "filing:old");
    let filings = evidence_filtered(&db, None, None, Some("filing"), 1, None).unwrap();
    assert_eq!(filings[0].id, "filing:old");
    assert!(
        evidence_filtered(&db, None, Some("2026-10-02"), Some("filing"), 1, None)
            .unwrap()
            .is_empty()
    );
    assert!(evidence_filtered(&db, None, Some("invalid"), None, 1, None).is_err());
    assert_eq!(
        delta_services::source_url(&db, "filing:old")
            .unwrap()
            .as_deref(),
        Some("https://example.test/")
    );
    assert!(delta_services::source_url(&db, "file:///tmp/private")
        .unwrap()
        .is_none());
    assert!(delta_services::source_url(&db, "javascript:alert(1)")
        .unwrap()
        .is_none());
}

#[test]
fn malformed_stored_evidence_is_reported_and_unknown_ids_remain_skipped() {
    let mut db = Db::open_memory().unwrap();
    db.store_items(&[StoreItem::News(NewsItem {
        id: "broken".into(),
        instrument_ids: vec!["US:AAPL".into()],
        published: NaiveDate::from_ymd_opt(2026, 10, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        title: "Bad stored time".into(),
        url: "https://example.test".into(),
        body: None,
        source: "rss".into(),
    })])
    .unwrap();
    db.conn()
        .execute(
            "UPDATE newsitem SET published='invalid' WHERE id='broken'",
            [],
        )
        .unwrap();
    assert!(delta_services::evidence_by_ids(&db, &["news:broken".into()]).is_err());
    assert!(
        delta_services::evidence_by_ids(&db, &["news:missing".into()])
            .unwrap()
            .is_empty()
    );
    db.conn()
        .execute(
            "UPDATE newsitem SET published='2026-10-01 00:00:00' WHERE id='broken'",
            [],
        )
        .unwrap();
    // A valid timestamp restores normal reads; unknown IDs still do not create synthetic evidence.
    assert_eq!(
        delta_services::evidence_by_ids(&db, &["news:broken".into()])
            .unwrap()
            .len(),
        1
    );
}
