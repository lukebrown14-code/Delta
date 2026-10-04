//! Offline provider tests over `wiremock`, mirroring the scenarios in
//! `tests/test_openrouter_provider.py` / `tests/test_provider_setup.py`:
//! request/response parsing, auth headers, verify_key, and catalog pricing.

use std::collections::BTreeMap;
use std::time::Duration;

use delta_llm::providers::{
    auth_headers, verify_key, CompletionRequest, Message, OpenAiCompatProvider, OpenRouterProvider,
    Provider, ProviderSpec,
};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const ANTHROPIC: ProviderSpec = ProviderSpec {
    name: "anthropic",
    base_url: "https://api.anthropic.com/v1",
    env_var: "ANTHROPIC_API_KEY",
    kind: "openai-compat",
    verify_path: "/models",
};

const OPENAI: ProviderSpec = ProviderSpec {
    name: "openai",
    base_url: "https://api.openai.com/v1",
    env_var: "OPENAI_API_KEY",
    kind: "openai-compat",
    verify_path: "/models",
};

fn chat_body(text: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": text}}],
        "usage": {"prompt_tokens": 11, "completion_tokens": 7}
    })
}

#[tokio::test]
async fn compat_provider_sends_messages_and_parses_result() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("hi there")))
        .mount(&server)
        .await;

    let provider =
        OpenAiCompatProvider::new(&OPENAI, "k", Duration::from_secs(5), None, &server.uri())
            .unwrap();
    let messages = vec![Message::new("user", "hello")];
    let result = provider
        .complete(CompletionRequest {
            model: "m1",
            messages: &messages,
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap();

    assert_eq!(result.text, "hi there");
    assert_eq!(result.input_tokens, 11);
    assert_eq!(result.output_tokens, 7);
    assert_eq!(result.cost_usd, 0.0);
}

#[tokio::test]
async fn compat_provider_retries_a_transient_status() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "0"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("retried")))
        .mount(&server)
        .await;
    let provider =
        OpenAiCompatProvider::new(&OPENAI, "k", Duration::from_secs(5), None, &server.uri())
            .unwrap();
    let messages = vec![Message::new("user", "hello")];
    let result = provider
        .complete(CompletionRequest {
            model: "m1",
            messages: &messages,
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap();
    assert_eq!(result.text, "retried");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
}

#[tokio::test]
async fn anthropic_sends_native_auth_headers() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("x-api-key", "k"))
        .and(header("anthropic-version", "2023-06-01"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("ok")))
        .mount(&server)
        .await;

    let provider =
        OpenAiCompatProvider::new(&ANTHROPIC, "k", Duration::from_secs(5), None, &server.uri())
            .unwrap();
    let messages = vec![Message::new("user", "hello")];
    let result = provider
        .complete(CompletionRequest {
            model: "m1",
            messages: &messages,
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap();
    assert_eq!(result.text, "ok");
}

#[test]
fn auth_headers_match_python() {
    let headers: BTreeMap<String, String> = auth_headers(&OPENAI, "k");
    assert_eq!(headers.get("Authorization").unwrap(), "Bearer k");
    assert!(!headers.contains_key("x-api-key"));

    let headers = auth_headers(&ANTHROPIC, "k");
    assert_eq!(headers.get("x-api-key").unwrap(), "k");
    assert_eq!(headers.get("anthropic-version").unwrap(), "2023-06-01");

    assert!(auth_headers(&OPENAI, "").is_empty());
}

#[tokio::test]
async fn verify_key_never_errors_and_checks_status() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    assert!(verify_key(&OPENAI, "k", &server.uri()).await);
    assert!(!verify_key(&OPENAI, "", &server.uri()).await);
}

#[tokio::test]
async fn openrouter_models_carry_pricing() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {
                    "id": "vendor/model-a",
                    "name": "Model A",
                    "context_length": "128000",
                    "pricing": {"prompt": "0.0000015", "completion": "0.000002"}
                },
                {"id": "vendor/free", "name": "Free", "pricing": {"prompt": "0", "completion": "0"}}
            ]
        })))
        .mount(&server)
        .await;

    let provider =
        OpenRouterProvider::with_base_url("k", Duration::from_secs(5), None, &server.uri())
            .unwrap();
    let models = provider.models(true).await;
    assert_eq!(models.len(), 2);
    let a = &models[0];
    assert_eq!(a.id, "vendor/model-a");
    assert_eq!(a.context_length, Some(128_000));
    assert!((a.prompt_price - 0.0000015).abs() < 1e-12);
    assert!((a.completion_price - 0.000002).abs() < 1e-12);
}

#[tokio::test]
async fn openrouter_complete_normalizes_cost_from_pricing() {
    let server = MockServer::start().await;

    // Pricing catalog first (auto-fit reads /credits; failure degrades to the
    // configured cap, matching Python).
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id": "vendor/model-a",
                "pricing": {"prompt": "0.000001", "completion": "0.000002"}
            }]
        })))
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("answer")))
        .mount(&server)
        .await;

    let provider =
        OpenRouterProvider::with_base_url("k", Duration::from_secs(5), None, &server.uri())
            .unwrap();
    let messages = vec![Message::new("user", "hello")];
    let result = provider
        .complete(CompletionRequest {
            model: "vendor/model-a",
            messages: &messages,
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap();
    assert_eq!(result.text, "answer");
    // 11 * 0.000001 + 7 * 0.000002
    assert!((result.cost_usd - (11.0 * 0.000001 + 7.0 * 0.000002)).abs() < 1e-12);
}
