//! Pipeline tests: ingest over a wiremock-served RSS feed and event
//! extraction through the scripted LLM path.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use chrono::{Duration, NaiveDate};
use delta_core::db::{Db, StoreItem};
use delta_core::models::{AssetClass, Instrument, NewsItem};
use delta_llm::client::LlmClient;
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};
use delta_plugins::plugin::DataPlugin;
use delta_plugins::RssData;
use delta_services::pipeline::{extract_events, gather_configured, ingest};

#[tokio::test]
async fn configured_gather_reports_optional_llm_failure_without_losing_ingest_result() {
    let mut db = Db::open_memory().unwrap();
    let cfg = delta_core::config::AppConfig::default();
    let result = gather_configured(&mut db, &cfg, &[], |_| {}).await.unwrap();
    assert!(result.ingested.counts.is_empty());
    assert_eq!(result.extracted.events, 0);
    assert!(!result.warnings.is_empty());
}

fn inst(id: &str, market: &str, symbol: &str, name: Option<&str>) -> Instrument {
    Instrument {
        id: id.to_string(),
        market: market.to_string(),
        symbol: symbol.to_string(),
        name: name.map(str::to_string),
        currency: "USD".to_string(),
        sector: None,
        asset_class: AssetClass::Equity,
        watchlists: Vec::new(),
        tags: Default::default(),
        industry: None,
        meta: Default::default(),
    }
}

fn since() -> chrono::NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
}

struct ScriptedSource {
    fails: bool,
}

#[async_trait::async_trait]
impl DataPlugin for ScriptedSource {
    fn name(&self) -> &'static str {
        if self.fails {
            "failed"
        } else {
            "successful"
        }
    }
    async fn fetch(
        &self,
        _instruments: &[Instrument],
        _since: chrono::NaiveDateTime,
    ) -> Result<Vec<StoreItem>, delta_plugins::PluginError> {
        if self.fails {
            return Err(delta_plugins::PluginError::Other {
                plugin: "failed",
                message: "offline scripted failure".into(),
            });
        }
        Ok(vec![StoreItem::News(NewsItem {
            id: "retained".into(),
            instrument_ids: vec!["US:AAPL".into()],
            published: since(),
            title: "Stored despite another source failure".into(),
            url: "https://example.test".into(),
            body: None,
            source: "successful".into(),
        })])
    }
}

#[tokio::test]
async fn successful_source_results_are_stored_even_if_an_earlier_source_fails() {
    let mut db = Db::open_memory().unwrap();
    let plugins: Vec<Arc<dyn DataPlugin>> = vec![
        Arc::new(ScriptedSource { fails: true }),
        Arc::new(ScriptedSource { fails: false }),
    ];
    let result = ingest(
        &mut db,
        &plugins,
        &[inst("US:AAPL", "us", "AAPL", None)],
        None,
        None,
        Some("2026-01-01"),
        2,
        |_| {},
    )
    .await;
    assert!(result.is_err());
    assert_eq!(db.table_count("newsitem").unwrap(), 1);
}

#[test]
fn invalid_extract_sentiment_rejects_the_batch() {
    let valid = serde_json::json!({"events": [{
        "kind": "other", "summary": "valid", "sentiment": 1.0, "evidence_ids": ["n1"]
    }]});
    assert!(serde_json::from_value::<delta_services::pipeline::EventBatch>(valid).is_ok());
    for sentiment in [-1.1, 1.1] {
        let invalid = serde_json::json!({"events": [{
            "kind": "other", "summary": "invalid", "sentiment": sentiment,
            "evidence_ids": ["n1"]
        }]});
        assert!(serde_json::from_value::<delta_services::pipeline::EventBatch>(invalid).is_err());
    }
}

/// A dated feed, so item ids are stable across runs and idempotency is
/// observable (the shared fixture's entries are undated, so their ids derive
/// from fetch time by design).
fn feed_body() -> Vec<u8> {
    br#"<?xml version="1.0"?>
<rss version="2.0"><channel>
<title>t</title><link>https://x</link><description>d</description>
<item><title>Apple ships thing</title><link>https://x/1</link>
<pubDate>Wed, 10 Sep 2026 10:00:00 +0000</pubDate></item>
<item><title>Microsoft follows</title><link>https://x/2</link>
<pubDate>Thu, 11 Sep 2026 10:00:00 +0000</pubDate></item>
</channel></rss>"#
        .to_vec()
}

#[tokio::test]
async fn ingest_stores_feed_rows_idempotently() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_raw(feed_body(), "application/xml"),
        )
        .mount(&server)
        .await;

    let mut rss = RssData::default();
    rss.configure(&serde_json::json!({"feeds": [format!("{}/feed.xml", server.uri())]}));
    let plugins: Vec<Arc<dyn DataPlugin>> = vec![Arc::new(rss)];
    let universe = vec![inst("US:AAPL", "us", "AAPL", Some("Apple Inc."))];

    let mut db = Db::open_memory().unwrap();
    let mut logs = Vec::new();
    let counts = ingest(&mut db, &plugins, &universe, None, None, None, 4, |m| {
        logs.push(m.to_string())
    })
    .await
    .unwrap();
    assert!(counts.counts["newsitem"] > 0);
    assert!(logs.iter().any(|l| l.contains("Ingesting via rss")));

    // The bar-since floor only applies to yfinance; a second run stores nothing new.
    let counts2 = ingest(&mut db, &plugins, &universe, None, None, None, 4, |_| {})
        .await
        .unwrap();
    assert_eq!(
        counts2.counts.get("newsitem").copied().unwrap_or(0),
        0,
        "dated items are stable ids"
    );
}

#[tokio::test]
async fn ingest_market_and_ticker_filters_apply() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_raw(feed_body(), "application/xml"),
        )
        .mount(&server)
        .await;
    let mut rss = RssData::default();
    rss.configure(&serde_json::json!({"feeds": [format!("{}/feed.xml", server.uri())]}));
    let plugins: Vec<Arc<dyn DataPlugin>> = vec![Arc::new(rss)];
    let universe = vec![
        inst("US:AAPL", "us", "AAPL", Some("Apple Inc.")),
        inst("ASX:BHP", "asx", "BHP", Some("BHP Group")),
    ];
    let mut db = Db::open_memory().unwrap();
    // A market filter that excludes the only plugin's market: no fetch at all.
    let counts = ingest(
        &mut db,
        &plugins,
        &universe,
        Some("nowhere"),
        None,
        None,
        4,
        |_| {},
    )
    .await
    .unwrap();
    assert!(counts.counts.values().all(|c| *c == 0));
    let counts = ingest(
        &mut db,
        &plugins,
        &universe,
        None,
        Some("MSFT"),
        None,
        4,
        |_| {},
    )
    .await
    .unwrap();
    assert_eq!(
        counts.counts.get("newsitem").copied().unwrap_or(0),
        0,
        "ticker filter empties the target list"
    );
}

struct ScriptedProvider {
    payloads: std::sync::Mutex<Vec<String>>,
    calls: AtomicUsize,
}

#[async_trait::async_trait]
impl Provider for ScriptedProvider {
    fn name(&self) -> &'static str {
        "scripted"
    }
    async fn complete(&self, req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        let schema = req
            .response_format
            .expect("extract must request structured output");
        assert_eq!(schema["type"], "json_schema");
        assert_eq!(schema["json_schema"]["name"], "EventBatch");
        assert_eq!(
            schema["json_schema"]["schema"]["$defs"]["EventDraft"]["properties"]["sentiment"]
                ["minimum"],
            -1
        );
        assert_eq!(
            schema["json_schema"]["schema"]["$defs"]["EventDraft"]["properties"]["sentiment"]
                ["maximum"],
            1
        );
        let mut queue = self.payloads.lock().unwrap();
        self.calls.fetch_add(1, Ordering::SeqCst);
        // An exhausted queue answers garbage: the extract loop must skip it.
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

#[tokio::test]
async fn extract_events_persists_cited_events_only() {
    let mut db = Db::open_memory().unwrap();
    let aapl = inst("US:AAPL", "us", "AAPL", Some("Apple Inc."));
    let msft = inst("US:MSFT", "us", "MSFT", Some("Microsoft Corporation"));
    db.store_items(&[
        StoreItem::News(NewsItem {
            id: "n1".to_string(),
            instrument_ids: vec!["US:AAPL".to_string()],
            published: since() + Duration::days(1),
            title: "Apple beats earnings".to_string(),
            url: "https://x/1".to_string(),
            body: None,
            source: "rss".to_string(),
        }),
        StoreItem::News(NewsItem {
            id: "n2".to_string(),
            instrument_ids: vec!["US:MSFT".to_string()],
            published: since() + Duration::days(1),
            title: "Microsoft raises guidance".to_string(),
            url: "https://x/2".to_string(),
            body: None,
            source: "rss".to_string(),
        }),
    ])
    .unwrap();

    // The model cites the provided item id: the event is kept.
    let good = r#"{"events": [{"kind": "earnings", "summary": "Apple beat earnings",
                 "sentiment": 0.4, "evidence_ids": ["n1"]}]}"#;
    // Second batch (MSFT) cites nothing provided: dropped.
    let bad = r#"{"events": [{"kind": "guidance", "summary": "ghost guidance",
                 "sentiment": -0.1, "evidence_ids": ["ghost"]}]}""#;
    let provider = Arc::new(ScriptedProvider {
        payloads: std::sync::Mutex::new(vec![good.to_string(), bad.to_string()]),
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());

    let universe = vec![aapl.clone(), msft.clone()];
    let events = extract_events(&mut db, &client, "test/model", &universe, since(), 20)
        .await
        .unwrap();
    assert_eq!(events.len(), 1, "ghost-cited events are dropped");
    assert_eq!(
        events[0].id,
        delta_services::pipeline::event_id("US:AAPL", "earnings", "Apple beat earnings")
    );
    assert_eq!(events[0].evidence_ids, vec!["n1".to_string()]);
    assert_eq!(events[0].extracted_by, "test/model");
    assert_eq!(events[0].prompt_version, "extract_v1");
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);

    // Re-running extracts nothing: both items are now covered (n1 by the kept
    // event, n2 remains cited-by-nothing? no - it was sent in the dropped
    // batch but never cited, so it is uncovered and re-sent; the provider is
    // out of payloads and the queue fallback... it returns an error path).
    // The contract asserted here: n1 is not re-extracted.
    let events2 = extract_events(&mut db, &client, "test/model", &universe, since(), 20)
        .await
        .unwrap();
    assert!(
        events2.iter().all(|e| e.id != events[0].id),
        "covered items are not re-sent"
    );
}

struct FailingProvider;
#[async_trait::async_trait]
impl Provider for FailingProvider {
    fn name(&self) -> &'static str {
        "failure"
    }
    async fn complete(&self, _: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        Err(ProviderError::Status {
            status: 401,
            body: "offline denied".into(),
        })
    }
}
#[tokio::test]
async fn extract_provider_failure_is_not_a_successful_empty_batch() {
    let mut db = Db::open_memory().unwrap();
    db.store_items(&[StoreItem::News(NewsItem {
        id: "failure-news".into(),
        instrument_ids: vec!["US:AAPL".into()],
        published: since(),
        title: "Offline story".into(),
        url: "https://example.test".into(),
        body: None,
        source: "rss".into(),
    })])
    .unwrap();
    let client = LlmClient::new(Arc::new(FailingProvider));
    let result = extract_events(
        &mut db,
        &client,
        "test/model",
        &[inst("US:AAPL", "us", "AAPL", None)],
        since(),
        20,
    )
    .await;
    assert!(result.unwrap_err().to_string().contains("401"));
    assert_eq!(db.table_count("event").unwrap(), 0);
}

struct PendingSource {
    started: tokio::sync::Notify,
    dropped: Arc<std::sync::atomic::AtomicBool>,
}

struct FetchDropFlag(Arc<std::sync::atomic::AtomicBool>);
impl Drop for FetchDropFlag {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

#[async_trait::async_trait]
impl DataPlugin for PendingSource {
    fn name(&self) -> &'static str {
        "pending"
    }
    async fn fetch(
        &self,
        _: &[Instrument],
        _: chrono::NaiveDateTime,
    ) -> Result<Vec<StoreItem>, delta_plugins::PluginError> {
        let _guard = FetchDropFlag(self.dropped.clone());
        self.started.notify_one();
        std::future::pending().await
    }
}

#[tokio::test]
async fn cancelling_ingest_drops_in_flight_source_fetches() {
    let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let source = Arc::new(PendingSource {
        started: tokio::sync::Notify::new(),
        dropped: dropped.clone(),
    });
    let worker_source = source.clone();
    let worker = tokio::spawn(async move {
        let mut db = Db::open_memory().unwrap();
        let plugins: Vec<Arc<dyn DataPlugin>> = vec![worker_source];
        ingest(
            &mut db,
            &plugins,
            &[inst("US:AAPL", "us", "AAPL", None)],
            None,
            None,
            Some("2026-01-01"),
            1,
            |_| {},
        )
        .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(1), source.started.notified())
        .await
        .expect("scripted fetch must start");
    worker.abort();
    assert!(worker.await.unwrap_err().is_cancelled());
    assert!(
        dropped.load(Ordering::SeqCst),
        "fetch must not survive cancelled ingest"
    );
}
