//! D8 (finding rust-services #5): `sentiment` outside [-1, 1] fails the whole
//! batch, exactly as pydantic's `Field(ge=-1, le=1)` does in
//! `delta/extract.py` — nothing from that batch is stored.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{Duration, NaiveDate};
use delta_core::db::{Db, StoreItem};
use delta_core::models::{AssetClass, Instrument, NewsItem};
use delta_llm::client::LlmClient;
use delta_llm::providers::{CompletionRequest, Provider, ProviderError, ProviderResult};
use delta_services::pipeline::extract_events;

fn inst(id: &str, symbol: &str) -> Instrument {
    Instrument {
        id: id.to_string(),
        market: "us".to_string(),
        symbol: symbol.to_string(),
        name: Some(symbol.to_string()),
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

struct ScriptedProvider {
    payloads: Mutex<Vec<String>>,
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
        let text = if queue.is_empty() {
            String::new()
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

fn news(id: &str, instrument: &str) -> StoreItem {
    StoreItem::News(NewsItem {
        id: id.to_string(),
        instrument_ids: vec![instrument.to_string()],
        published: since() + Duration::days(1),
        title: format!("{id} headline"),
        url: format!("https://x/{id}"),
        body: None,
        source: "rss".to_string(),
    })
}

#[tokio::test]
async fn out_of_range_sentiment_skips_the_whole_batch() {
    let mut db = Db::open_memory().unwrap();
    let aapl = inst("US:AAPL", "AAPL");
    db.store_items(&[news("n1", "US:AAPL"), news("n2", "US:AAPL")])
        .unwrap();

    // One batch, one event with an impossible sentiment: pydantic rejects the
    // EventBatch, so neither event lands and the loop moves on.
    let payload = r#"{"events": [
        {"kind": "earnings", "summary": "good quarter", "sentiment": 0.9, "evidence_ids": ["n1"]},
        {"kind": "guidance", "summary": "raised outlook", "sentiment": 5, "evidence_ids": ["n2"]}
    ]}"#;
    let provider = Arc::new(ScriptedProvider {
        payloads: Mutex::new(vec![payload.to_string()]),
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());

    let events = extract_events(
        &mut db,
        &client,
        "test/model",
        std::slice::from_ref(&aapl),
        since(),
        20,
    )
    .await
    .unwrap();

    assert!(events.is_empty(), "the batch with sentiment 5 is skipped");
    // structured() re-prompts once on the validation failure (Python parity)
    // before extract_events gives up on the batch.
    assert_eq!(provider.calls.load(Ordering::SeqCst), 2);
    assert!(db.events_all().unwrap().is_empty(), "nothing was stored");
}

#[tokio::test]
async fn boundary_sentiments_still_store() {
    let mut db = Db::open_memory().unwrap();
    let aapl = inst("US:AAPL", "AAPL");
    db.store_items(&[news("n1", "US:AAPL"), news("n2", "US:AAPL")])
        .unwrap();

    // -1.0 and 1.0 are inside the closed range (pydantic ge/le are inclusive).
    let payload = r#"{"events": [
        {"kind": "earnings", "summary": "disaster quarter", "sentiment": -1.0, "evidence_ids": ["n1"]},
        {"kind": "guidance", "summary": "raised outlook", "sentiment": 1.0, "evidence_ids": ["n2"]}
    ]}"#;
    let provider = Arc::new(ScriptedProvider {
        payloads: Mutex::new(vec![payload.to_string()]),
        calls: AtomicUsize::new(0),
    });
    let client = LlmClient::new(provider.clone());

    let events = extract_events(&mut db, &client, "test/model", &[aapl], since(), 20)
        .await
        .unwrap();
    assert_eq!(events.len(), 2);
    let sentiments: Vec<f64> = events.iter().map(|e| e.sentiment).collect();
    assert_eq!(sentiments, vec![-1.0, 1.0]);
}
