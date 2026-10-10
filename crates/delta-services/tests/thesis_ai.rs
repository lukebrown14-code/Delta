use std::sync::Arc;

use chrono::{Duration, Utc};
use delta_core::config::AppConfig;
use delta_core::db::{Db, StoreItem};
use delta_core::models::NewsItem;
use delta_llm::client::LlmClient;
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};
use delta_services::{
    create_thesis, evidence_for, propose_evidence, set_accepted, summarize_thesis, thesis_fleet,
};

struct DraftProvider(&'static str);

#[async_trait::async_trait]
impl Provider for DraftProvider {
    fn name(&self) -> &'static str {
        "draft"
    }
    async fn complete(
        &self,
        _request: CompletionRequest<'_>,
    ) -> Result<ProviderResult, ProviderError> {
        Ok(ProviderResult {
            text: self.0.to_string(),
            input_tokens: 1,
            output_tokens: 1,
            cost_usd: 0.0,
        })
    }
}

fn client(text: &'static str) -> LlmClient {
    LlmClient::new(Arc::new(DraftProvider(text)))
}
fn cfg() -> AppConfig {
    AppConfig {
        llm_model: "test/model".to_string(),
        ..Default::default()
    }
}

fn seeded() -> (Db, String) {
    let mut db = Db::open_memory().unwrap();
    for i in 0..3 {
        db.store_items(&[StoreItem::News(NewsItem {
            id: format!("n{i}"),
            instrument_ids: vec!["US:AAPL".to_string()],
            published: Utc::now().naive_utc() - Duration::days(1),
            title: format!("Headline {i}"),
            url: format!("https://example.test/{i}"),
            body: None,
            source: "rss".to_string(),
        })])
        .unwrap();
    }
    let thesis = create_thesis(
        &db,
        "Apple grows",
        "",
        &[],
        &[],
        &["US:AAPL".to_string()],
        "5y",
        None,
    )
    .unwrap();
    (db, thesis.id)
}

#[tokio::test]
async fn proposal_keeps_only_fresh_gathered_ids_unaccepted_and_health_waits_for_user() {
    let (mut db, id) = seeded();
    let ai = client(
        r#"{"candidates":[{"evidence_id":"news:n0","side":"support","note":"growth"},{"evidence_id":"news:ghost","side":"against","note":"fake"},{"evidence_id":"news:n0","side":"against","note":"duplicate"}]}"#,
    );
    let proposed = propose_evidence(&mut db, &ai, &cfg(), &id, None, 100)
        .await
        .unwrap();
    assert_eq!(proposed.len(), 1);
    assert_eq!(proposed[0].evidence_id, "news:n0");
    assert!(!proposed[0].accepted);
    assert_eq!(thesis_fleet(&db, None).unwrap()[0].result.is_none(), true);
    set_accepted(&db, &id, "news:n0", true).unwrap();
    let health = thesis_fleet(&db, None).unwrap();
    assert_eq!(health[0].result.as_ref().unwrap().support, 1);
    assert_eq!(evidence_for(&db, &id, true).unwrap().len(), 1);
}

#[tokio::test]
async fn summary_cites_only_accepted_ids_and_empty_case_skips_model() {
    let (mut db, id) = seeded();
    let ai =
        client(r#"{"summary":"Growth [news:n0]","citations":["news:n0","news:ghost","news:n0"]}"#);
    let empty = summarize_thesis(&mut db, &ai, &cfg(), &id).await.unwrap();
    assert_eq!(empty.state.as_str(), "emerging");
    assert!(empty.summary.contains("No accepted evidence"));
    delta_services::add_evidence(&db, &id, "news:n0", "support", "growth", Some(true)).unwrap();
    let summary = summarize_thesis(&mut db, &ai, &cfg(), &id).await.unwrap();
    assert_eq!(summary.citations, vec!["news:n0"]);
    assert_eq!(summary.state.as_str(), "emerging");
}
