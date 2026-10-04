//! Asset-class-aware live metrics and provider history for the Watchlist.
use chrono::{NaiveDateTime, Utc};
use delta_core::{config::AppConfig, db::Db, models::Instrument};
use delta_plugins::{
    metrics::{format_metric, merge_quote_summary, profiles},
    yahoo::{yf_symbol, YahooClient},
};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;

pub type MetricGroups = Vec<(String, Vec<(String, String)>)>;
#[derive(Debug, Clone, PartialEq)]
pub struct AssetMetrics {
    pub instrument_id: String,
    pub profile: String,
    pub values: BTreeMap<String, String>,
    pub groups: MetricGroups,
    pub series: Vec<f64>,
    pub series_times: Vec<String>,
    pub change_label: String,
    pub fetched_at: Option<NaiveDateTime>,
    pub source: String,
    pub history_start: Option<String>,
    pub history_end: Option<String>,
    pub period_high: Option<f64>,
    pub period_low: Option<f64>,
    pub volatility: Option<f64>,
    pub week_52_high: Option<f64>,
    pub week_52_low: Option<f64>,
    pub error: Option<String>,
}
impl AssetMetrics {
    fn empty(instrument: &Instrument) -> Self {
        Self {
            instrument_id: instrument.id.clone(),
            profile: profile_for(instrument).into(),
            values: BTreeMap::new(),
            groups: vec![],
            series: vec![],
            series_times: vec![],
            change_label: String::new(),
            fetched_at: None,
            source: "Yahoo Finance".into(),
            history_start: None,
            history_end: None,
            period_high: None,
            period_low: None,
            volatility: None,
            week_52_high: None,
            week_52_low: None,
            error: None,
        }
    }
}
pub fn profile_for(instrument: &Instrument) -> &'static str {
    instrument.asset_class.as_str()
}
pub use profiles::groups_for;
pub fn metric_help(label: &str) -> Option<&'static str> {
    profiles::METRIC_HELP
        .iter()
        .find(|(key, _)| *key == label)
        .map(|(_, value)| *value)
}
pub fn range_spec(range: &str) -> (&'static str, &'static str) {
    match range {
        "day" | "1d" => ("1d", "5m"),
        "5d" => ("5d", "1d"),
        "6m" => ("6mo", "1d"),
        "ytd" => ("ytd", "1d"),
        "1y" => ("1y", "1d"),
        "all" => ("max", "1wk"),
        _ => ("1mo", "1d"),
    }
}
fn raw(value: &Value) -> &Value {
    value.get("raw").unwrap_or(value)
}
fn number(value: &Value) -> Option<f64> {
    let value = raw(value);
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(|v| v.parse().ok()))
        .filter(|v| v.is_finite())
}
pub fn group_values(profile: &str, info: &Value) -> MetricGroups {
    let values = profiles::keys_for(profile)
        .iter()
        .filter_map(|(label, key, fmt)| {
            info.get(key)
                .and_then(|v| format_metric(v, fmt))
                .map(|v| (*label, v))
        })
        .collect::<BTreeMap<_, _>>();
    let mut groups = Vec::new();
    for (title, labels) in profiles::groups_for(profile) {
        let rows = labels
            .iter()
            .filter_map(|label| {
                values
                    .get(label)
                    .map(|value| ((*label).into(), value.clone()))
            })
            .collect::<Vec<_>>();
        if !rows.is_empty() {
            groups.push(((*title).into(), rows));
        }
    }
    if groups.is_empty() && !values.is_empty() {
        groups.push((
            "Available Metrics".into(),
            profiles::keys_for(profile)
                .iter()
                .filter_map(|(label, _, _)| values.get(label).map(|v| ((*label).into(), v.clone())))
                .collect(),
        ));
    }
    groups
}
/// Normalize one info payload and aligned adjusted closes; provider history is never merged with local bars.
pub fn normalize_asset_metrics(
    instrument: &Instrument,
    info: &Value,
    series: Vec<f64>,
    times: Vec<String>,
) -> AssetMetrics {
    let mut result = AssetMetrics::empty(instrument);
    result.series = series;
    result.series_times = if times.len() == result.series.len() {
        times
    } else {
        vec![]
    };
    result.groups = group_values(&result.profile, info);
    result.week_52_high = number(&info["fiftyTwoWeekHigh"]);
    result.week_52_low = number(&info["fiftyTwoWeekLow"]);
    if let Some(&last) = result.series.last() {
        result.period_high = result.series.iter().copied().reduce(f64::max);
        result.period_low = result.series.iter().copied().reduce(f64::min);
        result.values.insert(
            if result.profile == "bond" {
                "Current yield"
            } else {
                "Current price"
            }
            .into(),
            delta_core::format::grouped(last, 2),
        );
        result.history_start = result.series_times.first().cloned();
        result.history_end = result.series_times.last().cloned();
        if result.series.len() > 1 {
            let first = result.series[0];
            result.change_label = if result.profile == "bond" {
                format!("{:+.1} bps", (last - first) * 100.0)
            } else if first != 0.0 {
                format!("{:+.1}%", (last / first - 1.0) * 100.0)
            } else {
                String::new()
            };
            let returns = result
                .series
                .windows(2)
                .filter(|pair| pair[0] != 0.0)
                .map(|pair| pair[1] / pair[0] - 1.0)
                .collect::<Vec<_>>();
            if !returns.is_empty() {
                let mean = returns.iter().sum::<f64>() / returns.len() as f64;
                result.volatility = Some(
                    (returns.iter().map(|v| (v - mean).powi(2)).sum::<f64>()
                        / returns.len() as f64)
                        .sqrt(),
                );
            }
        }
        if let Some(high) = result.week_52_high.filter(|v| *v != 0.0) {
            if let Some((_, rows)) = result
                .groups
                .iter_mut()
                .find(|(title, _)| title == "Price Context")
            {
                rows.push((
                    "From 52w high".into(),
                    format!("{:+.1}%", (last / high - 1.0) * 100.0),
                ));
            }
        }
    }
    for (_, rows) in &result.groups {
        for (label, value) in rows {
            result.values.insert(label.clone(), value.clone());
        }
    }
    result.fetched_at = Some(Utc::now().naive_utc());
    result
}
async fn history(
    client: &YahooClient,
    instrument: &Instrument,
    range: &str,
) -> Result<(Vec<f64>, Vec<String>), String> {
    let symbol = yf_symbol(instrument, &client.suffixes);
    let url = if client.chart_base_url.is_empty() {
        delta_plugins::yahoo::CHART_URL.replace("{symbol}", &symbol)
    } else {
        format!(
            "{}/v8/finance/chart/{symbol}",
            client.chart_base_url.trim_end_matches('/')
        )
    };
    let (period, interval) = range_spec(range);
    let payload = client
        .http
        .get(url)
        .query(&[("range", period), ("interval", interval)])
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .json::<Value>()
        .await
        .map_err(|e| e.to_string())?;
    if !payload["chart"]["error"].is_null() {
        return Err(payload["chart"]["error"].to_string());
    }
    let result = &payload["chart"]["result"][0];
    let mut closes = Vec::new();
    let mut times = Vec::new();
    let stamps = result["timestamp"].as_array().cloned().unwrap_or_default();
    let raw_closes = &result["indicators"]["quote"][0]["close"];
    let adjusted = &result["indicators"]["adjclose"][0]["adjclose"];
    let values = if adjusted.is_array() {
        adjusted
    } else {
        raw_closes
    };
    for (index, stamp) in stamps.iter().enumerate() {
        let Some(close) = number(&values[index]) else {
            continue;
        };
        let Some(ts) = stamp
            .as_i64()
            .and_then(|v| chrono::DateTime::from_timestamp(v, 0))
        else {
            continue;
        };
        closes.push(close);
        times.push(ts.format("%Y-%m-%d %H:%M:%S+00:00").to_string());
    }
    Ok((closes, times))
}
/// Fetch profile metrics and requested history; errors are displayed by the inspector.
pub fn fetch_asset_metrics<'a>(
    client: &'a YahooClient,
    instrument: &'a Instrument,
    range: &'a str,
    db: Option<&Db>,
) -> impl Future<Output = AssetMetrics> + Send + 'a {
    // rusqlite connections are not Sync. Read the optional local fallback
    // before constructing the Send future; the owned bars can safely cross
    // provider awaits, and are only used if the all-history request is empty.
    let fallback = if range == "all" {
        db.map(|db| db.bars(&instrument.id).map_err(|error| error.to_string()))
    } else {
        None
    };
    async move {
        let fetch = async {
            let symbol = yf_symbol(instrument, &client.suffixes);
            let payload = client
                .quote_summary(
                    &symbol,
                    "price,summaryDetail,defaultKeyStatistics,financialData,summaryProfile,assetProfile,fundProfile,earningsTrend,calendarEvents",
                )
                .await
                .map_err(|error| error.to_string())?;
            if !payload["quoteSummary"]["error"].is_null() {
                return Err(payload["quoteSummary"]["error"].to_string());
            }
            let info = merge_quote_summary(&payload).unwrap_or_else(|| serde_json::json!({}));
            let (mut series, mut times) = history(client, instrument, range).await?;
            if series.is_empty() && matches!(range, "day" | "1d") {
                (series, times) = history(client, instrument, "5d").await?;
            }
            if series.is_empty() && range == "all" {
                if let Some(fallback) = fallback {
                    for bar in fallback? {
                        series.push(bar.close);
                        times.push(bar.ts.to_string());
                    }
                }
            }
            let mut result = normalize_asset_metrics(instrument, &info, series, times);
            if result.profile == "equity" {
                merge_estimates(&mut result, &payload);
            }
            Ok::<_, String>(result)
        }
        .await;
        match fetch {
            Ok(result) => result,
            Err(error) => {
                let mut result = AssetMetrics::empty(instrument);
                result.error = Some(error);
                result
            }
        }
    }
}
fn merge_estimates(result: &mut AssetMetrics, payload: &Value) {
    let trend = &payload["quoteSummary"]["result"][0]["earningsTrend"]["trend"];
    let mut additions = Vec::new();
    for row in trend.as_array().map(Vec::as_slice).unwrap_or(&[]) {
        for (period, path, label, fmt) in [
            (
                "+1q",
                &["earningsEstimate", "avg"][..],
                "EPS est (next q)",
                "number",
            ),
            (
                "+1q",
                &["earningsEstimate", "growth"][..],
                "EPS growth est",
                "ratio",
            ),
            (
                "0y",
                &["revenueEstimate", "avg"][..],
                "Revenue est (fy)",
                "money",
            ),
        ] {
            if row["period"].as_str() == Some(period) {
                if let Some(value) = format_metric(&row[path[0]][path[1]], fmt) {
                    additions.push((label.to_string(), value));
                }
            }
        }
    }
    if !additions.is_empty() {
        if let Some((_, rows)) = result
            .groups
            .iter_mut()
            .find(|(title, _)| title == "Analyst View")
        {
            rows.extend(additions.clone());
        } else {
            result
                .groups
                .push(("Analyst View".into(), additions.clone()));
        }
        result.values.extend(additions);
    }
}
pub async fn fetch_asset_metrics_configured(
    instrument: &Instrument,
    range: &str,
    db: Option<&Db>,
    cfg: &AppConfig,
) -> AssetMetrics {
    let client = YahooClient {
        suffixes: delta_plugins::configured_suffixes(cfg, "yfinance"),
        ..Default::default()
    };
    fetch_asset_metrics(&client, instrument, range, db).await
}
