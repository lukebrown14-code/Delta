use std::sync::{Arc, Mutex};

use chrono::NaiveDate;
use delta_core::config::AppConfig;
use delta_core::db::{Db, StoreItem};
use delta_core::models::NewsItem;
use delta_llm::client::LlmClient;
use delta_llm::fake::FakeLlm;
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};
use delta_services::chat::{chat, ChatMessage, SearchTool, WebHit};

struct Replies(Mutex<Vec<String>>);

struct WebSearch(Mutex<Vec<String>>);

#[async_trait::async_trait]
impl SearchTool for WebSearch {
    async fn search(&self, query: &str) -> Vec<WebHit> {
        self.0.lock().unwrap().push(query.to_string());
        vec![WebHit {
            title: "Quarterly demand".to_string(),
            url: "https://example.test/solar".to_string(),
            snippet: "Demand increased".to_string(),
        }]
    }
}

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

#[tokio::test]
async fn web_search_is_opt_in_labelled_and_never_stored_as_evidence() {
    let mut db = seeded_db();
    let before = delta_services::evidence(&db, Some("US:AAPL"), None, None, 100, None)
        .unwrap()
        .len();
    let cfg = AppConfig {
        llm_model: "test/model".to_string(),
        ..Default::default()
    };
    let search = WebSearch(Mutex::new(Vec::new()));
    let client = LlmClient::new(Arc::new(Replies(Mutex::new(vec![
        r#"{"answer":"Checking","citations":["news:n1"],"web_queries":["solar demand"]}"#
            .to_string(),
        r#"{"answer":"Web says demand increased","citations":["https://example.test/solar"]}"#
            .to_string(),
    ]))));
    let answer = chat(
        &mut db,
        &client,
        &cfg,
        &history(),
        &["US:AAPL".to_string()],
        true,
        Some(&search),
    )
    .await
    .unwrap();
    assert_eq!(search.0.lock().unwrap().as_slice(), ["solar demand"]);
    assert_eq!(answer.source, "web");
    assert_eq!(answer.citations, ["https://example.test/solar"]);
    assert_eq!(
        delta_services::evidence(&db, Some("US:AAPL"), None, None, 100, None)
            .unwrap()
            .len(),
        before
    );
    assert!(
        delta_services::evidence_by_ids(&db, &["https://example.test/solar".to_string()])
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn disabled_web_never_calls_search_even_if_model_requests_it() {
    let mut db = seeded_db();
    let search = WebSearch(Mutex::new(Vec::new()));
    let client = LlmClient::new(Arc::new(Replies(Mutex::new(vec![
        r#"{"answer":"Grounded","citations":["news:n1"],"web_queries":["solar"]}"#.to_string(),
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
        Some(&search),
    )
    .await
    .unwrap();
    assert_eq!(answer.source, "stored");
    assert!(search.0.lock().unwrap().is_empty());
}

#[tokio::test]
async fn shared_fake_llm_script_keeps_only_ids_in_the_seeded_evidence_pool() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let temp = tempfile::tempdir().unwrap();
    let db_path = temp.path().join("seed.db");
    std::fs::copy(root.join("fixtures/golden_seed.db"), &db_path).unwrap();
    let mut db = Db::open(&db_path).unwrap();
    let fake = Arc::new(FakeLlm::load(&root.join("fixtures/llm")).unwrap());
    fake.serve("chat");
    let client = LlmClient::new(fake);
    let cfg = AppConfig {
        llm_model: "test-model".to_string(),
        ..Default::default()
    };
    let answer = chat(
        &mut db,
        &client,
        &cfg,
        &[ChatMessage {
            role: "user".into(),
            text: "What drove Apple's latest quarter?".into(),
            citations: vec![],
            source: "user".into(),
        }],
        &["US:AAPL".to_string()],
        false,
        None,
    )
    .await
    .unwrap();
    assert_eq!(answer.citations, ["news:news-aapl-chip"]);
    assert_eq!(answer.source, "inference");
    assert_eq!(answer.text, "A fresh chip cycle and a clean 10-q with double-digit services growth drove the quarter; supplier warnings are the watch item.");
}
