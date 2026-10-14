//! Offline tests for the Jev Decisions client, mirroring `tests/test_jev.py`:
//! parsing, caching, cost logging, error mapping.

use delta_core::db::Db;
use delta_llm::jev::{JevClient, JevError, Usage, JEV_MODEL};
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Answers bull for upbeat titles, bear otherwise, per request state
/// (`_mock_by_title`).
struct ByTitle;

impl Respond for ByTitle {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let sent: serde_json::Value = request.body_json().expect("request body");
        let title = sent["state"]["title"].as_str().unwrap_or("");
        let choice = if title.contains("upgrades") {
            "bull"
        } else {
            "bear"
        };
        ResponseTemplate::new(200).set_body_json(body(
            serde_json::json!({"stance": {"type": "choice", "choice": choice}}),
            0.0001,
        ))
    }
}

fn question() -> delta_llm::jev::ChoiceQuestion {
    delta_llm::jev::ChoiceQuestion::new(
        "stance",
        "Bull or bear?",
        vec![
            ("bull".to_string(), "Good.".to_string()),
            ("bear".to_string(), "Bad.".to_string()),
            ("neutral".to_string(), "Neither.".to_string()),
        ],
    )
}

fn body(answers: serde_json::Value, cost: f64) -> serde_json::Value {
    serde_json::json!({
        "id": "gen-dec-1",
        "model": "typesafe/jev-1.13-20260917",
        "provider": "TypeSafe",
        "answers": answers,
        "usage": {"input_tokens": 100, "output_tokens": 10, "cost": cost}
    })
}

#[tokio::test]
async fn decide_parses_answers_and_logs_call() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .and(header("Authorization", "Bearer k"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(
            serde_json::json!({"stance": {"type": "choice", "choice": "bull", "confidence": 0.8}}),
            0.0005,
        )))
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    let client = JevClient::new("k").with_base_url(&server.uri());

    let decision = client
        .decide(
            &mut db,
            "sentiment",
            &serde_json::json!({"title": "x"}),
            &[question()],
        )
        .await
        .unwrap();

    assert_eq!(decision.model, "typesafe/jev-1.13-20260917");
    assert_eq!(decision.answers["stance"]["choice"], "bull");
    assert_eq!(
        decision.usage,
        Usage {
            input_tokens: 100,
            output_tokens: 10,
            cost_usd: 0.0005
        }
    );
    assert!(!decision.cached);

    let sent: serde_json::Value = server.received_requests().await.unwrap()[0]
        .body_json()
        .unwrap();
    assert_eq!(sent["model"], JEV_MODEL);
    assert_eq!(sent["questions"]["stance"]["criteria"]["bear"], "Bad.");

    let rows = db.llm_lookup_all().unwrap();
    assert_eq!(rows.len(), 1);
    let row = &rows[0];
    assert_eq!(row.task, "sentiment");
    assert_eq!(row.model, "typesafe/jev-1.13-20260917");
    assert_eq!(row.prompt_version, "jev_v1");
    assert_eq!(row.input_tokens, 100);
    assert!((row.cost_usd - 0.0005).abs() < 1e-12);
    assert!(!row.cached);
    let logged: serde_json::Value = serde_json::from_str(row.response.as_deref().unwrap()).unwrap();
    assert_eq!(logged["answers"]["stance"]["choice"], "bull");
}

#[tokio::test]
async fn decide_caches_identical_payload() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ByTitle)
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    let client = JevClient::new("k").with_base_url(&server.uri());
    let state = serde_json::json!({"title": "same"});

    let first = client
        .decide(&mut db, "sentiment", &state, &[question()])
        .await
        .unwrap();
    let second = client
        .decide(&mut db, "sentiment", &state, &[question()])
        .await
        .unwrap();

    assert_eq!(server.received_requests().await.unwrap().len(), 1);
    assert!(!first.cached);
    assert!(second.cached);
    assert_eq!(second.answers["stance"]["choice"], "bear");
    assert_eq!(second.usage.cost_usd, 0.0);
}

#[tokio::test]
async fn decide_raises_on_api_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ResponseTemplate::new(502).set_body_json(serde_json::json!({"error": {}})))
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    let client = JevClient::new("k").with_base_url(&server.uri());

    let err = client
        .decide(
            &mut db,
            "sentiment",
            &serde_json::json!({"title": "x"}),
            &[question()],
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("502"), "{err}");
    assert!(matches!(err, JevError::Api { status: 502, .. }));
}

#[tokio::test]
async fn decide_logs_cache_hits() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body(
            serde_json::json!({"stance": {"type": "choice", "choice": "bull"}}),
            0.0005,
        )))
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    let client = JevClient::new("k").with_base_url(&server.uri());
    let state = serde_json::json!({"title": "same"});

    client
        .decide(&mut db, "sentiment", &state, &[question()])
        .await
        .unwrap();
    client
        .decide(&mut db, "sentiment", &state, &[question()])
        .await
        .unwrap();

    let rows = db.llm_lookup_all().unwrap();
    assert_eq!(
        rows.iter().map(|r| r.cached).collect::<Vec<_>>(),
        vec![false, true]
    );
    assert_eq!(rows[1].task, "sentiment");
    assert_eq!(rows[1].input_tokens, 0);
    assert_eq!(rows[1].cost_usd, 0.0);
}

/// The payload hash must match the Python client's `stable_id` over
/// `json.dumps(..., sort_keys=True)` so caches interoperate.
#[test]
fn payload_hash_matches_python() {
    let state = serde_json::json!({
        "instrument_id": "US:AAPL",
        "symbol": "AAPL",
        "published": "2026-09-16T12:00:00+00:00",
        "source": "rss",
        "title": "Apple upgrades guidance",
        "body": "body text",
        "evidence_id": "n1"
    });
    let questions = serde_json::json!({
        "stance": {
            "type": "choice",
            "instructions": "Bull or bear?",
            "criteria": {"bear": "Bad.", "bull": "Good.", "neutral": "Neither."}
        }
    });
    // Computed with the Python client's `_payload_hash` for the same payload.
    let expected = "88ef6a5c5431bda724790d37ab30bbfdf0c89df4619f16637ba3c9f5a4b9b9f8";
    assert_eq!(
        delta_llm::jev::payload_hash("typesafe/jev-1.13", &state, &questions),
        expected
    );
}

/// Ordering of question criteria must not matter: the hash sorts keys.
#[test]
fn payload_hash_ignores_key_order() {
    let state = serde_json::json!({"a": "1", "b": "2"});
    let q1 = serde_json::json!({"x": {"type": "choice", "instructions": "i", "criteria": {"a": "1", "b": "2"}}});
    let q2 = serde_json::json!({"x": {"criteria": {"b": "2", "a": "1"}, "instructions": "i", "type": "choice"}});
    assert_eq!(
        delta_llm::jev::payload_hash("m", &state, &q1),
        delta_llm::jev::payload_hash("m", &state, &q2)
    );
}
