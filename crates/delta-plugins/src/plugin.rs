//! Plugin base traits, Scope filter and the static registry
//! (port of `delta/core/plugin.py`; entry points become a static registry
//! per the rewrite plan).

use std::collections::BTreeSet;

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

/// One setting an adapter safely exposes to the source-setup UI
/// (`DataProviderField`).
#[derive(Debug, Clone, Copy)]
pub struct DataProviderField {
    pub name: &'static str,
    pub label: &'static str,
    pub required: bool,
    /// Secrets go to `.env`, never to `config.toml`.
    pub secret: bool,
    /// Fixed environment-variable name a secret must declare.
    pub env_var: &'static str,
    pub placeholder: &'static str,
}

/// An adapter-owned setup contract, not a generic HTTP connector
/// (`DataProviderSpec`).
#[derive(Debug, Clone, Copy)]
pub struct DataProviderSpec {
    pub label: &'static str,
    pub fields: &'static [DataProviderField],
    pub primary_disclosure: bool,
    pub notice: &'static str,
}

/// `SECEdgar.provider_spec` — the only plugin with a setup contract today.
pub static SEC_EDGAR_PROVIDER_SPEC: DataProviderSpec = DataProviderSpec {
    label: "SEC EDGAR",
    fields: &[DataProviderField {
        name: "contact",
        label: "Contact email",
        required: true,
        secret: false,
        env_var: "",
        placeholder: "you@example.com",
    }],
    primary_disclosure: true,
    notice: "SEC requires a real contact address in the User-Agent.",
};

/// The setup contract each named plugin declares (`plugin.provider_spec`).
///
/// The per-plugin trait impls live with their plugins; the table sits here so
/// the settings surface can look a spec up by plugin name without touching
/// every plugin module.
pub fn provider_specs() -> std::collections::BTreeMap<String, &'static DataProviderSpec> {
    std::collections::BTreeMap::from([("sec_edgar".to_string(), &SEC_EDGAR_PROVIDER_SPEC)])
}

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
    /// The plugin's setup contract, when it exposes one (`provider_spec`).
    fn provider_spec(&self) -> Option<&'static DataProviderSpec> {
        None
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

#[cfg(test)]
mod tests {
    use super::*;
    use delta_core::models::AssetClass;

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
