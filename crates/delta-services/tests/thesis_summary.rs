use chrono::Utc;
use delta_core::{
    config::AppConfig,
    db::{Db, StoreItem},
    models::NewsItem,
};
use delta_llm::{
    client::LlmClient,
    providers::{CompletionRequest, Provider, ProviderError, ProviderResult},
};
use delta_services::{
    add_thesis_evidence, create_thesis, summarize_thesis, EvidenceSide, HealthState,
};
use std::sync::Arc;

struct SummaryProvider;
#[async_trait::async_trait]
impl Provider for SummaryProvider {
    fn name(&self) -> &'static str {
        "summary"
    }
    async fn complete(&self, _req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        Ok(ProviderResult { text:r#"{"summary":"Limited evidence [news:n1]","citations":["news:n1","news:invented","news:n1"]}"#.into(),input_tokens:1,output_tokens:1,cost_usd:0.0 })
    }
}

#[tokio::test]
async fn summary_uses_only_accepted_citations_and_computed_state() {
    let mut db = Db::open_memory().unwrap();
    let thesis = create_thesis(&db, "Growth", "", &[], "").unwrap();
    let client = LlmClient::new(Arc::new(SummaryProvider));
    let cfg = AppConfig {
        llm_model: "test/model".into(),
        ..Default::default()
    };
    let empty = summarize_thesis(&mut db, &client, &cfg, &thesis.id)
        .await
        .unwrap();
    assert_eq!(empty.state, HealthState::Emerging);
    let configured =
        delta_services::summarize_thesis_configured(&mut db, &AppConfig::default(), &thesis.id)
            .await
            .unwrap();
    assert_eq!(configured.state, HealthState::Emerging);
    let calls: i64 = db
        .conn()
        .query_row("SELECT COUNT(*) FROM llmcall", [], |row| row.get(0))
        .unwrap();
    assert_eq!(calls, 0);
    db.store_items(&[StoreItem::News(NewsItem {
        id: "n1".into(),
        instrument_ids: vec![],
        published: Utc::now().naive_utc(),
        title: "Launch".into(),
        url: "https://example.test".into(),
        body: None,
        source: "rss".into(),
    })])
    .unwrap();
    add_thesis_evidence(
        &db,
        &thesis.id,
        "news:n1",
        EvidenceSide::Support,
        "launch",
        Some(true),
    )
    .unwrap();
    let summary = summarize_thesis(&mut db, &client, &cfg, &thesis.id)
        .await
        .unwrap();
    assert_eq!(summary.citations, vec!["news:n1"]);
    assert_eq!(summary.state, HealthState::Emerging);
}
