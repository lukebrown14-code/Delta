use chrono::{Duration, NaiveDate};
use delta_core::{
    config::AppConfig,
    db::{Db, StoreItem},
    models::{AssetClass, Instrument, NewsItem},
};
use delta_services::{create_thesis, review_queue, update_thesis, ThesisEdit};

#[test]
fn review_queue_prioritises_disclosures_and_body_falsifiers_and_excludes_future_items() {
    let mut db = Db::open_memory().unwrap();
    let now = NaiveDate::from_ymd_opt(2026, 10, 2)
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap();
    let inst = Instrument {
        id: "US:AAPL".into(),
        market: "us".into(),
        symbol: "AAPL".into(),
        name: None,
        currency: "USD".into(),
        sector: None,
        asset_class: AssetClass::Equity,
        watchlists: vec![],
        tags: Default::default(),
        industry: None,
        meta: Default::default(),
    };
    for (id, ts) in [
        ("current", now - Duration::days(1)),
        ("future", now + Duration::days(1)),
    ] {
        db.store_items(&[StoreItem::News(NewsItem {
            id: id.into(),
            instrument_ids: vec![inst.id.clone()],
            published: ts,
            title: "Filing".into(),
            url: "https://example.test".into(),
            body: Some("Revenue decline".into()),
            source: "sec_edgar".into(),
        })])
        .unwrap();
    }
    let thesis = create_thesis(&db, "Growth", "", std::slice::from_ref(&inst.id), "").unwrap();
    let thesis = update_thesis(
        &mut db,
        &thesis.id,
        &ThesisEdit {
            claim: thesis.claim,
            scope: thesis.scope,
            assumptions: vec![],
            falsifiers: vec!["decline".into()],
            targets: thesis.targets,
            time_horizon: "".into(),
            status: "active".into(),
        },
    )
    .unwrap();
    let items = review_queue(&db, &AppConfig::default(), &[inst], &[], None, now).unwrap();
    assert_eq!(items[0].kind, "primary_disclosure");
    assert_eq!(items[1].kind, "falsifier");
    assert_eq!(items[1].thesis_id.as_deref(), Some(thesis.id.as_str()));
    assert!(items
        .iter()
        .all(|item| item.evidence_id.as_deref() != Some("filing:future")));
    assert!(items.iter().any(|item| item.kind == "thin_evidence"));
}
