//! Offline tests for Jev news-stance classification and aggregation, mirroring
//! `tests/test_sentiment.py` (HTTP via wiremock).

use std::collections::BTreeMap;

use chrono::{Duration, NaiveDateTime};
use delta_core::config::AppConfig;
use delta_core::db::{Db, StoreItem};
use delta_core::models::{AssetClass, Instrument, NewsItem};
use delta_services::sentiment::{judgment_id, stance_of, stock_sentiment, HALF_LIFE_DAYS};
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

const NOW: &str = "2026-09-16 12:00:00";

fn now() -> NaiveDateTime {
    NaiveDateTime::parse_from_str(NOW, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn inst(id: &str, symbol: &str) -> Instrument {
    let market = id.split(':').next().unwrap_or("us");
    Instrument {
        id: id.to_string(),
        market: market.to_string(),
        symbol: symbol.to_string(),
        name: None,
        currency: "USD".to_string(),
        sector: None,
        asset_class: AssetClass::Equity,
        watchlists: Vec::new(),
        tags: Default::default(),
        industry: None,
        meta: Default::default(),
    }
}

fn universe() -> Vec<Instrument> {
    vec![inst("US:AAPL", "AAPL"), inst("US:MSFT", "MSFT")]
}

fn cfg() -> AppConfig {
    let mut cfg = AppConfig::default();
    let mut routing = BTreeMap::new();
    routing.insert("sentiment".to_string(), "typesafe/jev-1.13".to_string());
    cfg.llm_routing = routing;
    cfg
}

fn seed_news(db: &mut Db, good: bool, instrument_ids: &[&str], published: NaiveDateTime) {
    let title = if good {
        "Apple upgrades guidance on strong demand"
    } else {
        "Apple cuts guidance on weak demand"
    };
    db.store_items(&[StoreItem::News(NewsItem {
        id: format!("news-{}", &title[..20]),
        instrument_ids: instrument_ids.iter().map(|s| s.to_string()).collect(),
        published,
        title: title.to_string(),
        url: "https://example.com/a".to_string(),
        body: Some("body text".to_string()),
        source: "rss".to_string(),
    })])
    .unwrap();
}

fn stance_answer(choice: &str, confidence: f64) -> serde_json::Value {
    serde_json::json!({
        "stance": {
            "type": "choice",
            "choice": choice,
            "confidence": confidence,
            "probabilities": {"bull": 0.5, "bear": 0.3, "neutral": 0.2}
        }
    })
}

/// Answers bull for upbeat titles, bear otherwise (`_mock_by_title`).
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
        ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "model": "typesafe/jev-1.13-20260917",
            "answers": stance_answer(choice, 0.8),
            "usage": {"input_tokens": 50, "output_tokens": 5, "cost": 0.0001}
        }))
    }
}

fn rows(db: &Db) -> Vec<(String, String, String, f64, String)> {
    let mut stmt = db
        .conn()
        .prepare("SELECT instrument_id, evidence_id, stance, confidence, model, probabilities FROM sentiment")
        .unwrap();
    let out = stmt
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f64>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    out.into_iter()
        .map(|(i, e, s, c, m, p)| {
            let probs: BTreeMap<String, f64> = serde_json::from_str(&p).unwrap();
            assert_eq!(probs["bull"], 0.5);
            (i, e, s, c, m)
        })
        .collect()
}

#[tokio::test]
async fn classify_news_stores_stance_per_instrument() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ByTitle)
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    seed_news(&mut db, true, &["US:AAPL"], now());
    seed_news(&mut db, false, &["US:AAPL", "US:MSFT"], now());

    let stored = classify_with_server(&mut db, &server).await;

    assert_eq!(server.received_requests().await.unwrap().len(), 3);
    assert_eq!(stored.len(), 3);
    let all = rows(&db);
    assert_eq!(all.len(), 3);
    let good = all.iter().find(|r| r.1.contains("upgrades")).unwrap();
    assert_eq!(good.0, "US:AAPL");
    assert_eq!(good.2, "bull");
    assert!((good.3 - 0.8).abs() < 1e-12);
    assert_eq!(good.4, "typesafe/jev-1.13-20260917");
    let both: Vec<&str> = all
        .iter()
        .filter(|r| r.1.contains("cuts"))
        .map(|r| r.0.as_str())
        .collect();
    assert_eq!(both.len(), 2);
    assert!(all
        .iter()
        .filter(|r| r.1.contains("cuts"))
        .all(|r| r.2 == "bear"));
}

/// `classify_news` against a wiremock server (test-only endpoint override).
async fn classify_with_server(
    db: &mut Db,
    server: &MockServer,
) -> Vec<delta_services::sentiment::SentimentRow> {
    // The endpoint override lives on the client; route the module's client
    // there by pointing DECISIONS_URL via env-independent builder.
    delta_services::sentiment::test_hooks::classify_with_url(
        db,
        &cfg(),
        "k",
        &universe(),
        Some(now() - Duration::days(2)),
        &format!("{}/api/alpha/decisions", server.uri()),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn classify_news_is_idempotent() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ByTitle)
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    seed_news(&mut db, true, &["US:AAPL"], now());

    let first = classify_with_server(&mut db, &server).await;
    let again = classify_with_server(&mut db, &server).await;

    assert_eq!(first.len(), 1);
    assert!(again.is_empty());
    assert_eq!(server.received_requests().await.unwrap().len(), 1);
}

/// 502 for the older item, a neutral answer for the newer one
/// (`test_classify_news_skips_failed_requests`).
struct Flaky {
    fail_title: &'static str,
}

impl Respond for Flaky {
    fn respond(&self, request: &Request) -> ResponseTemplate {
        let sent: serde_json::Value = request.body_json().expect("request body");
        let title = sent["state"]["title"].as_str().unwrap_or("");
        if title.contains(self.fail_title) {
            ResponseTemplate::new(502).set_body_json(serde_json::json!({"error": {}}))
        } else {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model": "m",
                "answers": stance_answer("neutral", 0.8),
                "usage": {"input_tokens": 1, "output_tokens": 1, "cost": 0.0}
            }))
        }
    }
}

#[tokio::test]
async fn classify_news_skips_failed_requests() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(Flaky {
            fail_title: "upgrades",
        })
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    seed_news(&mut db, true, &["US:AAPL"], now() - Duration::days(1));
    seed_news(&mut db, false, &["US:AAPL"], now());

    let stored = classify_with_server(&mut db, &server).await;

    assert_eq!(stored.len(), 1);
    assert_eq!(stored[0].stance, "neutral");
}

#[tokio::test]
async fn classify_news_maps_unknown_choice_to_neutral() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "model": "m",
            "answers": stance_answer("sideways", 1.5),
            "usage": {}
        })))
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    seed_news(&mut db, true, &["US:AAPL"], now());

    let stored = classify_with_server(&mut db, &server).await;

    assert_eq!(stored[0].stance, "neutral");
    assert_eq!(stored[0].confidence, 1.0);
}

#[tokio::test]
async fn services_classify_sentiment_counts_and_logs() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/alpha/decisions"))
        .respond_with(ByTitle)
        .mount(&server)
        .await;
    let mut db = Db::open_memory().unwrap();
    seed_news(&mut db, true, &["US:AAPL"], now());
    let lines: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

    let (classified, instruments) =
        delta_services::sentiment::test_hooks::classify_sentiment_with_url(
            &mut db,
            &cfg(),
            "k",
            &universe(),
            Some(now() - Duration::days(2)),
            &format!("{}/api/alpha/decisions", server.uri()),
            Some(&|line: &str| lines.lock().unwrap().push(line.to_string())),
        )
        .await
        .unwrap();

    assert_eq!(classified, 1);
    assert_eq!(instruments, 1);
    assert!(lines
        .lock()
        .unwrap()
        .iter()
        .any(|l| l.contains("Classified 1")));
}

#[test]
fn stance_of_handles_bad_answers() {
    assert_eq!(stance_of(&serde_json::json!({"choice": "bull"})), "bull");
    assert_eq!(
        stance_of(&serde_json::json!({"choice": "sideways"})),
        "neutral"
    );
    assert_eq!(stance_of(&serde_json::json!({})), "neutral");
    assert_eq!(stance_of(&serde_json::Value::Null), "neutral");
    assert_eq!(stance_of(&serde_json::json!("bull")), "neutral");
}

fn seed_judgment(db: &mut Db, id: &str, instrument_id: &str, ts: NaiveDateTime, stance: &str) {
    db.conn()
        .execute(
            "INSERT INTO sentiment \
             (id, instrument_id, evidence_id, ts, stance, confidence, probabilities, model, prompt_version) \
             VALUES (?1, ?2, ?3, ?4, ?5, 1.0, '{}', 'm', 'jev_v1')",
            rusqlite::params![
                id,
                instrument_id,
                format!("n-{id}"),
                ts.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),
                stance
            ],
        )
        .unwrap();
}

#[test]
fn stock_sentiment_weights_recency_and_confidence() {
    let mut db = Db::open_memory().unwrap();
    let now = now();
    let fresh = now - Duration::hours(1);
    let week = now - Duration::days(HALF_LIFE_DAYS as i64);
    seed_judgment(&mut db, "j1", "US:AAPL", fresh, "bull");
    seed_judgment(&mut db, "j2", "US:AAPL", week, "bear");
    seed_judgment(&mut db, "j3", "US:MSFT", fresh, "bear");

    let summary = stock_sentiment(&db, "US:AAPL", 8, Some(now))
        .unwrap()
        .unwrap();
    assert_eq!(summary.bull, 1);
    assert_eq!(summary.bear, 1);
    assert_eq!(summary.neutral, 0);
    // Fresh bull weighs ~1.0; a one-half-life-old bear weighs ~0.5.
    assert!((summary.score - (1.0 - 0.5) / (1.0 + 0.5)).abs() < 0.01);

    let short = stock_sentiment(&db, "US:AAPL", 3, Some(now))
        .unwrap()
        .unwrap();
    assert_eq!(short.bear, 0);
    assert!((short.score - 1.0).abs() < 1e-12);

    assert!(stock_sentiment(&db, "US:NOPE", 7, Some(now))
        .unwrap()
        .is_none());
}

#[test]
fn stock_sentiment_zero_confidence_scores_neutral() {
    let db = Db::open_memory().unwrap();
    let now = now();
    db.conn()
        .execute(
            "INSERT INTO sentiment \
             (id, instrument_id, evidence_id, ts, stance, confidence, probabilities, model, prompt_version) \
             VALUES ('j1', 'US:AAPL', 'n1', ?1, 'bull', 0.0, '{}', 'm', 'jev_v1')",
            [now.format("%Y-%m-%d %H:%M:%S%.6f").to_string()],
        )
        .unwrap();

    let summary = stock_sentiment(&db, "US:AAPL", 7, Some(now))
        .unwrap()
        .unwrap();
    assert_eq!(summary.score, 0.0);
    assert_eq!(summary.bull, 1);
}

/// Judgment ids are `stable_id(instrument_id, evidence_id)` and thus stable
/// across re-runs (Python `judgment_id` parity).
#[test]
fn judgment_id_is_deterministic() {
    assert_eq!(
        judgment_id("US:AAPL", "news-x"),
        delta_core::ids::stable_id(&["US:AAPL", "news-x"])
    );
}
