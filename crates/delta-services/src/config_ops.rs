//! Config-facing operations (ports of the `set_plugin_enabled`,
//! `market_profiles`, `save_market`, `remove_market`, `target_specs`,
//! `add_target`, `remove_target` group in `delta/services.py`).

use std::collections::BTreeMap;
use std::path::Path;

use delta_core::config::{load_toml, update_config, AppConfig};
use delta_core::models::AssetClass;
use serde_json::{json, Value};

use crate::error::ServiceError;
use crate::targets::{target_from_spec, WatchTarget, DEFAULT_KIND, KNOWN_KINDS, LEGACY_KIND};

const MARKET_ID: &str = r"^[a-z][a-z0-9_]*$";
const CURRENCY: &str = r"^[A-Z]{3}$";

/// Enable or disable one plugin (`set_plugin_enabled`).
pub fn set_plugin_enabled(config_path: &Path, name: &str, value: bool) -> Result<(), ServiceError> {
    update_config(
        |raw| {
            let table = raw
                .entry("plugins".to_string())
                .or_insert_with(|| json!({}));
            if let Some(map) = table.as_object_mut() {
                let entry = map.entry(name.to_string()).or_insert_with(|| json!({}));
                if let Some(spec) = entry.as_object_mut() {
                    spec.insert("enabled".to_string(), Value::from(value));
                }
            }
        },
        config_path,
    )
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    Ok(())
}

/// All built-in and user-configured exchange profiles in the active TOML
/// (`market_profiles`).
pub fn market_profiles(
    config_path: &Path,
) -> Result<BTreeMap<String, delta_core::config::MarketConfig>, ServiceError> {
    let raw = load_toml(config_path).map_err(|e| ServiceError::invalid(e.to_string()))?;
    let cfg: AppConfig = delta_core::config::build_config(Some(&raw));
    Ok(cfg.markets)
}

/// Create or edit a config-backed exchange profile (`save_market`).
pub fn save_market(
    config_path: &Path,
    name: &str,
    label: &str,
    currency: &str,
    yahoo_suffix: &str,
) -> Result<(), ServiceError> {
    let name = name.trim().to_lowercase();
    let currency = currency.trim().to_uppercase();
    if !regex_ok(MARKET_ID, &name) {
        return Err(ServiceError::invalid(
            "market ID must use lowercase letters, numbers, or underscores".to_string(),
        ));
    }
    if label.trim().is_empty() {
        return Err(ServiceError::invalid("market name is required".to_string()));
    }
    if !regex_ok(CURRENCY, &currency) {
        return Err(ServiceError::invalid(
            "currency must be a three-letter ISO code".to_string(),
        ));
    }
    let raw = load_toml(config_path).map_err(|e| ServiceError::invalid(e.to_string()))?;
    let built_in = !raw
        .get("markets")
        .and_then(Value::as_object)
        .map(|m| m.contains_key(&name))
        .unwrap_or(false);
    if (name == "us" || name == "asx") && built_in {
        return Err(ServiceError::invalid(format!(
            "{name} is built in and cannot be edited"
        )));
    }
    update_config(
        |toml| {
            let markets = toml
                .entry("markets".to_string())
                .or_insert_with(|| json!({}));
            if let Some(map) = markets.as_object_mut() {
                map.insert(
                    name.clone(),
                    json!({
                        "label": label.trim(),
                        "currency": currency,
                        "yahoo_suffix": yahoo_suffix.trim().to_uppercase(),
                    }),
                );
            }
        },
        config_path,
    )
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    Ok(())
}

/// Remove a user-defined market when nothing still depends on it
/// (`remove_market`).
pub fn remove_market(config_path: &Path, name: &str) -> Result<(), ServiceError> {
    let name = name.trim().to_lowercase();
    let raw = load_toml(config_path).map_err(|e| ServiceError::invalid(e.to_string()))?;
    if !raw
        .get("markets")
        .and_then(Value::as_object)
        .map(|m| m.contains_key(&name))
        .unwrap_or(false)
    {
        return Err(ServiceError::invalid(
            "only user-defined markets can be removed".to_string(),
        ));
    }
    // Targets in that market.
    let mut used_by: Vec<String> = Vec::new();
    for section in ["targets", "watchlists"] {
        if let Some(table) = raw.get(section).and_then(Value::as_object) {
            for (target_name, spec) in table {
                let market = spec
                    .get("market")
                    .map(|v| match v {
                        Value::String(s) => s.to_lowercase(),
                        other => other.to_string().to_lowercase(),
                    })
                    .unwrap_or_default();
                if market == name {
                    used_by.push(target_name.clone());
                }
            }
        }
    }
    // Data sources scoped to that market.
    if let Some(plugins) = raw.get("plugins").and_then(Value::as_object) {
        for (source_name, spec) in plugins {
            let values = spec
                .get("scope")
                .and_then(|s| s.get("markets"))
                .map(|v| match v {
                    Value::String(s) => vec![s.to_lowercase()],
                    Value::Array(items) => items
                        .iter()
                        .map(|x| match x {
                            Value::String(s) => s.to_lowercase(),
                            other => other.to_string().to_lowercase(),
                        })
                        .collect(),
                    other => vec![other.to_string().to_lowercase()],
                })
                .unwrap_or_default();
            if values.contains(&name) {
                used_by.push(source_name.clone());
            }
        }
    }
    if !used_by.is_empty() {
        used_by.sort();
        return Err(ServiceError::invalid(format!(
            "{name} is still used by: {}",
            used_by.join(", ")
        )));
    }
    update_config(
        |toml| {
            if let Some(markets) = toml.get_mut("markets").and_then(Value::as_object_mut) {
                markets.remove(&name);
            }
        },
        config_path,
    )
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    Ok(())
}

/// User-facing watch targets from `[targets]` and legacy `[watchlists]`
/// tables (`target_specs`). The `[universe]` shim entries stay hidden.
pub fn target_specs(config_path: &Path) -> Result<BTreeMap<String, WatchTarget>, ServiceError> {
    let raw = load_toml(config_path).map_err(|e| ServiceError::invalid(e.to_string()))?;
    let mut specs: BTreeMap<String, (Value, bool)> = BTreeMap::new();
    if let Some(table) = raw.get("targets").and_then(Value::as_object) {
        for (name, values) in table {
            let mut spec = values.clone();
            if let Some(map) = spec.as_object_mut() {
                map.entry("kind".to_string())
                    .or_insert_with(|| Value::from(DEFAULT_KIND));
            }
            specs.insert(name.clone(), (spec, false));
        }
    }
    if let Some(table) = raw.get("watchlists").and_then(Value::as_object) {
        for (name, values) in table {
            specs
                .entry(name.clone())
                .or_insert_with(|| (values.clone(), true));
        }
    }
    specs
        .into_iter()
        .map(|(name, (spec, legacy))| Ok((name.clone(), target_from_spec(&name, &spec, legacy)?)))
        .collect()
}

/// Whether `name` exists in either targets section (`_existing_target`).
fn existing_target(config_path: &Path, name: &str) -> Result<bool, ServiceError> {
    let raw = load_toml(config_path).map_err(|e| ServiceError::invalid(e.to_string()))?;
    Ok(["targets", "watchlists"].iter().any(|section| {
        raw.get(*section)
            .and_then(Value::as_object)
            .map(|m| m.contains_key(name))
            .unwrap_or(false)
    }))
}

/// Add a watch target (`add_target`).
#[allow(clippy::too_many_arguments)]
pub fn add_target(
    config_path: &Path,
    name: &str,
    kind: &str,
    market: &str,
    tickers: &[String],
    tags: &[String],
    notes: &str,
    label: Option<&str>,
    asset_class: &str,
) -> Result<(), ServiceError> {
    let kind = kind.to_lowercase();
    if !KNOWN_KINDS.contains(&kind.as_str()) {
        return Err(ServiceError::invalid(format!(
            "target {name:?} names unknown kind {kind:?}; known kinds are {}",
            KNOWN_KINDS.join(", ")
        )));
    }
    let known: Vec<String> = market_profiles(config_path)?.keys().cloned().collect();
    let market = market.to_lowercase();
    if !known.contains(&market) {
        return Err(ServiceError::invalid(format!(
            "target {name:?} names market {market:?}; known markets are {}",
            known.join(", ")
        )));
    }
    let tickers: Vec<String> = tickers.iter().map(|t| t.to_uppercase()).collect();
    if kind == "market" {
        if !tickers.is_empty() {
            return Err(ServiceError::invalid(format!(
                "market target {name:?} takes no tickers"
            )));
        }
    } else if tickers.is_empty() {
        return Err(ServiceError::invalid(format!(
            "{kind} target {name:?} requires tickers"
        )));
    }
    if AssetClass::parse(asset_class).is_none() {
        return Err(ServiceError::invalid(format!(
            "unknown asset class {asset_class:?}"
        )));
    }
    if existing_target(config_path, name)? {
        return Err(ServiceError::invalid(format!(
            "target {name:?} already exists"
        )));
    }
    let mut spec = json!({"kind": kind, "market": market, "asset_class": asset_class});
    let obj = spec.as_object_mut().unwrap();
    if kind != "market" {
        obj.insert("tickers".to_string(), json!(tickers));
    }
    if !tags.is_empty() {
        obj.insert("tags".to_string(), json!(tags));
    }
    if !notes.is_empty() {
        obj.insert("notes".to_string(), json!(notes));
    }
    if let Some(label) = label {
        obj.insert("label".to_string(), json!(label));
    }
    let spec: Value = spec;
    update_config(
        |raw| {
            let targets = raw
                .entry("targets".to_string())
                .or_insert_with(|| json!({}));
            if let Some(map) = targets.as_object_mut() {
                map.insert(name.to_string(), spec.clone());
            }
        },
        config_path,
    )
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    Ok(())
}

/// Remove a watch target from whichever section holds it (`remove_target`).
pub fn remove_target(config_path: &Path, name: &str) -> Result<(), ServiceError> {
    let raw = load_toml(config_path).map_err(|e| ServiceError::invalid(e.to_string()))?;
    for section in ["targets", "watchlists"] {
        if raw
            .get(section)
            .and_then(Value::as_object)
            .map(|m| m.contains_key(name))
            .unwrap_or(false)
        {
            update_config(
                |raw| {
                    if let Some(table) = raw.get_mut(section).and_then(Value::as_object_mut) {
                        table.remove(name);
                    }
                },
                config_path,
            )
            .map_err(|e| ServiceError::invalid(e.to_string()))?;
            return Ok(());
        }
    }
    Err(ServiceError::invalid(format!("unknown target: {name}")))
}

/// Placeholder so LEGACY_KIND is referenced from this module's public surface.
pub fn legacy_kind() -> &'static str {
    LEGACY_KIND
}

fn regex_ok(pattern: &str, value: &str) -> bool {
    // Two anchored patterns only; avoid pulling `regex` into this crate.
    match pattern {
        MARKET_ID => {
            let mut chars = value.chars();
            matches!(chars.next(), Some(c) if c.is_ascii_lowercase())
                && value
                    .chars()
                    .skip(1)
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
                && !value.is_empty()
        }
        CURRENCY => value.len() == 3 && value.chars().all(|c| c.is_ascii_uppercase()),
        _ => false,
    }
}
