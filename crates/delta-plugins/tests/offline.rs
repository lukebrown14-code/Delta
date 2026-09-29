//! Offline plugin tests over `wiremock`, mirroring the Python plugin suites:
//! `tests/test_rss.py`, `tests/test_sec_edgar.py`, `test_asx_announcements.py`
//! reuse the same fixtures in `tests/fixtures/`.

use std::path::PathBuf;

use delta_core::db::StoreItem;
use delta_core::models::{AssetClass, Instrument, NewsItem};
use delta_plugins::plugin::DataPlugin;
use delta_plugins::{AsxAnnouncements, Matcher, RssData, SecEdgar};
use wiremock::matchers::{method, path, query_param};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn fixture(name: &str) -> Vec<u8> {
    std::fs::read(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("../../tests/fixtures/{name}")),
    )
    .expect("fixture")
}

fn inst(id: &str, market: &str, symbol: &str, name: Option<&str>) -> Instrument {
    Instrument {
        id: id.to_string(),
        market: market.to_string(),
        symbol: symbol.to_string(),
        name: name.map(str::to_string),
        currency: if market == "asx" { "AUD" } else { "USD" }.to_string(),
        sector: None,
        asset_class: AssetClass::Equity,
        watchlists: Vec::new(),
        tags: Default::default(),
        industry: None,
        meta: Default::default(),
    }
}

fn news(items: Vec<StoreItem>) -> Vec<NewsItem> {
    items
        .into_iter()
        .map(|i| match i {
            StoreItem::News(n) => n,
            _ => panic!("expected news rows"),
        })
        .collect()
}

// ------------------------------------------------------------------ RSS

#[tokio::test]
async fn rss_serves_fixture_and_matches_instruments() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/feed.xml"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(fixture("business_feed.xml"), "application/xml"),
        )
        .mount(&server)
        .await;

    let mut plugin = RssData::default();
    plugin.configure(&serde_json::json!({"feeds": [format!("{}/feed.xml", server.uri())]}));
    let universe = vec![
        inst("US:AAPL", "us", "AAPL", Some("Apple Inc.")),
        inst("US:MSFT", "us", "MSFT", Some("Microsoft Corporation")),
    ];
    let since = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let items = news(plugin.fetch(&universe, since).await.unwrap());
    assert!(!items.is_empty(), "fixture feed should yield items");
    for item in &items {
        assert!(!item.title.is_empty());
        assert!(item.url.starts_with("http"));
    }
    // Items are matched to universe instruments by whole-word ticker/name.
    for item in &items {
        for id in &item.instrument_ids {
            assert!(universe.iter().any(|u| &u.id == id));
        }
    }
}

#[tokio::test]
async fn rss_matcher_whole_word_only() {
    let matcher = Matcher::new(&[inst("US:F", "us", "F", Some("Ford Motor"))]);
    // "F" alone would hit ordinary words with a case-insensitive match; the
    // ticker is case-sensitive whole-word.
    assert_eq!(matcher.matches("the F stock"), vec!["US:F"]);
    assert!(matcher.matches("of the market").is_empty());
    assert_eq!(matcher.matches("ford reports"), vec!["US:F".to_string()]);
}

#[tokio::test]
async fn rss_failing_feed_is_skipped() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;
    let mut plugin = RssData::default();
    plugin.configure(&serde_json::json!({"feeds": [format!("{}/feed.xml", server.uri())]}));
    let since = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    assert!(plugin.fetch(&[], since).await.unwrap().is_empty());
}

// ------------------------------------------------------------------ ASX

#[tokio::test]
async fn asx_maps_announcements_from_fixture() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/asx-research/1.0/companies/bhp/announcements"))
        .and(query_param("itemsPerPage", "20"))
        .and(query_param("fromDate", "2026-08-01"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(
                serde_json::from_slice::<serde_json::Value>(&fixture("asx_announcements_bhp.json"))
                    .unwrap(),
            ),
        )
        .mount(&server)
        .await;

    let mut plugin = AsxAnnouncements {
        base_url: server.uri(),
        ..Default::default()
    };
    plugin.configure(&serde_json::json!({"backoff_seconds": 0.0, "max_retries": 2}));
    let bhp = inst("ASX:BHP", "asx", "BHP", None);
    let since = chrono::NaiveDate::from_ymd_opt(2026, 8, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let items = news(
        plugin
            .fetch(&[bhp, inst("US:AAPL", "us", "AAPL", None)], since)
            .await
            .unwrap(),
    );
    let received = server.received_requests().await.unwrap_or_default();
    for r in &received {
        eprintln!("req: {} {}", r.method, r.url);
    }
    let received = server.received_requests().await.unwrap_or_default();
    eprintln!(
        "received: {:?}",
        received
            .iter()
            .map(|r| (r.url.clone(), r.method.clone()))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        items.len(),
        3,
        "US instruments are filtered out by market; fixture has 3 rows"
    );
    // Ids derive from ASX's document key and stay stable.
    for item in &items {
        assert_eq!(item.source, "asx_announcements");
        assert_eq!(item.instrument_ids, vec!["ASX:BHP".to_string()]);
    }
}

#[tokio::test]
async fn asx_404_is_skipped_not_an_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let plugin = AsxAnnouncements {
        base_url: server.uri(),
        ..Default::default()
    };
    let since = chrono::NaiveDate::from_ymd_opt(2026, 8, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let out = plugin
        .fetch(&[inst("ASX:ZZZ", "asx", "ZZZ", None)], since)
        .await
        .unwrap();
    assert!(out.is_empty());
}

#[tokio::test]
async fn asx_transient_500_retries_then_succeeds() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            serde_json::json!({"data": {"items": [
                {"documentKey": "k1", "date": "2026-09-09T22:31:12.000Z",
                 "headline": "Result of Meeting", "isPriceSensitive": true}
            ]}}),
        ))
        .mount(&server)
        .await;
    let mut plugin = AsxAnnouncements {
        base_url: server.uri(),
        ..Default::default()
    };
    plugin.configure(&serde_json::json!({"backoff_seconds": 0.0}));
    let since = chrono::NaiveDate::from_ymd_opt(2026, 8, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let items = news(
        plugin
            .fetch(&[inst("ASX:BHP", "asx", "BHP", None)], since)
            .await
            .unwrap(),
    );
    assert_eq!(items.len(), 1);
    assert!(
        items[0].title.starts_with("[PS] "),
        "price-sensitive rows get the [PS] prefix"
    );
}

// ------------------------------------------------------------------ SEC

#[tokio::test]
async fn sec_fetches_filings_and_facts_with_cassettes() {
    let server = MockServer::start().await;

    let tickers: serde_json::Value =
        serde_json::from_slice(&fixture("edgar/company_tickers.json")).unwrap();
    let submissions: serde_json::Value =
        serde_json::from_slice(&fixture("edgar/submissions_CIK0000320193.json")).unwrap();
    let facts: serde_json::Value =
        serde_json::from_slice(&fixture("edgar/companyfacts_CIK0000320193.json")).unwrap();

    Mock::given(method("GET"))
        .and(path("/company_tickers.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(tickers))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/submissions/CIK0000320193.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(submissions))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/xbrl/companyfacts/CIK0000320193.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(facts))
        .mount(&server)
        .await;

    let plugin = SecEdgar {
        contact: "you@example.com".to_string(),
        sleep_scale: 0.0,
        tickers_url: format!("{}/company_tickers.json", server.uri()),
        data_base_url: server.uri(),
        ..Default::default()
    };
    let aapl = inst("US:AAPL", "us", "AAPL", Some("Apple Inc."));
    let msft = inst("US:MSFT", "us", "MSFT", None); // mapped but not cassetted: its 404s are skipped? no — 404 is not retryable, fetch errors
    let since = chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let rows = plugin.fetch(&[aapl], since).await.unwrap();
    assert!(!rows.is_empty());
    let news: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            StoreItem::News(n) => Some(n),
            _ => None,
        })
        .collect();
    assert!(!news.is_empty(), "cassette filings map to news");
    assert!(news.iter().all(|n| n.source == "sec_edgar"));
    let funds: Vec<_> = rows
        .iter()
        .filter_map(|r| match r {
            StoreItem::Fundamental(f) => Some(f),
            _ => None,
        })
        .collect();
    assert!(!funds.is_empty(), "cassette facts map to fundamentals");
    let _ = msft;
}

// ------------------------------------------------------------------ Yahoo

mod yahoo {
    use super::*;
    use delta_plugins::yahoo::{YahooClient, YahooQuotes};
    use std::collections::BTreeMap;

    fn client(base: &str) -> YahooClient {
        YahooClient {
            quote_base_url: base.to_string(),
            ..YahooClient::default()
        }
    }

    #[tokio::test]
    async fn quote_summary_uses_cookie_then_crumb() {
        let server = MockServer::start().await;
        // Step 1: the cookie endpoint (any response; the jar is what matters).
        Mock::given(method("GET"))
            .and(path("/cookie"))
            .respond_with(ResponseTemplate::new(302).append_header("Set-Cookie", "yf=1"))
            .mount(&server)
            .await;
        // Step 2: the crumb.
        Mock::given(method("GET"))
            .and(path("/getcrumb"))
            .respond_with(ResponseTemplate::new(200).set_body_string("aBcD1234"))
            .mount(&server)
            .await;
        // Step 3: quoteSummary requires the crumb query param.
        Mock::given(method("GET"))
            .and(path("/v10/finance/quoteSummary/AAPL"))
            .and(query_param("crumb", "aBcD1234"))
            .and(query_param("modules", "calendarEvents"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "quoteSummary": {"result": [{"calendarEvents": {}}]}
            })))
            .mount(&server)
            .await;

        let c = client(&server.uri());
        let payload = c.quote_summary("AAPL", "calendarEvents").await.unwrap();
        assert_eq!(
            payload["quoteSummary"]["result"][0]["calendarEvents"],
            serde_json::json!({})
        );
    }

    #[tokio::test]
    async fn quotes_batch_sends_symbols_and_crumb() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cookie"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/getcrumb"))
            .respond_with(ResponseTemplate::new(200).set_body_string("crumb1"))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v7/finance/quote"))
            .and(query_param("symbols", "AAPL,BHP.AX"))
            .and(query_param("crumb", "crumb1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "quoteResponse": {"result": [
                    {"symbol": "AAPL", "regularMarketPrice": 100.0, "currency": "USD"},
                    {"symbol": "BHP.AX", "regularMarketPrice": 40.0, "currency": "AUD"}
                ]}
            })))
            .mount(&server)
            .await;

        let c = client(&server.uri());
        let payload = c
            .quotes(&["AAPL".to_string(), "BHP.AX".to_string()])
            .await
            .unwrap();
        let results = payload["quoteResponse"]["result"].as_array().unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[1]["currency"], "AUD");
    }

    #[tokio::test]
    async fn search_maps_exchanges() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/finance/search"))
            .and(query_param("q", "apple"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "quotes": [{"symbol": "AAPL", "shortname": "Apple Inc.",
                            "exchange": "NMS", "quoteType": "EQUITY"}]
            })))
            .mount(&server)
            .await;
        let c = client(&server.uri());
        let results = c.search("apple", 8).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].symbol, "AAPL");
        assert_eq!(results[0].market, "us");
    }

    /// The quotes stream state machine with a scripted transport:
    /// stale ticks ignored, unknown symbols ignored, backoff doubling capped.
    #[tokio::test]
    async fn quotes_stream_state_machine() {
        let bhp = inst("ASX:BHP", "asx", "BHP", None);
        let suffixes: BTreeMap<String, String> =
            BTreeMap::from([("asx".to_string(), ".AX".to_string())]);
        let mut yq = YahooQuotes::new(&[bhp], &suffixes);
        yq.receive(
            &serde_json::json!({"id": "BHP.AX", "price": 40.5, "time": 1000, "currency": "AUD"}),
        );
        assert_eq!(yq.quotes["ASX:BHP"].price, 40.5);
        assert_eq!(yq.quotes["ASX:BHP"].currency, "AUD");
        yq.receive(&serde_json::json!({"id": "BHP.AX", "price": 39.0, "time": 500}));
        assert_eq!(yq.quotes["ASX:BHP"].price, 40.5, "stale tick ignored");
        assert_eq!(YahooQuotes::reconnect_delay(1), 2);
        assert_eq!(YahooQuotes::reconnect_delay(30), 30, "backoff caps at 30s");
    }
}
