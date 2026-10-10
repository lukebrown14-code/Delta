use std::path::PathBuf;
use std::sync::Arc;

use chrono::NaiveDate;
use delta_core::config::AppConfig;
use delta_core::db::{Db, StoreItem};
use delta_core::models::NewsItem;
use delta_llm::client::LlmClient;
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};
use delta_services::{
    build_report, cite, evidence, read_report, render_markdown, report_history, write_report,
};

struct DraftProvider {
    text: String,
}

#[async_trait::async_trait]
impl Provider for DraftProvider {
    fn name(&self) -> &'static str {
        "draft"
    }

    async fn complete(&self, _req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        Ok(ProviderResult {
            text: self.text.clone(),
            input_tokens: 10,
            output_tokens: 10,
            cost_usd: 0.0,
        })
    }
}

fn seeded_db() -> Db {
    let mut db = Db::open_memory().unwrap();
    db.store_items(&[StoreItem::News(NewsItem {
        id: "news-1".to_string(),
        instrument_ids: vec!["US:AAPL".to_string()],
        published: NaiveDate::from_ymd_opt(2026, 9, 20)
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap(),
        title: "Apple announces partnership".to_string(),
        url: "https://example.test/news".to_string(),
        body: Some("A signed partnership was announced.".to_string()),
        source: "rss".to_string(),
    })])
    .unwrap();
    db
}

fn client(text: &str) -> LlmClient {
    LlmClient::new(Arc::new(DraftProvider {
        text: text.to_string(),
    }))
}

fn cfg() -> AppConfig {
    AppConfig {
        llm_model: "test/model".to_string(),
        ..Default::default()
    }
}

#[tokio::test]
async fn report_only_keeps_gathered_citations_and_persists_history() {
    let mut db = seeded_db();
    let items = evidence(&db, Some("US:AAPL"), None, None, 200, None).unwrap();
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].id, "news:news-1");
    assert_eq!(
        cite(&items[0]),
        "[rss] Apple announces partnership <https://example.test/news>"
    );

    let draft = serde_json::json!({
        "summary": "A partnership was announced.",
        "bull": [
            {"text": "A deal was announced.", "evidence_ids": ["news:news-1"]},
            {"text": "Unsupported claim.", "evidence_ids": ["news:ghost"]}
        ],
        "sentiment": 0.2
    });
    let report = build_report(&mut db, &client(&draft.to_string()), &cfg(), "US:AAPL")
        .await
        .unwrap();
    assert_eq!(report.draft.bull.len(), 1);
    assert!(render_markdown(&report, false).contains("[rss] Apple announces partnership"));
    let temp = tempfile::tempdir().unwrap();
    let path = write_report(&report, temp.path()).unwrap();
    assert_eq!(
        read_report(&path.with_extension("json"))
            .unwrap()
            .draft
            .bull
            .len(),
        1
    );
    write_report(&report, temp.path()).unwrap();
    assert_eq!(report_history(temp.path(), "US:AAPL").len(), 2);
}

#[tokio::test]
async fn report_rejects_a_draft_without_supported_claims() {
    let mut db = seeded_db();
    let draft = serde_json::json!({
        "summary": "Guess.",
        "bull": [{"text": "Unsupported", "evidence_ids": ["news:ghost"]}],
        "sentiment": 0.0
    });
    let err = build_report(&mut db, &client(&draft.to_string()), &cfg(), "US:AAPL")
        .await
        .err()
        .unwrap();
    assert!(err.to_string().contains("no claims supported"));
}

#[test]
fn legacy_report_dates_normalize_to_utc_and_history_sorts_instants() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("US:AAPL");
    std::fs::create_dir(&dir).unwrap();
    for (name, ts) in [
        ("a", "2026-09-21T12:00:00"),
        ("b", "2026-09-21T13:00:00+02:00"),
    ] {
        let report = serde_json::json!({"target_id":"US:AAPL","as_of":ts,"prompt_version":"report_v2","citations":{"news:news-1":"source"},"summary":"grounded","bull":[{"text":"Deal","evidence_ids":["news:news-1"]}],"sentiment":0});
        std::fs::write(dir.join(format!("{name}.json")), report.to_string()).unwrap();
    }
    let history = report_history(temp.path(), "US:AAPL");
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].as_of, "2026-09-21T12:00:00+00:00");
    assert_eq!(history[1].as_of, "2026-09-21T11:00:00+00:00");
}

struct PromptProvider;
#[async_trait::async_trait]
impl Provider for PromptProvider {
    fn name(&self) -> &'static str {
        "prompt-check"
    }
    async fn complete(&self, req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        assert_eq!(
            req.response_format.unwrap()["json_schema"]["name"],
            "ReportDraft"
        );
        let prompt = req
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(prompt.contains("Apple Incorporated"), "{prompt}");
        assert!(
            prompt.contains("2026-09-20T12:00:00.123456+00:00"),
            "{prompt}"
        );
        Ok(ProviderResult { text: serde_json::json!({"summary":"Grounded", "bull":[{"text":"Deal", "evidence_ids":["news:news-1"]}],"sentiment":0}).to_string(), input_tokens:1, output_tokens:1, cost_usd:0.0 })
    }
}

#[tokio::test]
async fn report_prompt_preserves_target_label_timestamp_and_schema() {
    let mut db = seeded_db();
    db.conn()
        .execute(
            "UPDATE newsitem SET published = '2026-09-20 12:00:00.123456'",
            [],
        )
        .unwrap();
    let mut config = cfg();
    config.targets.insert(
        "US:AAPL".into(),
        serde_json::json!({"kind":"company","label":"Apple Incorporated"}),
    );
    build_report(
        &mut db,
        &LlmClient::new(Arc::new(PromptProvider)),
        &config,
        "US:AAPL",
    )
    .await
    .unwrap();
}

#[test]
fn python_report_fixture_round_trips_byte_for_byte() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture = root.join("fixtures/golden_reports/US:AAPL/2026-09-21.json");
    let expected_sidecar = std::fs::read_to_string(&fixture).unwrap();
    let expected_markdown = std::fs::read_to_string(fixture.with_extension("md")).unwrap();
    let report = read_report(&fixture).expect("Python sidecar opens in Rust");
    assert_eq!(render_markdown(&report, false), expected_markdown);

    let temp = tempfile::tempdir().unwrap();
    let path = write_report(&report, temp.path()).unwrap();
    let actual_sidecar = std::fs::read_to_string(path.with_extension("json")).unwrap();
    assert_eq!(actual_sidecar, expected_sidecar);
}
