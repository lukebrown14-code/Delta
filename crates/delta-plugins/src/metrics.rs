//! Watchlist inspector metrics: the ordered profile tables from Python's
//! `delta/metrics.toml` (label, Yahoo info key, format), formatting parity
//! with `asset_metrics._fmt`, and the quoteSummary -> info merge.

use serde_json::Value;
use std::collections::BTreeMap;

#[path = "metrics_profiles.rs"]
pub mod profiles;
pub use profiles::{groups_for, keys_for, EQUITY_KEYS, METRIC_HELP};

/// K/M/B/T compaction (`_compact`): two decimals, comma grouped; plain
/// integers render with no decimals (`{:,0f}`).
fn compact(value: f64) -> String {
    let magnitude = value.abs();
    for (threshold, suffix) in [(1e12, "T"), (1e9, "B"), (1e6, "M"), (1e3, "K")] {
        if magnitude >= threshold {
            return format!(
                "{}{suffix}",
                delta_core::format::grouped(value / threshold, 2)
            );
        }
    }
    if value.fract() == 0.0 {
        delta_core::format::grouped(value, 0)
    } else {
        delta_core::format::grouped(value, 2)
    }
}

fn money(value: f64) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    format!("{sign}${}", compact(value.abs()))
}

fn display_value(value: &Value) -> String {
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

/// One value formatted per its key spec (`asset_metrics._fmt`); `None` when
/// the value is absent; non-numeric values retain Python's text fallback.
pub fn format_metric(value: &Value, fmt: &str) -> Option<String> {
    // quoteSummary values arrive as {"raw": ..., "fmt": ...}; the raw field
    // is the number the spec formats.
    let value = match value.get("raw") {
        Some(raw) if !value.as_object().map(|o| o.len() > 2).unwrap_or(false) => raw,
        _ => value,
    };
    if value.is_null() || value.as_str() == Some("") {
        return None;
    }
    match fmt {
        "text" => {
            return Some(
                value
                    .as_str()
                    .map(str::to_string)
                    .unwrap_or_else(|| value.to_string()),
            )
        }
        "date" => {
            let secs = value
                .as_i64()
                .or_else(|| value.as_f64().map(|v| v as i64))
                .or_else(|| value.as_str().and_then(|v| v.parse().ok()));
            let Some(secs) = secs else {
                return Some(display_value(value));
            };
            use chrono::TimeZone;
            let Some(stamp) = chrono::Utc.timestamp_opt(secs, 0).single() else {
                return Some(display_value(value));
            };
            let day = stamp.format("%d").to_string();
            let day = day.trim_start_matches('0');
            return Some(format!("{day} {}", stamp.format("%b %Y")));
        }
        _ => {}
    }
    let number = match value {
        Value::Number(n) => n.as_f64()?,
        Value::String(s) => match s.parse::<f64>() {
            Ok(v) => v,
            Err(_) => return Some(s.clone()),
        },
        Value::Bool(v) => {
            if *v {
                1.0
            } else {
                0.0
            }
        }
        _ => return Some(display_value(value)),
    };
    match fmt {
        "ratio" => Some(format!("{:.1}%", number * 100.0)),
        "percent" => Some(format!("{:.1}%", number)),
        "x" => Some(format!("{}x", delta_core::format::grouped(number, 1))),
        "money" => Some(money(number)),
        "count" => Some(compact(number)),
        "fx" => Some(format!("{:.4}", number)),
        "price" => {
            if number.abs() >= 10.0 {
                Some(delta_core::format::grouped(number, 2))
            } else {
                Some(format!("{number:.4}"))
            }
        }
        _ => Some(delta_core::format::grouped(number, 2)),
    }
}

/// The equity metric rows, in table order, skipping absent keys.
pub fn equity_values(info: &Value) -> Vec<(String, String)> {
    let mut rows = Vec::new();
    for (label, key, fmt) in EQUITY_KEYS {
        if let Some(text) = info.get(key).and_then(|v| format_metric(v, fmt)) {
            rows.push(((*label).to_string(), text));
        }
    }
    rows
}

/// Merge a quoteSummary payload into one flat info map: the first result's
/// module objects (`price`, `summaryDetail`, `defaultKeyStatistics`,
/// `financialData`, `summaryProfile`, `assetProfile`) flattened one level.
pub fn merge_quote_summary(payload: &Value) -> Option<Value> {
    let result = payload
        .get("quoteSummary")
        .and_then(|q| q.get("result"))
        .and_then(|r| r.as_array())?
        .first()?;
    let mut info = serde_json::Map::new();
    if let Some(obj) = result.as_object() {
        for (_module, body) in obj {
            if let Some(fields) = body.as_object() {
                for (key, value) in fields {
                    info.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
        }
    }
    Some(Value::Object(info))
}

/// The inspector's headline order: the most-watched metrics first (the
/// painter caps the two-column grid, so deep table order would hide them).
pub const HEADLINE_ORDER: &[&str] = &[
    "Market cap",
    "P/E",
    "EPS (trailing)",
    "Dividend yield",
    "52w high",
    "52w low",
    "Beta",
    "Revenue growth",
    "EPS growth",
    "Gross margin",
    "Operating margin",
    "Net margin",
    "ROE",
    "Free cash flow",
    "Total cash",
    "Total debt",
    "Forward P/E",
    "EV / EBITDA",
    "Price / Book",
    "Price / Sales",
    "Payout ratio",
    "Current ratio",
    "50-day average",
    "200-day average",
];

/// Equity rows in the inspector's headline order (missing keys skipped).
pub fn headline_values(info: &Value) -> Vec<(String, String)> {
    let rows = equity_values(info);
    let by_label: BTreeMap<String, String> = rows.into_iter().collect();
    HEADLINE_ORDER
        .iter()
        .filter_map(|label| {
            by_label
                .get(*label)
                .map(|v| ((*label).to_string(), v.clone()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn formats_match_the_python_spec() {
        assert_eq!(format_metric(&json!(""), "text"), None);
        assert_eq!(
            format_metric(&json!("unavailable"), "ratio").as_deref(),
            Some("unavailable")
        );
        assert_eq!(
            format_metric(&json!("unknown"), "date").as_deref(),
            Some("unknown")
        );
        assert_eq!(
            format_metric(&json!(1234.5), "x").as_deref(),
            Some("1,234.5x")
        );
        assert_eq!(format_metric(&json!(0.0473), "ratio").unwrap(), "4.7%");
        assert_eq!(format_metric(&json!(0.32), "percent").unwrap(), "0.3%");
        assert_eq!(format_metric(&json!(31.2), "x").unwrap(), "31.2x");
        assert_eq!(
            format_metric(&json!(3_400_000_000_000.0), "money").unwrap(),
            "$3.40T"
        );
        assert_eq!(
            format_metric(&json!(12_790_000.0), "money").unwrap(),
            "$12.79M"
        );
        assert_eq!(format_metric(&json!(15733000), "count").unwrap(), "15.73M");
        assert_eq!(format_metric(&json!(236.3), "price").unwrap(), "236.30");
        assert_eq!(format_metric(&json!(0.9734), "price").unwrap(), "0.9734");
        assert_eq!(format_metric(&json!(97.4), "number").unwrap(), "97.40");
        assert_eq!(
            format_metric(&json!(1_700_000_000), "date").unwrap(),
            "14 Nov 2023"
        );
        assert_eq!(format_metric(&json!("buy"), "text").unwrap(), "buy");
        assert_eq!(format_metric(&json!(null), "ratio"), None);
    }

    #[test]
    fn equity_values_follow_the_table_order_and_skip_missing() {
        let info = json!({
            "trailingPE": 31.2,
            "marketCap": 3400000000000.0,
            "grossMargins": 0.46,
            "recommendationKey": "buy",
        });
        let rows = equity_values(&info);
        let labels: Vec<&str> = rows.iter().map(|(l, _)| l.as_str()).collect();
        assert_eq!(
            labels,
            vec!["Gross margin", "P/E", "Market cap", "Consensus"]
        );
        assert_eq!(rows[1], ("P/E".to_string(), "31.2x".to_string()));
        assert_eq!(rows[2], ("Market cap".to_string(), "$3.40T".to_string()));
    }

    #[test]
    fn quote_summary_modules_flatten() {
        let payload = json!({
            "quoteSummary": { "result": [ {
                "price": { "regularMarketPrice": { "raw": 236.3 } },
                "summaryDetail": { "trailingPE": 31.2 },
                "financialData": { "grossMargins": 0.46 }
            } ] }
        });
        let info = merge_quote_summary(&payload).unwrap();
        assert_eq!(info["trailingPE"], 31.2);
        assert!(info.get("regularMarketPrice").is_some());
        assert_eq!(equity_values(&info).len(), 2);
    }
}
