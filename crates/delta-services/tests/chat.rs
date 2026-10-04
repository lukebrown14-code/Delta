use std::sync::{Arc, Mutex};

use chrono::NaiveDate;
use delta_core::config::AppConfig;
use delta_core::db::{Db, StoreItem};
use delta_core::models::NewsItem;
use delta_llm::client::LlmClient;
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};
use delta_services::{chat, ChatMessage};

struct Replies(Mutex<Vec<String>>);

#[async_trait::async_trait]
impl Provider for Replies {
    fn name(&self) -> &'static str {
        "replies"
    }

    async fn complete(&self, _req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        let text = self.0.lock().unwrap().remove(0);
        Ok(ProviderResult {
            text,
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
        })
    }
}

fn seeded_db() -> Db {
    let mut db = Db::open_memory().unwrap();
    db.store_items(&[StoreItem::News(NewsItem {
        id: "n1".to_string(),
        instrument_ids: vec!["US:AAPL".to_string()],
        published: NaiveDate::from_ymd_opt(2026, 9, 20)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        title: "Apple reports earnings".to_string(),
        url: "https://example.test/n1".to_string(),
        body: None,
        source: "rss".to_string(),
    })])
    .unwrap();
    db
}

fn history() -> Vec<ChatMessage> {
    vec![ChatMessage {
        role: "user".to_string(),
        text: "What happened?".to_string(),
        citations: Vec::new(),
        source: "user".to_string(),
    }]
}

#[tokio::test]
async fn chat_keeps_only_gathered_citations() {
    let mut db = seeded_db();
    let client = LlmClient::new(Arc::new(Replies(Mutex::new(vec![serde_json::json!({
        "answer": "Apple reported earnings.",
        "citations": ["news:n1", "news:ghost"]
    })
    .to_string()]))));
    let cfg = AppConfig {
        llm_model: "test/model".to_string(),
        ..Default::default()
    };
    let answer = chat(
        &mut db,
        &client,
        &cfg,
        &history(),
        &["US:AAPL".to_string()],
        false,
        None,
    )
    .await
    .unwrap();
    assert_eq!(answer.citations, vec!["news:n1"]);
    assert_eq!(answer.source, "inference");
}

#[tokio::test]
async fn chat_without_support_is_labelled_inference() {
    let mut db = seeded_db();
    let client = LlmClient::new(Arc::new(Replies(Mutex::new(vec![
        r#"{"answer":"No source","citations":[]}"#.to_string(),
    ]))));
    let cfg = AppConfig {
        llm_model: "test/model".to_string(),
        ..Default::default()
    };
    let answer = chat(
        &mut db,
        &client,
        &cfg,
        &history(),
        &["US:AAPL".to_string()],
        false,
        None,
    )
    .await
    .unwrap();
    assert!(answer.text.contains("treat it as AI inference"));
    assert!(answer.citations.is_empty());
}
