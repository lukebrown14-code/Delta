//! Ported from `tests/test_llm_client.py`: caching, cost logging, and the
//! poisoned-cache validator — against a fake provider, fully offline.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use delta_core::db::Db;
use delta_llm::client::{build_client, CompleteParams, LlmClient};
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};

fn params(prompt: &str) -> CompleteParams<'_> {
    CompleteParams {
        task: "analyse",
        model: "m",
        prompt_version: "v1",
        prompt: Some(prompt),
        messages: None,
        response_format: None,
        cache_validator: None,
    }
}

struct FakeProvider {
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Provider for FakeProvider {
    fn name(&self) -> &'static str {
        "fake"
    }

    async fn complete(&self, _req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(ProviderResult {
            text: "hello".to_string(),
            input_tokens: 10,
            output_tokens: 5,
            cost_usd: 0.001,
        })
    }
}

#[tokio::test]
async fn client_logs_and_caches() {
    let provider = Arc::new(FakeProvider {
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());
    let mut db = Db::open_memory().unwrap();

    let r1 = client
        .complete(&mut db, params("same prompt"))
        .await
        .unwrap();
    assert!(!r1.cached);
    assert_eq!(r1.text, "hello");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);

    let r2 = client
        .complete(&mut db, params("same prompt"))
        .await
        .unwrap();
    assert!(r2.cached);
    assert_eq!(r2.cost_usd, 0.0);
    // cache hit, no second call
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cache_hits_are_logged_as_a_cached_row() {
    let provider = Arc::new(FakeProvider {
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());
    let mut db = Db::open_memory().unwrap();

    client
        .complete(&mut db, params("same prompt"))
        .await
        .unwrap();
    client
        .complete(&mut db, params("same prompt"))
        .await
        .unwrap();

    assert_eq!(db.table_count("llmcall").unwrap(), 2);
    let rows = db.llm_lookup_all().unwrap();
    assert!(!rows[0].cached);
    assert!(rows[1].cached);
    assert!(rows.iter().all(|r| r.task == "analyse"));
    assert_eq!(rows[1].input_tokens, 0);
    assert_eq!(rows[1].cost_usd, 0.0);
}

#[tokio::test]
async fn cache_validator_rejects_poisoned_response() {
    let provider = Arc::new(FakeProvider {
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());
    let mut db = Db::open_memory().unwrap();

    // "hello" is not JSON, so the validator rejects the cached row.
    let not_json = |text: &str| serde_json::from_str::<serde_json::Value>(text).is_ok();
    let with_validator = CompleteParams {
        cache_validator: Some(&not_json),
        ..params("same prompt")
    };

    client.complete(&mut db, with_validator).await.unwrap();
    let second = client.complete(&mut db, with_validator).await.unwrap();
    // The cached "hello" fails the validator, so it is a miss and re-called.
    assert!(!second.cached);
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn build_client_selects_and_rejects_providers() {
    let keys = |name: &str| -> String {
        match name {
            "OPENROUTER_API_KEY" | "OPENAI_API_KEY" | "ANTHROPIC_API_KEY" | "CUSTOM_API_KEY" => {
                "k".to_string()
            }
            "MY_KEY" => "mine".to_string(),
            _ => String::new(),
        }
    };
    let timeout = std::time::Duration::from_secs(60);
    let c = build_client("openrouter", &keys, timeout, None, "", "").unwrap();
    assert_eq!(c.provider_name(), "openrouter");
    let c = build_client("openai", &keys, timeout, None, "", "").unwrap();
    assert_eq!(c.provider_name(), "openai");
    let c = build_client("anthropic", &keys, timeout, None, "", "").unwrap();
    assert_eq!(c.provider_name(), "anthropic");
    let c = build_client(
        "custom",
        &keys,
        timeout,
        None,
        "http://localhost:11434/v1",
        "MY_KEY",
    )
    .unwrap();
    assert_eq!(c.provider_name(), "custom");

    let err = match build_client("nope", &keys, timeout, None, "", "") {
        Err(e) => e.to_string(),
        Ok(_) => panic!("unknown provider must fail loudly"),
    };
    assert!(err.contains("valid:"), "{err}");
}
