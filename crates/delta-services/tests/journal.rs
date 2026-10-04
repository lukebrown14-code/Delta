use chrono::NaiveDate;
use delta_core::db::{Db, StoreItem};
use delta_core::models::NewsItem;
use delta_llm::{
    client::LlmClient,
    providers::{CompletionRequest, Provider, ProviderError, ProviderResult},
};
use delta_services::{
    add_thesis_evidence, append_review, create_decision, create_thesis, delete_decision,
    due_reviews, get_decision, list_decisions, propose_thesis_evidence, review_history,
    set_thesis_evidence_accepted, set_thesis_status, thesis_evidence, update_thesis, DecisionInput,
    EvidenceSide, ThesisEdit,
};
use std::sync::Arc;

struct CandidateProvider;

#[async_trait::async_trait]
impl Provider for CandidateProvider {
    fn name(&self) -> &'static str {
        "candidate"
    }
    async fn complete(&self, _req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        Ok(ProviderResult { text:serde_json::json!({"candidates":[{"evidence_id":"news:n1","side":"support","note":"launch"},{"evidence_id":"news:invented","side":"against","note":"unsupported"}]}).to_string(),input_tokens:1,output_tokens:1,cost_usd:0.0 })
    }
}

fn day() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 9, 30).unwrap()
}

#[test]
fn thesis_candidates_need_user_acceptance() {
    let mut db = Db::open_memory().unwrap();
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
    let thesis = create_thesis(
        &db,
        "Growth continues",
        "US",
        &["US:AAPL".into()],
        "3 years",
    )
    .unwrap();
    assert!(create_thesis(&db, "Growth continues", "US", &[], "").is_err());
    let link = add_thesis_evidence(
        &db,
        &thesis.id,
        "news:n1",
        EvidenceSide::Support,
        "new product",
        None,
    )
    .unwrap();
    assert!(!link.accepted);
    assert!(thesis_evidence(&db, &thesis.id, true).unwrap().is_empty());
    set_thesis_evidence_accepted(&db, &thesis.id, "news:n1", true).unwrap();
    assert_eq!(thesis_evidence(&db, &thesis.id, true).unwrap().len(), 1);
    assert_eq!(
        set_thesis_status(&db, &thesis.id, "paused").unwrap().status,
        "paused"
    );
    assert!(add_thesis_evidence(
        &db,
        &thesis.id,
        "news:missing",
        EvidenceSide::Support,
        "",
        None
    )
    .is_err());
}

fn input(thesis_id: Option<String>) -> DecisionInput {
    DecisionInput {
        instrument_id: "US:AAPL".into(),
        rationale: "Product growth".into(),
        valuation_context: "20x earnings".into(),
        time_horizon: "3 years".into(),
        review_date: day(),
        invalidation_criteria: "Revenue declines".into(),
        thesis_id,
    }
}

#[test]
fn decision_review_history_and_due_queue_survive_updates() {
    let mut db = Db::open_memory().unwrap();
    let thesis = create_thesis(
        &db,
        "Growth continues",
        "US",
        &["US:AAPL".into()],
        "3 years",
    )
    .unwrap();
    let decision = create_decision(&db, &input(Some(thesis.id))).unwrap();
    assert_eq!(
        decision.thesis_claim_snapshot.as_deref(),
        Some("Growth continues")
    );
    assert_eq!(due_reviews(&db, day()).unwrap().len(), 1);
    append_review(&mut db, &decision.id, "Still valid", Some("reviewed")).unwrap();
    assert_eq!(review_history(&db, &decision.id).unwrap().len(), 1);
    assert_eq!(get_decision(&db, &decision.id).unwrap().status, "reviewed");
    append_review(&mut db, &decision.id, "Closed", Some("retired")).unwrap();
    assert!(due_reviews(&db, day()).unwrap().is_empty());
    assert_eq!(list_decisions(&db, None, false).unwrap().len(), 0);
    delete_decision(&mut db, &decision.id).unwrap();
    assert!(get_decision(&db, &decision.id).is_err());
}

#[test]
fn thesis_rename_moves_links_without_rewriting_decision_history() {
    let mut db = Db::open_memory().unwrap();
    db.store_items(&[StoreItem::News(NewsItem {
        id: "n1".into(),
        instrument_ids: vec!["US:AAPL".into()],
        published: day().and_hms_opt(0, 0, 0).unwrap(),
        title: "Launch".into(),
        url: "https://example.test".into(),
        body: None,
        source: "rss".into(),
    })])
    .unwrap();
    let thesis = create_thesis(&db, "Old claim", "US", &["US:AAPL".into()], "3 years").unwrap();
    add_thesis_evidence(
        &db,
        &thesis.id,
        "news:n1",
        EvidenceSide::Support,
        "launch",
        Some(true),
    )
    .unwrap();
    let decision = create_decision(&db, &input(Some(thesis.id.clone()))).unwrap();
    let renamed = update_thesis(
        &mut db,
        &thesis.id,
        &ThesisEdit {
            claim: "New claim".into(),
            scope: "US".into(),
            assumptions: vec!["growth".into()],
            falsifiers: vec!["decline".into()],
            targets: vec!["US:AAPL".into()],
            time_horizon: "5 years".into(),
            status: "active".into(),
        },
    )
    .unwrap();
    assert_ne!(renamed.id, thesis.id);
    assert_eq!(thesis_evidence(&db, &renamed.id, true).unwrap().len(), 1);
    let linked = get_decision(&db, &decision.id).unwrap();
    assert_eq!(linked.thesis_id.as_deref(), Some(renamed.id.as_str()));
    assert_eq!(linked.thesis_claim_snapshot.as_deref(), Some("Old claim"));
}

#[tokio::test]
async fn thesis_proposals_filter_unknown_ids_and_remain_candidates() {
    let mut db = Db::open_memory().unwrap();
    db.store_items(&[StoreItem::News(NewsItem {
        id: "n1".into(),
        instrument_ids: vec!["US:AAPL".into()],
        published: day().and_hms_opt(0, 0, 0).unwrap(),
        title: "Launch".into(),
        url: "https://example.test".into(),
        body: None,
        source: "rss".into(),
    })])
    .unwrap();
    let thesis = create_thesis(&db, "Growth", "US", &["US:AAPL".into()], "3 years").unwrap();
    let cfg = delta_core::config::AppConfig {
        llm_model: "test/model".into(),
        ..Default::default()
    };
    let client = LlmClient::new(Arc::new(CandidateProvider));
    let proposed = propose_thesis_evidence(&mut db, &client, &cfg, &thesis.id, 100)
        .await
        .unwrap();
    assert_eq!(proposed.len(), 1);
    assert!(!proposed[0].accepted);
    assert!(thesis_evidence(&db, &thesis.id, true).unwrap().is_empty());
}
