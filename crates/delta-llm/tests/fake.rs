//! FakeLlm tests (R3.0 item 6, deferred to R3.1c): the fixture contract
//! `fixtures/llm/<task>/<name>.json` = `{"cost_usd": float, "text": payload}`,
//! shared with the Python `load_llm_fixtures` / `FakeLLM` rig, and the cost
//! plumbing through a real [`LlmClient`].

use std::path::PathBuf;
use std::sync::Arc;

use delta_core::db::Db;
use delta_llm::client::{CompleteParams, LlmClient, LlmResult};
use delta_llm::fake::FakeLlm;
use delta_llm::providers::Message;

fn fixtures_root() -> PathBuf {
    // Tests run with the crate dir as CWD; the shared fixtures live at the
    // repository root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/llm")
}

#[test]
fn fixture_keys_mirror_the_python_loader() {
    let fake = FakeLlm::load(&fixtures_root()).unwrap();
    // `<task>/response.json` is addressable as `<task>`; other names as
    // `<task>/<name>` (parity with `load_llm_fixtures`).
    assert_eq!(fake.cost_of("chat"), Some(0.01));
    assert_eq!(fake.cost_of("report/apple"), Some(0.01));
    assert_eq!(fake.cost_of("report/microsoft"), Some(0.01));
    assert_eq!(fake.cost_of("report"), None, "no <task>/response.json here");
}

#[tokio::test]
async fn canned_payload_and_cost_flow_through_the_client() {
    let mut db = Db::open_memory().unwrap();
    let fake = Arc::new(FakeLlm::load(&fixtures_root()).unwrap());
    fake.serve("report/apple");
    let client = LlmClient::new(fake.clone());

    let params = CompleteParams {
        task: "report",
        model: "openrouter/anthropic/claude-sonnet-4.5",
        prompt_version: "report_v1",
        prompt: Some("write the apple report"),
        messages: None,
        response_format: None,
        cache_validator: None,
    };
    let LlmResult {
        text,
        cost_usd,
        cached,
        ..
    } = client.complete(&mut db, params).await.unwrap();

    // The payload is the fixture's `text`, serialised exactly as the Python
    // FakeLLM serialises it.
    let fixture_text = std::fs::read_to_string(fixtures_root().join("report/apple.json")).unwrap();
    let fixture: serde_json::Value = serde_json::from_str(&fixture_text).unwrap();
    let expected = serde_json::to_string(&fixture["text"]).unwrap();
    assert_eq!(text, expected);
    assert!(
        text.contains("chip cycle"),
        "payload carries the canned report"
    );

    // Cost plumbing: the fixture's cost_usd lands on the result...
    assert_eq!(cost_usd, 0.01);
    assert!(!cached);

    // ...and on the persisted llmcall row (normal cost logging, untouched).
    let phash = LlmClient::prompt_hash(
        "openrouter/anthropic/claude-sonnet-4.5",
        "report_v1",
        "write the apple report",
    );
    let row = db.llm_lookup(&phash).unwrap().expect("call is logged");
    assert_eq!(row.cost_usd, 0.01);
    assert!(!row.cached);
    assert_eq!(row.task, "report");
    assert_eq!(row.response.as_deref(), Some(expected.as_str()));
}

#[tokio::test]
async fn serve_switches_the_response_and_calls_are_recorded() {
    let mut db = Db::open_memory().unwrap();
    let fake = Arc::new(FakeLlm::load(&fixtures_root()).unwrap());
    let client = LlmClient::new(fake.clone());
    fake.serve("chat");

    // Distinct prompts per call: each gets its own cache key, so the served
    // fixture is what each result shows.
    let first = ask(&client, &mut db, "what changed this week?")
        .await
        .unwrap();
    assert!(first.text.contains("\"answer\""), "chat default fixture");
    assert_eq!(first.cost_usd, 0.01);
    assert!(!first.cached);

    fake.serve("report/microsoft");
    let second = ask(&client, &mut db, "and microsoft?").await.unwrap();
    assert!(second.text.contains("\"summary\""), "named fixture served");
    assert_eq!(second.cost_usd, 0.01);

    let calls = fake.calls();
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].model, "fake/model");
    assert_eq!(calls[0].messages.len(), 1);
    assert_eq!(calls[0].messages[0].role, "user");

    // An unknown key fails loudly instead of serving the wrong payload.
    fake.serve("report/tesla");
    let err = ask(&client, &mut db, "and tesla?").await.unwrap_err();
    assert!(err.to_string().contains("report/tesla"), "{err}");
}

async fn ask(client: &LlmClient, db: &mut Db, question: &str) -> Result<LlmResult, String> {
    let messages = [Message::new("user", question)];
    client
        .complete(
            db,
            CompleteParams {
                task: "chat",
                model: "fake/model",
                prompt_version: "chat_v1",
                prompt: None,
                messages: Some(&messages),
                response_format: None,
                cache_validator: None,
            },
        )
        .await
        .map_err(|e| e.to_string())
}
