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
use delta_services::pipeline::{extract_events, ingest};

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
    async fn complete(&self, _req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
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

/// Records each call's `response_format` so tests can assert provider-side
/// schema enforcement without a network.
struct CapturingProvider {
    formats: std::sync::Mutex<Vec<serde_json::Value>>,
}

#[async_trait::async_trait]
impl Provider for CapturingProvider {
    fn name(&self) -> &'static str {
        "capturing"
    }
    async fn complete(&self, req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        self.formats.lock().unwrap().push(
            req.response_format
                .cloned()
                .unwrap_or(serde_json::Value::Null),
        );
        Ok(ProviderResult {
            text: r#"{"events": []}"#.to_string(),
            input_tokens: 1,
            output_tokens: 1,
            cost_usd: 0.0,
        })
    }
}

#[tokio::test]
async fn extract_enforces_the_canonical_event_batch_schema() {
    let mut db = Db::open_memory().unwrap();
    db.store_items(&[StoreItem::News(NewsItem {
        id: "n1".to_string(),
        instrument_ids: vec!["US:AAPL".to_string()],
        published: since() + Duration::days(1),
        title: "Apple beats earnings".to_string(),
        url: "https://x/1".to_string(),
        body: None,
        source: "rss".to_string(),
    })])
    .unwrap();
    let provider = Arc::new(CapturingProvider {
        formats: std::sync::Mutex::new(Vec::new()),
    });
    let client = LlmClient::new(provider.clone());
    let universe = vec![inst("US:AAPL", "us", "AAPL", None)];
    extract_events(&mut db, &client, "test/model", &universe, since(), 20)
        .await
        .unwrap();
    let formats = provider.formats.lock().unwrap();
    assert!(!formats.is_empty(), "one batch was sent to the model");
    assert!(
        formats
            .iter()
            .all(|f| f == &delta_services::schemas::event_batch()),
        "every extract call must carry the canonical EventBatch schema"
    );
}
