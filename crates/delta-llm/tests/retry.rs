//! D7 retry tests (finding rust-llm "no retry policy"): 429/5xx/timeout
//! failures are retried up to `RETRY_MAX_ATTEMPTS` times with exponential
//! backoff + jitter, a `Retry-After` delay is honoured verbatim, and
//! non-retryable statuses (402 among them) fail on the first attempt.
//!
//! The sleeper is a seam: pauses are recorded, never slept, so these tests
//! run in milliseconds.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use delta_llm::providers::{
    retry_delay, OpenAiCompatProvider, OpenRouterProvider, Provider, ProviderError, ProviderSpec,
    RetrySleep, RETRY_BASE_DELAY, RETRY_JITTER, RETRY_MAX_ATTEMPTS, RETRY_MAX_DELAY,
};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const OPENAI: ProviderSpec = ProviderSpec {
    name: "openai",
    base_url: "https://api.openai.com/v1",
    env_var: "OPENAI_API_KEY",
    kind: "openai-compat",
    verify_path: "/models",
};

struct RecordingSleeper {
    sleeps: Mutex<Vec<Duration>>,
}

#[async_trait]
impl RetrySleep for RecordingSleeper {
    async fn sleep(&self, delay: Duration) {
        self.sleeps.lock().unwrap().push(delay);
    }
}

fn sleeper() -> Arc<RecordingSleeper> {
    Arc::new(RecordingSleeper {
        sleeps: Mutex::new(Vec::new()),
    })
}

fn chat_body(text: &str) -> serde_json::Value {
    serde_json::json!({
        "choices": [{"message": {"role": "assistant", "content": text}}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1}
    })
}

async fn provider_at(uri: &str) -> (OpenAiCompatProvider, Arc<RecordingSleeper>) {
    let mut provider =
        OpenAiCompatProvider::new(&OPENAI, "k", Duration::from_secs(5), None, uri).unwrap();
    let sleep = sleeper();
    provider.sleep = sleep.clone();
    (provider, sleep)
}

fn messages() -> Vec<delta_llm::providers::Message> {
    vec![delta_llm::providers::Message::new("user", "hello")]
}

#[tokio::test]
async fn throttled_then_success_succeeds_on_second_attempt() {
    let server = MockServer::start().await;
    // First request gets a 429 with Retry-After; the mock exhausts after one
    // hit, so the retry reaches the 200 mounted below it.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(429)
                .set_body_json(serde_json::json!({"error": "rate limited"}))
                .append_header("Retry-After", "2"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("recovered")))
        .mount(&server)
        .await;

    let (provider, sleep) = provider_at(&server.uri()).await;
    let result = provider
        .complete(delta_llm::providers::CompletionRequest {
            model: "m1",
            messages: &messages(),
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap();

    assert_eq!(result.text, "recovered");
    assert_eq!(server.received_requests().await.unwrap().len(), 2);
    // The server's Retry-After is honoured exactly, no jitter added.
    assert_eq!(*sleep.sleeps.lock().unwrap(), vec![Duration::from_secs(2)]);
}

#[tokio::test]
async fn five_server_errors_exhaust_the_attempts() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(500).set_body_json(serde_json::json!({"oops": 1})))
        .mount(&server)
        .await;

    let (provider, sleep) = provider_at(&server.uri()).await;
    let err = provider
        .complete(delta_llm::providers::CompletionRequest {
            model: "m1",
            messages: &messages(),
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap_err();

    assert!(err.is_status(500), "unexpected error: {err}");
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        RETRY_MAX_ATTEMPTS as usize
    );
    let sleeps = sleep.sleeps.lock().unwrap();
    assert_eq!(sleeps.len(), (RETRY_MAX_ATTEMPTS - 1) as usize);
    // Exponential backoff, each delay strictly larger than the last (jitter
    // is bounded well below one doubling step).
    for pair in sleeps.windows(2) {
        assert!(pair[0] < pair[1]);
    }
    assert!(sleeps[0] >= RETRY_BASE_DELAY && sleeps[0] <= RETRY_BASE_DELAY + RETRY_JITTER);
}

#[tokio::test]
async fn timeouts_are_retried() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_delay(Duration::from_secs(3)))
        .mount(&server)
        .await;

    let mut provider =
        OpenAiCompatProvider::new(&OPENAI, "k", Duration::from_millis(50), None, &server.uri())
            .unwrap();
    let sleep = sleeper();
    provider.sleep = sleep.clone();

    let err = provider
        .complete(delta_llm::providers::CompletionRequest {
            model: "m1",
            messages: &messages(),
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap_err();

    assert!(
        matches!(err, ProviderError::Http(ref e) if e.is_timeout()),
        "unexpected error: {err}"
    );
    assert_eq!(
        server.received_requests().await.unwrap().len(),
        RETRY_MAX_ATTEMPTS as usize
    );
    assert_eq!(
        sleep.sleeps.lock().unwrap().len(),
        (RETRY_MAX_ATTEMPTS - 1) as usize
    );
}

#[tokio::test]
async fn client_errors_fail_on_the_first_attempt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({"e": "bad key"})))
        .mount(&server)
        .await;

    let (provider, sleep) = provider_at(&server.uri()).await;
    let err = provider
        .complete(delta_llm::providers::CompletionRequest {
            model: "m1",
            messages: &messages(),
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap_err();

    assert!(err.is_status(401));
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(sleep.sleeps.lock().unwrap().is_empty());
}

#[tokio::test]
async fn openrouter_402_refit_path_is_unchanged_by_the_retry_loop() {
    let server = MockServer::start().await;
    // Auto-fit with credits + pricing; the balance drops between the two
    // /credits reads so the forced refit computes a strictly lower cap and
    // re-POSTs — exactly as before D7.
    Mock::given(method("GET"))
        .and(path("/credits"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "data": {"total_credits": 5.0, "total_usage": 0.0}
                }))
                .append_header("x-order", "1"),
        )
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/credits"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": {"total_credits": 0.0, "total_usage": -0.002}
        })))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [{
                "id": "vendor/m",
                "pricing": {"prompt": "0.000001", "completion": "0.000002"}
            }]
        })))
        .mount(&server)
        .await;
    // First POST is the 402; the refit POST succeeds.
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(402).set_body_json(serde_json::json!({"e": "402"})))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_body("refit ok")))
        .mount(&server)
        .await;

    let provider =
        OpenRouterProvider::with_base_url("k", Duration::from_secs(5), None, &server.uri())
            .unwrap();
    let result = provider
        .complete(delta_llm::providers::CompletionRequest {
            model: "vendor/m",
            messages: &messages(),
            response_format: None,
            max_tokens: None,
        })
        .await
        .unwrap();

    assert_eq!(result.text, "refit ok");
    // 2 credits reads + 1 pricing fetch + exactly 2 POSTs: the retry loop
    // passes the 402 straight through (never re-POSTs at the same cap).
    let requests = server.received_requests().await.unwrap();
    assert_eq!(requests.len(), 5);
    assert_eq!(
        requests
            .iter()
            .filter(|r| r.url.path() == "/chat/completions")
            .count(),
        2
    );
}

#[test]
fn retry_delay_is_exponential_with_bounded_jitter() {
    let d = |attempt, after| retry_delay(attempt, after);
    // Backoff doubles per attempt and caps at RETRY_MAX_DELAY; jitter is
    // bounded by RETRY_JITTER (added only on the backoff path).
    for attempt in 1..=6u32 {
        let delay = d(attempt, None);
        let base = (RETRY_BASE_DELAY * 2u32.pow(attempt - 1)).min(RETRY_MAX_DELAY);
        assert!(
            delay >= base && delay <= base + RETRY_JITTER,
            "{attempt}: {delay:?}"
        );
    }
    assert!(d(10, None) >= RETRY_MAX_DELAY);
    // Retry-After wins verbatim over the computed backoff.
    assert_eq!(d(3, Some(7.5)), Duration::from_secs_f64(7.5));
    // A nonsensical Retry-After falls back to backoff.
    assert!(d(1, Some(-1.0)) >= RETRY_BASE_DELAY);
    assert!(d(1, Some(f64::NAN)) >= RETRY_BASE_DELAY);
}
