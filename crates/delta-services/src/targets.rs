//! Watch targets: the things a user follows, not just shares
//! (port of `delta/targets.py`).

use std::collections::BTreeSet;

use delta_core::models::{AssetClass, Instrument};
use serde_json::Value;

use crate::error::ServiceError;

pub const KNOWN_KINDS: &[&str] = &["company", "industry", "market", "sector", "theme"];
/// Kind assumed for a `[targets.<name>]` table that omits `kind`.
pub const DEFAULT_KIND: &str = "company";
/// Kind of legacy `[watchlists.<name>]` tables in the plugin registry.
pub const LEGACY_KIND: &str = "tickers";

/// A thing the user follows, with the tickers that trade it (if any).
#[derive(Debug, Clone, PartialEq)]
pub struct WatchTarget {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub markets: Vec<String>,
    pub tickers: Vec<String>,
    pub tags: BTreeSet<String>,
    pub notes: String,
    pub asset_class: AssetClass,
}

impl WatchTarget {
    /// Instruments this target contributes to the universe.
    pub fn instruments(&self) -> Vec<Instrument> {
        let market = self.markets.first().cloned().unwrap_or_default();
        let currency = if market == "asx" { "AUD" } else { "USD" };
        self.tickers
            .iter()
            .map(|symbol| Instrument {
                id: format!("{}:{symbol}", market.to_uppercase()),
                market: market.clone(),
                symbol: symbol.clone(),
                name: None,
                currency: currency.to_string(),
                sector: None,
                asset_class: self.asset_class,
                watchlists: vec![self.name.clone(), self.id.clone()],
                tags: self.tags.clone(),
                industry: None,
                meta: Default::default(),
            })
            .collect()
    }
}

/// Legacy tables infer their kind from shape when modelled
/// (`_kind_of`): one ticker is a company, several are a theme.
fn kind_of(name: &str, spec: &Value, legacy: bool) -> Result<String, ServiceError> {
    let raw = spec
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    if raw.is_empty() || raw == LEGACY_KIND || (legacy && !KNOWN_KINDS.contains(&raw.as_str())) {
        let tickers = spec
            .get("tickers")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter(|t| !t.as_str().unwrap_or("").is_empty())
                    .count()
            })
            .unwrap_or(0);
        return Ok(if tickers == 1 {
            "company".to_string()
        } else {
            "theme".to_string()
        });
    }
    if !KNOWN_KINDS.contains(&raw.as_str()) {
        return Err(ServiceError::invalid(format!(
            "target {name:?} names unknown kind {raw:?}; known kinds are {}",
            KNOWN_KINDS.join(", ")
        )));
    }
    Ok(raw)
}

/// Build the domain model from a `[targets.<name>]` (or legacy) table
/// (`target_from_spec`).
pub fn target_from_spec(
    name: &str,
    spec: &Value,
    legacy: bool,
) -> Result<WatchTarget, ServiceError> {
    let market = spec
        .get("market")
        .map(|v| match v {
            Value::String(s) => s.to_lowercase(),
            other => other.to_string().to_lowercase(),
        })
        .unwrap_or_default();
    let tickers: Vec<String> = spec
        .get("tickers")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .filter(|t| !t.is_empty())
                .map(|t| t.to_uppercase())
                .collect()
        })
        .unwrap_or_default();
    let tags: BTreeSet<String> = spec
        .get("tags")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|t| match t {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let notes = spec
        .get("notes")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let asset_class = spec
        .get("asset_class")
        .and_then(Value::as_str)
        .and_then(AssetClass::parse)
        .unwrap_or(AssetClass::Equity);
    Ok(WatchTarget {
        id: name.to_string(),
        kind: kind_of(name, spec, legacy)?,
        name: spec
            .get("label")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| name.to_string()),
        markets: if market.is_empty() {
            Vec::new()
        } else {
            vec![market]
        },
        tickers,
        tags,
        notes,
        asset_class,
    })
}
