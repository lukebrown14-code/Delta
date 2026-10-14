//! Offline tests for the model catalog and route writeback, mirroring the
//! non-TUI cases in `tests/test_catalog.py` (HTTP via wiremock).

use std::time::Duration;

use delta_core::config::load_toml;
use delta_llm::catalog::{
    cached_catalog, catalog, read_cache_entry, set_llm_model, set_llm_route, set_plugin_model,
    write_cache,
};
use delta_llm::providers::{ModelInfo, OpenRouterProvider, Provider};
use wiremock::matchers::{method, path as path_match};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn models_payload() -> serde_json::Value {
    serde_json::json!({
    "data": [
        {
            "id": "anthropic/claude-sonnet-4",
            "name": "Claude Sonnet 4",
            "context_length": 200000,
            "pricing": {"prompt": "0.000003", "completion": "0.000015"}
        },
        {
            "id": "openai/gpt-4o",
            "name": "GPT-4o",
            "context_length": 128000,
            "pricing": {"prompt": "0.0000025", "completion": "0.00001"}
        },
        {
            "id": "meta/llama-3.1-8b",
            "name": "Llama 3.1 8B",
            "context_length": null,
            "pricing": {}
        }
    ]
    })
}

fn sonnet() -> ModelInfo {
    ModelInfo {
        id: "anthropic/claude-sonnet-4".into(),
        name: "Claude Sonnet 4".into(),
        context_length: Some(200000),
        prompt_price: 3e-06,
        completion_price: 1.5e-05,
    }
}

fn gpt() -> ModelInfo {
    ModelInfo {
        id: "openai/gpt-4o".into(),
        name: "GPT-4o".into(),
        context_length: Some(128000),
        prompt_price: 2.5e-06,
        completion_price: 1e-05,
    }
}

fn llama() -> ModelInfo {
    ModelInfo {
        id: "meta/llama-3.1-8b".into(),
        name: "Llama 3.1 8B".into(),
        context_length: None,
        prompt_price: 0.0,
        completion_price: 0.0,
    }
}

fn provider(server: &MockServer) -> OpenRouterProvider {
    OpenRouterProvider::with_base_url("k", Duration::from_secs(5), None, &server.uri()).unwrap()
}

#[tokio::test]
async fn openrouter_models_builds_modelinfo() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_match("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(models_payload()))
        .mount(&server)
        .await;
    let p = provider(&server);

    let models = p.models(false).await;
    let expected = vec![sonnet(), gpt(), llama()];
    assert_eq!(models.len(), expected.len());
    for (m, e) in models.iter().zip(&expected) {
        assert_eq!(m.id, e.id);
        assert_eq!(m.name, e.name);
        assert_eq!(m.context_length, e.context_length);
        assert!((m.prompt_price - e.prompt_price).abs() < 1e-15);
        assert!((m.completion_price - e.completion_price).abs() < 1e-15);
    }

    let again = p.models(false).await; // served from the pricing cache
    assert_eq!(again.len(), models.len());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[tokio::test]
async fn openrouter_failed_fetch_yields_empty_and_does_not_raise() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_match("/models"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let p = provider(&server);

    assert!(p.models(false).await.is_empty());
    assert!(
        catalog(&p, &dir.path().join("data/model_catalog.json"), false)
            .await
            .is_empty()
    );
}

#[tokio::test]
async fn catalog_populates_disk_cache_and_reuses_it() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_match("/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(models_payload()))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data/model_catalog.json");
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();

    let models = catalog(&provider(&server), &path, false).await;
    assert_eq!(models.len(), 3);

    let raw: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let keys: Vec<&String> = raw.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["openrouter"]);
    let entry = &raw["openrouter"];
    let fetched_at = entry["fetched_at"].as_f64().unwrap();
    assert!(before <= fetched_at && fetched_at <= before + 10.0);
    let ids: Vec<&str> = entry["models"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["id"].as_str().unwrap())
        .collect();
    let want: Vec<&str> = models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, want);

    // A fresh provider (empty in-memory pricing) is served from the disk cache.
    let fresh = catalog(&provider(&server), &path, false).await;
    assert_eq!(fresh.len(), models.len());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

#[test]
fn disk_cache_round_trips_with_fetched_at_stamp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data/model_catalog.json");
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs_f64();
    let models = vec![sonnet(), gpt()];

    write_cache(&path, "openrouter", &models);

    let (fetched_at, loaded) = read_cache_entry(&path, "openrouter").unwrap();
    assert!(before <= fetched_at && fetched_at <= before + 10.0);
    assert_eq!(loaded.len(), models.len());
    assert_eq!(loaded[0].id, models[0].id);
    assert_eq!(loaded[1].id, models[1].id);
    assert_eq!(cached_catalog(&path, "openrouter").len(), 2);
    assert!(cached_catalog(&path, "openai").is_empty());
    assert!(read_cache_entry(&path, "openai").is_none());
}

#[tokio::test]
async fn failed_fetch_keeps_previous_catalog() {
    // A transient error must not wipe a good cache for the whole TTL.
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("data/model_catalog.json");
    write_cache(&path, "openrouter", &[sonnet(), gpt()]);
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path_match("/models"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let p = provider(&server);

    let forced = catalog(&p, &path, true).await;
    assert_eq!(
        forced.iter().map(|m| m.id.clone()).collect::<Vec<_>>(),
        vec![sonnet().id, gpt().id]
    );
    assert_eq!(cached_catalog(&path, "openrouter").len(), 2);
}

#[test]
fn set_llm_route_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[llm.routing]\nextract = \"old/model\"\n").unwrap();

    set_llm_route(&path, "analyse", "anthropic/claude-sonnet-4").unwrap();

    let routing = &load_toml(&path).unwrap()["llm"]["routing"];
    assert_eq!(
        serde_json::json!({
            "analyse": "anthropic/claude-sonnet-4",
            "extract": "old/model",
        }),
        routing.clone()
    );
}

#[test]
fn set_llm_model_round_trips_and_leaves_routing_alone() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(
        &path,
        "[llm]\nprovider = \"openrouter\"\n\n[llm.routing]\nextract = \"old/model\"\n",
    )
    .unwrap();

    set_llm_model(&path, "anthropic/claude-sonnet-4").unwrap();

    let llm = &load_toml(&path).unwrap()["llm"];
    assert_eq!(llm["model"], "anthropic/claude-sonnet-4");
    assert_eq!(llm["provider"], "openrouter");
    assert_eq!(llm["routing"]["extract"], "old/model");
}

#[test]
fn set_plugin_model_round_trips_and_none_removes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, "[plugins.sec_edgar]\nenabled = true\n").unwrap();

    set_plugin_model(&path, "yfinance", Some("openai/gpt-4o")).unwrap();
    let plugins = &load_toml(&path).unwrap()["plugins"];
    assert_eq!(plugins["yfinance"]["model"], "openai/gpt-4o");
    assert_eq!(plugins["sec_edgar"], serde_json::json!({"enabled": true}));

    set_plugin_model(&path, "yfinance", None).unwrap();
    let plugins = &load_toml(&path).unwrap()["plugins"];
    assert!(plugins["yfinance"].get("model").is_none());
}
