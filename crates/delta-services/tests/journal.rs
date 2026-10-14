//! Decision-journal flows across theses and decisions together (mined from
//! the donor line, adapted to the base service APIs). The candidate-
//! proposal flow is not here: `propose_evidence` lands with the theses
//! screen stream, which owns that wiring.

use chrono::NaiveDate;
use delta_core::db::{Db, StoreItem};
use delta_core::models::NewsItem;
use delta_services::{
    add_evidence, append_review, create_decision, create_thesis, delete_decision, due_reviews,
    evidence_for, get_decision, list_decisions, review_history, set_accepted, set_status,
    update_thesis,
};

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()
}

fn seed_news(db: &mut Db) {
    db.store_items(&[StoreItem::News(NewsItem {
        id: "n1".into(),
        instrument_ids: vec!["US:AAPL".into()],
        published: day().and_hms_opt(12, 0, 0).unwrap(),
        title: "Launch".into(),
        url: "https://example.test".into(),
        body: None,
        source: "rss".into(),
    })])
    .unwrap();
}

fn new_thesis(db: &Db, claim: &str) -> delta_services::Thesis {
    create_thesis(
        db,
        claim,
        "US",
        &["growth".to_string()],
        &["decline".to_string()],
        &["US:AAPL".to_string()],
        "3 years",
        None,
    )
    .unwrap()
}

#[test]
fn thesis_candidates_need_user_acceptance() {
    let mut db = Db::open_memory().unwrap();
    seed_news(&mut db);
    let thesis = new_thesis(&db, "Growth continues");
    // The same claim and scope is the same thesis: a duplicate errors.
    assert!(create_thesis(
        &db,
        "Growth continues",
        "US",
        &["growth".to_string()],
        &["decline".to_string()],
        &["US:AAPL".to_string()],
        "3 years",
        None
    )
    .is_err());
    let link = add_evidence(&db, &thesis.id, "news:n1", "support", "new product", None).unwrap();
    assert!(!link.accepted);
    assert!(evidence_for(&db, &thesis.id, true).unwrap().is_empty());
    set_accepted(&db, &thesis.id, "news:n1", true).unwrap();
    assert_eq!(evidence_for(&db, &thesis.id, true).unwrap().len(), 1);
    assert_eq!(
        set_status(&db, &thesis.id, "paused").unwrap().status,
        "paused"
    );
}

fn create_linked_decision(db: &Db, thesis_id: Option<&str>) -> delta_services::Decision {
    create_decision(
        db,
        "US:AAPL",
        "Product growth",
        "20x earnings",
        "3 years",
        day(),
        "Revenue declines",
        thesis_id,
        None,
    )
    .unwrap()
}

#[test]
fn decision_review_history_and_due_queue_survive_updates() {
    let db = Db::open_memory().unwrap();
    let thesis = new_thesis(&db, "Growth continues");
    let decision = create_linked_decision(&db, Some(&thesis.id));
    assert_eq!(
        decision.thesis_claim_snapshot.as_deref(),
        Some("Growth continues")
    );
    assert_eq!(due_reviews(&db, Some(day())).unwrap().len(), 1);
    append_review(&db, &decision.id, "Still valid", Some("reviewed"), None).unwrap();
    assert_eq!(review_history(&db, &decision.id).unwrap().len(), 1);
    assert_eq!(get_decision(&db, &decision.id).unwrap().status, "reviewed");
    append_review(&db, &decision.id, "Closed", Some("retired"), None).unwrap();
    assert!(due_reviews(&db, Some(day())).unwrap().is_empty());
    assert_eq!(list_decisions(&db, None, false).unwrap().len(), 0);
    delete_decision(&db, &decision.id).unwrap();
    assert!(get_decision(&db, &decision.id).is_err());
}

#[test]
fn thesis_rename_moves_links_without_rewriting_decision_history() {
    let mut db = Db::open_memory().unwrap();
    seed_news(&mut db);
    let thesis = new_thesis(&db, "Old claim");
    add_evidence(&db, &thesis.id, "news:n1", "support", "launch", Some(true)).unwrap();
    let decision = create_linked_decision(&db, Some(&thesis.id));
    let renamed = update_thesis(
        &db,
        &thesis.id,
        "New claim",
        &["US:AAPL".to_string()],
        "5 years",
        "active",
        Some("US"),
        Some(&["growth".to_string()]),
        Some(&["decline".to_string()]),
    )
    .unwrap();
    assert_ne!(renamed.id, thesis.id);
    assert_eq!(evidence_for(&db, &renamed.id, true).unwrap().len(), 1);
    let linked = get_decision(&db, &decision.id).unwrap();
    assert_eq!(linked.thesis_id.as_deref(), Some(renamed.id.as_str()));
    assert_eq!(linked.thesis_claim_snapshot.as_deref(), Some("Old claim"));
}
