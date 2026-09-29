//! Ported from the structured-call contract in `delta/llm/structured.py`:
//! template rendering with strict undefined, cache-gating on schema validity,
//! and the single re-prompt on invalid JSON.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use delta_core::db::Db;
use delta_llm::client::LlmClient;
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};
use delta_llm::structured::{render_prompt, structured};
use serde::Deserialize;
use serde_json::json;

#[derive(Debug, Deserialize)]
struct Events {
    #[allow(dead_code)]
    events: Vec<serde_json::Value>,
}

struct ScriptedProvider {
    responses: std::sync::Mutex<Vec<String>>,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Provider for ScriptedProvider {
    fn name(&self) -> &'static str {
        "scripted"
    }

    async fn complete(&self, _req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        let mut queue = self.responses.lock().unwrap();
        self.calls.fetch_add(1, Ordering::SeqCst);
        let text = if queue.is_empty() {
            "garbage".to_string()
        } else {
            queue.remove(0)
        };
        Ok(ProviderResult {
            text,
            input_tokens: 1,
            output_tokens: 1,
            cost_usd: 0.0,
        })
    }
}

#[test]
fn render_prompt_strict_undefined_and_loops() {
    let vars = json!({
        "symbol": "AAPL",
        "instrument_id": "US:AAPL",
        "items": [{"id": "n1", "published": "2026-01-03", "source": "rss", "title": "t", "body": "b"}]
    });
    let prompt = render_prompt("extract_v1.j2", &vars).unwrap();
    assert!(prompt.contains("US:AAPL"), "{prompt}");
    assert!(prompt.contains("### Item n1"));

    // StrictUndefined: a missing variable is an error, not empty output.
    let err = render_prompt("extract_v1.j2", &json!({}))
        .unwrap_err()
        .to_string();
    assert!(!err.is_empty());
}

#[tokio::test]
async fn structured_retries_once_on_invalid_json() {
    let good = r#"```json
{"events": [{"kind": "earnings", "summary": "s", "sentiment": 0.1, "evidence_ids": ["n1"]}]}
"#;
    // First call returns garbage; the retry then gets the good payload.
    let provider = Arc::new(ScriptedProvider {
        responses: std::sync::Mutex::new(vec!["garbage".to_string(), good.to_string()]),
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());
    let mut db = Db::open_memory().unwrap();

    let vars = json!({
        "symbol": "AAPL",
        "instrument_id": "US:AAPL",
        "items": [{"id": "n1", "published": "2026-01-03", "source": "rss", "title": "t", "body": "b"}]
    });
    let (events, result) = structured::<Events>(
        &client,
        &mut db,
        "extract",
        "test/model",
        "extract_v1.j2",
        &vars,
        None,
    )
    .await
    .unwrap();
    assert_eq!(events.events.len(), 1);
    assert!(!result.cached);
    // First call returned garbage, so a second call was made with the error
    // appended and produced the good payload.
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn structured_cache_only_reuses_validating_responses() {
    let good = r#"{"events": []}"#;
    let provider = Arc::new(ScriptedProvider {
        responses: std::sync::Mutex::new(vec![good.to_string()]),
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());
    let mut db = Db::open_memory().unwrap();

    let vars = json!({
        "symbol": "AAPL",
        "instrument_id": "US:AAPL",
        "items": []
    });
    let first = structured::<Events>(
        &client,
        &mut db,
        "extract",
        "test/model",
        "extract_v1.j2",
        &vars,
        None,
    )
    .await
    .unwrap();
    assert!(!first.1.cached);

    // Same prompt again: the cached JSON validates, so it is a cache hit and
    // the provider is never called.
    let second = structured::<Events>(
        &client,
        &mut db,
        "extract",
        "test/model",
        "extract_v1.j2",
        &vars,
        None,
    )
    .await
    .unwrap();
    assert!(second.1.cached);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}
