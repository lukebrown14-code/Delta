use delta_core::{
    db::{Db, StoreItem},
    models::{AssetClass, Bar, Instrument},
};
use delta_plugins::YahooClient;
use delta_services::asset_metrics::*;
use serde_json::json;
fn inst(class: AssetClass) -> Instrument {
    serde_json::from_value(
        json!({"id":"US:TEST","market":"us","symbol":"TEST","currency":"USD","asset_class":class}),
    )
    .unwrap()
}
#[test]
fn profiles_preserve_key_specific_scaling_group_order_and_glossary() {
    let cases = [
        (AssetClass::Equity, "Gross margin", "grossMargins", "46.0%"),
        (AssetClass::Etf, "Distribution yield", "yield", "4.7%"),
        (AssetClass::Bond, "Coupon", "couponRate", "4.7%"),
        (
            AssetClass::Commodity,
            "Open interest",
            "openInterest",
            "4.73",
        ),
        (AssetClass::Fx, "Bid", "bid", "4.7300"),
        (AssetClass::Crypto, "24h volume", "volume24Hr", "$4.73"),
        (AssetClass::Cash, "Dividend yield", "dividendYield", "4.7%"),
        (
            AssetClass::Other,
            "Previous close",
            "previousClose",
            "4.7300",
        ),
    ];
    for (class, label, key, expected) in cases {
        let value = if key == "grossMargins" {
            0.46
        } else if matches!(key, "yield" | "couponRate") {
            0.0473
        } else {
            4.73
        };
        let result = normalize_asset_metrics(&inst(class), &json!({key:value}), vec![], vec![]);
        assert_eq!(result.profile, class.as_str());
        assert_eq!(result.values[label], expected);
        assert!(metric_help(label).is_some());
    }
    assert_eq!(groups_for("equity")[0].0, "Profitability");
    assert_eq!(groups_for("etf")[0].0, "Fund Info");
    assert_eq!(groups_for("unknown"), groups_for("other"));
}
#[test]
fn normalized_history_computes_returns_population_volatility_and_bond_basis_points() {
    let result = normalize_asset_metrics(
        &inst(AssetClass::Equity),
        &json!({"fiftyTwoWeekHigh":200,"fiftyTwoWeekLow":80}),
        vec![100., 110., 99.],
        vec!["a".into(), "b".into(), "c".into()],
    );
    assert_eq!(result.change_label, "-1.0%");
    assert!((result.volatility.unwrap() - 0.1).abs() < 1e-10);
    assert_eq!(result.period_high, Some(110.));
    assert_eq!(result.period_low, Some(99.));
    assert_eq!(result.values["From 52w high"], "-50.5%");
    assert_eq!(result.history_end.as_deref(), Some("c"));
    let bond =
        normalize_asset_metrics(&inst(AssetClass::Bond), &json!({}), vec![4.0, 4.25], vec![]);
    assert_eq!(bond.change_label, "+25.0 bps");
    assert_eq!(bond.values["Current yield"], "4.25");
    assert!(bond.series_times.is_empty());
}
async fn server() -> wiremock::MockServer {
    use wiremock::{matchers::path, Mock, ResponseTemplate};
    let server = wiremock::MockServer::start().await;
    Mock::given(path("/cookie"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/getcrumb"))
        .respond_with(ResponseTemplate::new(200).set_body_string("offline"))
        .mount(&server)
        .await;
    Mock::given(path("/v10/finance/quoteSummary/TEST")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"quoteSummary":{"result":[{"summaryDetail":{"fiftyTwoWeekHigh":{"raw":200}},"earningsTrend":{"trend":[{"period":"+1q","earningsEstimate":{"avg":{"raw":1.2},"growth":{"raw":0.1}}}]}}]}}))).mount(&server).await;
    server
}
fn client(server: &wiremock::MockServer) -> YahooClient {
    YahooClient {
        chart_base_url: server.uri(),
        quote_base_url: server.uri(),
        ..Default::default()
    }
}
#[tokio::test]
async fn requested_history_is_adjusted_and_intraday_falls_back_only_when_empty() {
    use wiremock::{
        matchers::{path, query_param},
        Mock, ResponseTemplate,
    };
    let server = server().await;
    Mock::given(path("/v8/finance/chart/TEST"))
        .and(query_param("range", "1d"))
        .and(query_param("interval", "5m"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"chart":{"result":[{"timestamp":[],"indicators":{"quote":[{"close":[]}]}}]}}),
        ))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(path("/v8/finance/chart/TEST")).and(query_param("range","5d")).and(query_param("interval","1d")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"chart":{"result":[{"timestamp":[1700000000,1700086400,1700172800],"indicators":{"quote":[{"close":[10,11,12]}],"adjclose":[{"adjclose":[9,null,10]}]}}]}}))).expect(1).mount(&server).await;
    let result = fetch_asset_metrics(&client(&server), &inst(AssetClass::Equity), "1d", None).await;
    assert!(result.error.is_none());
    assert_eq!(result.series, vec![9., 10.]);
    assert_eq!(result.series_times.len(), 2);
    assert_eq!(result.values["EPS est (next q)"], "1.20");
    assert_eq!(result.values["EPS growth est"], "10.0%");
}
#[tokio::test]
async fn all_range_local_fallback_never_prepends_bars_to_provider_history() {
    use wiremock::{
        matchers::{path, query_param},
        Mock, ResponseTemplate,
    };
    let server = server().await;
    let mut db = Db::open_memory().unwrap();
    let instrument = inst(AssetClass::Other);
    db.store_items(&[StoreItem::Bar(Bar {
        instrument_id: instrument.id.clone(),
        ts: chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap(),
        open: 50.,
        high: 50.,
        low: 50.,
        close: 50.,
        volume: 1.,
        source: "offline".into(),
    })])
    .unwrap();
    Mock::given(path("/v8/finance/chart/TEST"))
        .and(query_param("range", "max"))
        .and(query_param("interval", "1wk"))
        .respond_with(ResponseTemplate::new(200).set_body_json(
            json!({"chart":{"result":[{"timestamp":[],"indicators":{"quote":[{"close":[]}]}}]}}),
        ))
        .mount(&server)
        .await;
    assert_eq!(
        fetch_asset_metrics(&client(&server), &instrument, "all", Some(&db))
            .await
            .series,
        vec![50.]
    );
    server.reset().await;
    // Reset also clears summary stubs; install a minimal new response.
    Mock::given(path("/cookie"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&server)
        .await;
    Mock::given(path("/getcrumb"))
        .respond_with(ResponseTemplate::new(200).set_body_string("offline"))
        .mount(&server)
        .await;
    Mock::given(path("/v10/finance/quoteSummary/TEST"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"quoteSummary":{"result":[{}]}})),
        )
        .mount(&server)
        .await;
    Mock::given(path("/v8/finance/chart/TEST")).respond_with(ResponseTemplate::new(200).set_body_json(json!({"chart":{"result":[{"timestamp":[1700000000],"indicators":{"quote":[{"close":[10]}]}}]}}))).mount(&server).await;
    assert_eq!(
        fetch_asset_metrics(&client(&server), &instrument, "all", Some(&db))
            .await
            .series,
        vec![10.]
    );
}
#[tokio::test]
async fn provider_error_is_exposed_instead_of_synthetic_local_history() {
    use wiremock::{matchers::path, Mock, ResponseTemplate};
    let server = server().await;
    Mock::given(path("/v8/finance/chart/TEST"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;
    let result =
        fetch_asset_metrics(&client(&server), &inst(AssetClass::Equity), "all", None).await;
    assert!(result.error.as_deref().unwrap().contains("403"));
    assert!(result.series.is_empty());
    assert!(result.fetched_at.is_none());
}
