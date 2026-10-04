//! Plugin base traits, Scope filter and the static registry
//! (port of `delta/core/plugin.py`; entry points become a static registry
//! per the rewrite plan).

use std::collections::BTreeSet;
use std::sync::Arc;

use chrono::NaiveDateTime;
use delta_core::db::StoreItem;
use delta_core::models::Instrument;
use serde_json::Value;

/// Declarative filter over watch targets.
///
/// Four axes — targets, asset_classes, markets, tags — are ANDed together;
/// within a single axis the values are ORed. A `None` axis means "no
/// restriction", so the default `Scope` matches everything.
#[derive(Debug, Clone, Default)]
pub struct Scope {
    pub targets: Option<BTreeSet<String>>,
    pub asset_classes: Option<BTreeSet<String>>,
    pub markets: Option<BTreeSet<String>>,
    pub tags: Option<BTreeSet<String>>,
}

fn freeze(values: Option<&Value>) -> Option<BTreeSet<String>> {
    match values {
        None | Some(Value::Null) => None,
        Some(Value::String(s)) => Some(BTreeSet::from([s.clone()])),
        Some(Value::Array(items)) => {
            let set: BTreeSet<String> = items
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
            if set.is_empty() {
                None
            } else {
                Some(set)
            }
        }
        Some(other) => Some(BTreeSet::from([other.to_string()])),
    }
}

fn string_list(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::String(s)) => vec![s.to_lowercase()],
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| match v {
                Value::String(s) => s.to_lowercase(),
                other => other.to_string().to_lowercase(),
            })
            .collect(),
        Some(other) => vec![other.to_string().to_lowercase()],
        None => Vec::new(),
    }
}

impl Scope {
    pub fn matches(&self, inst: &Instrument) -> bool {
        if let Some(targets) = &self.targets {
            if inst.watchlists.iter().all(|w| !targets.contains(w)) {
                return false;
            }
        }
        if let Some(classes) = &self.asset_classes {
            if !classes.contains(inst.asset_class.as_str()) {
                return false;
            }
        }
        if let Some(markets) = &self.markets {
            if !markets.contains(&inst.market) {
                return false;
            }
        }
        if let Some(tags) = &self.tags {
            if inst.tags.iter().all(|t| !tags.contains(t)) {
                return false;
            }
        }
        true
    }

    pub fn filter(&self, instruments: &[Instrument]) -> Vec<Instrument> {
        instruments
            .iter()
            .filter(|i| self.matches(i))
            .cloned()
            .collect()
    }
}

/// Build a `Scope` from a `scope` table, folding `market` in (the `targets`
/// axis also answers to its legacy `watchlists` key).
pub fn parse_scope(raw: Option<&Value>, market: Option<&str>) -> Scope {
    let empty = serde_json::Map::new();
    let table = raw.and_then(Value::as_object).unwrap_or(&empty);
    let mut markets = string_list(table.get("markets"));
    if markets.is_empty() {
        if let Some(market) = market {
            markets = vec![market.to_string()];
        }
    }
    Scope {
        targets: freeze(table.get("targets").or_else(|| table.get("watchlists"))),
        asset_classes: freeze(table.get("asset_classes")),
        markets: if markets.is_empty() {
            None
        } else {
            Some(markets.into_iter().collect())
        },
        tags: freeze(table.get("tags")),
    }
}

/// One gathered row (bars / news / fundamentals / events).
pub type Rows = Vec<StoreItem>;

/// A data source plugin (port of `DataPlugin`).
#[async_trait::async_trait]
pub trait DataPlugin: Send + Sync {
    /// Unique, snake_case.
    fn name(&self) -> &'static str;
    /// `None` = works for any market.
    fn market(&self) -> Option<&'static str> {
        None
    }
    /// Receives its `[plugins.<name>]` TOML table (already merged with
    /// `shared_config` defaults by the registry's `apply_config`).
    fn configure(&mut self, _cfg: &Value) {}
    fn scope(&self) -> Scope {
        Scope::default()
    }
    fn universe(&self, universe: &[Instrument]) -> Vec<Instrument> {
        self.scope().filter(universe)
    }
    async fn fetch(
        &self,
        instruments: &[Instrument],
        since: NaiveDateTime,
    ) -> Result<Rows, PluginError>;
}

#[derive(Debug, thiserror::Error)]
pub enum PluginError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("plugin {plugin}: {message}")]
    Other {
        plugin: &'static str,
        message: String,
    },
}

/// The static registry (replaces Python's entry-point discovery).
pub fn default_plugins() -> Vec<Box<dyn DataPlugin>> {
    vec![
        Box::new(crate::rss::RssData::default()),
        Box::new(crate::asx::AsxAnnouncements::default()),
        Box::new(crate::sec::SecEdgar::default()),
        Box::new(crate::yahoo::YfinanceBars::default()),
        Box::new(crate::calendar::YfinanceCalendar::default()),
    ]
}

/// Instantiate enabled built-in data sources with their configured scope and
/// source-specific settings. The live TUI must use this instead of defaults.
pub fn configured_plugins(cfg: &delta_core::config::AppConfig) -> Vec<Arc<dyn DataPlugin>> {
    default_plugins()
        .into_iter()
        .filter_map(|mut plugin| {
            let table = cfg.plugins.get(plugin.name());
            if table
                .and_then(|value| value.get("enabled"))
                .and_then(Value::as_bool)
                == Some(false)
            {
                return None;
            }
            let mut settings = table
                .cloned()
                .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
            if matches!(plugin.name(), "yfinance" | "yfinance_calendar") {
                let suffixes = crate::yahoo::configured_suffixes(cfg, plugin.name());
                if let Some(object) = settings.as_object_mut() {
                    object.insert("suffixes".into(), serde_json::json!(suffixes));
                }
            }
            plugin.configure(&settings);
            let scope = parse_scope(settings.get("scope"), plugin.market());
            Some(Arc::new(ConfiguredPlugin { plugin, scope }) as Arc<dyn DataPlugin>)
        })
        .collect()
}

struct ConfiguredPlugin {
    plugin: Box<dyn DataPlugin>,
    scope: Scope,
}

#[async_trait::async_trait]
impl DataPlugin for ConfiguredPlugin {
    fn name(&self) -> &'static str {
        self.plugin.name()
    }

    fn market(&self) -> Option<&'static str> {
        self.plugin.market()
    }

    fn universe(&self, universe: &[Instrument]) -> Vec<Instrument> {
        self.scope.filter(&self.plugin.universe(universe))
    }

    async fn fetch(
        &self,
        instruments: &[Instrument],
        since: NaiveDateTime,
    ) -> Result<Rows, PluginError> {
        self.plugin.fetch(instruments, since).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delta_core::models::AssetClass;

    #[test]
    fn configured_registry_respects_enabled_and_scope() {
        let mut cfg = delta_core::config::AppConfig::default();
        cfg.plugins.insert(
            "rss".to_string(),
            serde_json::json!({
                "enabled": true,
                "feeds": ["https://example.test/feed"],
                "scope": {"markets": ["us"]}
            }),
        );
        cfg.plugins.insert(
            "sec_edgar".to_string(),
            serde_json::json!({"enabled": false}),
        );
        let plugins = configured_plugins(&cfg);
        assert!(plugins.iter().any(|plugin| plugin.name() == "rss"));
        assert!(!plugins.iter().any(|plugin| plugin.name() == "sec_edgar"));
        let rss = plugins
            .iter()
            .find(|plugin| plugin.name() == "rss")
            .unwrap();
        let us = inst(&[], AssetClass::Equity, "us", &[]);
        let asx = inst(&[], AssetClass::Equity, "asx", &[]);
        let scoped = rss.universe(&[us.clone(), asx]);
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, us.id);
    }

    fn inst(watchlists: &[&str], class: AssetClass, market: &str, tags: &[&str]) -> Instrument {
        Instrument {
            id: format!("{market}:X"),
            market: market.to_string(),
            symbol: "X".to_string(),
            name: None,
            currency: "AUD".to_string(),
            sector: None,
            asset_class: class,
            watchlists: watchlists.iter().map(|s| s.to_string()).collect(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
            industry: None,
            meta: Default::default(),
        }
    }

    #[test]
    fn scope_axes_are_anded_within_axis_ored() {
        let scope = Scope {
            targets: Some(BTreeSet::from(["quality".to_string(), "core".to_string()])),
            markets: Some(BTreeSet::from(["us".to_string()])),
            ..Default::default()
        };
        let a = inst(&["quality"], AssetClass::Equity, "us", &[]);
        let b = inst(&["quality"], AssetClass::Equity, "asx", &[]);
        let c = inst(&["other"], AssetClass::Equity, "us", &[]);
        assert!(scope.matches(&a));
        assert!(!scope.matches(&b));
        assert!(!scope.matches(&c));
        // Default scope matches everything.
        assert!(Scope::default().matches(&b));
    }

    #[test]
    fn parse_scope_folds_market_and_legacy_key() {
        let raw = serde_json::json!({"watchlists": ["core"], "asset_classes": "etf"});
        let scope = parse_scope(Some(&raw), Some("asx"));
        assert_eq!(scope.targets, Some(BTreeSet::from(["core".to_string()])));
        assert_eq!(
            scope.asset_classes,
            Some(BTreeSet::from(["etf".to_string()]))
        );
        assert_eq!(scope.markets, Some(BTreeSet::from(["asx".to_string()])));

        let explicit = parse_scope(Some(&serde_json::json!({"markets": ["US"]})), None);
        assert_eq!(explicit.markets, Some(BTreeSet::from(["us".to_string()])));
    }
}
