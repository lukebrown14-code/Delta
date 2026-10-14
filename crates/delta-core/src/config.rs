//! Configuration: `.env` secrets + `config.toml` app config.
//! Port of `delta/core/config.py`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const CONFIG_PATH: &str = "config.toml";
pub const ENV_PATH: &str = ".env";

/// Kind assumed for a `[targets.<name>]` table that omits `kind`.
pub const DEFAULT_KIND: &str = "company";
/// Kind of legacy `[watchlists.<name>]` tables in the plugin registry.
pub const LEGACY_KIND: &str = "tickers";

/// Secrets and environment overrides, never committed.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default)]
    pub openrouter_api_key: String,
    #[serde(default)]
    pub openai_api_key: String,
    #[serde(default)]
    pub anthropic_api_key: String,
}

/// One exchange a user can select for targets and Yahoo-backed data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketConfig {
    pub label: String,
    pub currency: String,
    #[serde(default)]
    pub yahoo_suffix: String,
}

pub fn default_markets() -> BTreeMap<String, MarketConfig> {
    [
        (
            "us".to_string(),
            MarketConfig {
                label: "United States".to_string(),
                currency: "USD".to_string(),
                yahoo_suffix: String::new(),
            },
        ),
        (
            "asx".to_string(),
            MarketConfig {
                label: "Australian Securities Exchange".to_string(),
                currency: "AUD".to_string(),
                yahoo_suffix: ".AX".to_string(),
            },
        ),
    ]
    .into_iter()
    .collect()
}

/// The merged view of `config.toml` (loaded lazily).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub base_currency: String,
    pub db_path: String,
    pub reports_dir: String,
    #[serde(default)]
    pub universe: BTreeMap<String, Vec<String>>,
    /// target name -> spec (always carries `kind`).
    #[serde(default)]
    pub targets: BTreeMap<String, Value>,
    pub llm_provider: String,
    pub llm_model: String,
    #[serde(default)]
    pub llm_routing: BTreeMap<String, String>,
    pub llm_max_output_tokens: i64,
    pub llm_base_url: String,
    pub llm_api_key_env: String,
    #[serde(default)]
    pub plugins: BTreeMap<String, Value>,
    pub markets: BTreeMap<String, MarketConfig>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            base_currency: "AUD".to_string(),
            db_path: "data/delta.db".to_string(),
            reports_dir: "reports".to_string(),
            universe: BTreeMap::new(),
            targets: BTreeMap::new(),
            llm_provider: "openrouter".to_string(),
            llm_model: String::new(),
            llm_routing: BTreeMap::new(),
            llm_max_output_tokens: 4096,
            llm_base_url: String::new(),
            llm_api_key_env: String::new(),
            plugins: BTreeMap::new(),
            markets: default_markets(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("failed to parse {path}: {message}")]
    Parse { path: PathBuf, message: String },
}

/// Load a TOML file; a missing file is an empty table (matching Python).
pub fn load_toml(path: &Path) -> Result<BTreeMap<String, Value>, ConfigError> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(BTreeMap::new()),
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    toml::from_str::<BTreeMap<String, Value>>(&String::from_utf8_lossy(&bytes)).map_err(|e| {
        ConfigError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        }
    })
}

/// Load `path`, let `mutator` change the raw TOML, and write it back.
///
/// The one place a config change is persisted (matches `update_config` in
/// Python). Unlike the Python writer (a full `tomli_w` rewrite, D6), the file
/// is edited in place with `toml_edit`: comments and key order of untouched
/// entries survive; only changed/added keys are re-formatted.
pub fn update_config<F>(mutator: F, path: &Path) -> Result<BTreeMap<String, Value>, ConfigError>
where
    F: FnOnce(&mut BTreeMap<String, Value>),
{
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(source) => {
            return Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            });
        }
    };
    let before = if text.is_empty() {
        BTreeMap::new()
    } else {
        toml::from_str(&text).map_err(|e| ConfigError::Parse {
            path: path.to_path_buf(),
            message: e.to_string(),
        })?
    };
    let mut raw = before.clone();
    mutator(&mut raw);

    // Replay only the diff onto the format-preserving document.
    let mut doc: toml_edit::DocumentMut =
        text.parse()
            .map_err(|e: toml_edit::TomlError| ConfigError::Parse {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?;
    apply_map(doc.as_table_mut(), &to_map(&before), &to_map(&raw));
    std::fs::write(path, doc.to_string()).map_err(|source| ConfigError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(raw)
}

fn to_map(map: &BTreeMap<String, Value>) -> serde_json::Map<String, Value> {
    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// Replay the `before -> after` change set onto a `toml_edit` table, so
/// untouched keys keep their comments and ordering. Both maps are the parsed
/// `config.toml`; equal subtrees are skipped, changed values are written in
/// place, removed keys are dropped, and new tables become standard tables.
fn apply_map(
    table: &mut toml_edit::Table,
    before: &serde_json::Map<String, Value>,
    after: &serde_json::Map<String, Value>,
) {
    for key in before.keys() {
        if !after.contains_key(key) {
            table.remove(key);
        }
    }
    for (key, new) in after {
        match (before.get(key), new) {
            (Some(old), n) if old == n => {}
            (Some(Value::Object(old_map)), Value::Object(new_map)) => match table.get_mut(key) {
                Some(toml_edit::Item::Table(sub)) => apply_map(sub, old_map, new_map),
                // Shape changed or the entry is an inline table/array-of-tables:
                // replace wholesale.
                Some(slot) => *slot = json_to_item(new),
                None => {
                    table.insert(key.as_str(), json_to_item(new));
                }
            },
            // Existing value: swap in place, keeping the key's decor (the
            // comment lines above it) and the value's spacing. Going through
            // `Table::insert` would re-format the key and drop its comments.
            (Some(_), _) => match table.get_mut(key) {
                Some(slot @ toml_edit::Item::Value(_)) => {
                    let replacement = json_to_item(new);
                    let decor = slot.as_value().map(toml_edit::Value::decor).cloned();
                    *slot = replacement;
                    if let (Some(v), Some(decor)) = (slot.as_value_mut(), decor) {
                        *v.decor_mut() = decor;
                    }
                }
                Some(slot) => *slot = json_to_item(new),
                None => unreachable!("key is present in `before`"),
            },
            (None, _) => {
                table.insert(key.as_str(), json_to_item(new));
            }
        }
    }
}

/// `serde_json::Value` -> `toml_edit` item; objects become standard tables
/// (`[section]`), everything else keeps its natural scalar/array form.
fn json_to_item(value: &Value) -> toml_edit::Item {
    match value {
        Value::Object(map) => {
            let mut t = toml_edit::Table::new();
            for (k, v) in map {
                t.insert(k.as_str(), json_to_item(v));
            }
            toml_edit::Item::Table(t)
        }
        scalar => toml_edit::value(json_scalar(scalar)),
    }
}

/// A non-object `Value` as a `toml_edit` value (arrays nest inline).
fn json_scalar(value: &Value) -> toml_edit::Value {
    match value {
        Value::Null => toml_edit::Value::from(""),
        Value::Bool(b) => toml_edit::Value::from(*b),
        Value::Number(n) => n
            .as_i64()
            .map(toml_edit::Value::from)
            .unwrap_or_else(|| toml_edit::Value::from(n.as_f64().unwrap_or_default())),
        Value::String(s) => toml_edit::Value::from(s.as_str()),
        Value::Array(items) => {
            let mut array = toml_edit::Array::new();
            for item in items {
                array.push(json_scalar(item));
            }
            toml_edit::Value::from(array)
        }
        Value::Object(map) => {
            let mut inline = toml_edit::InlineTable::new();
            for (k, v) in map {
                inline.insert(k.as_str(), json_scalar(v));
            }
            toml_edit::Value::from(inline)
        }
    }
}

/// Merge `[targets]` and legacy `[watchlists]` tables, then the `[universe]`
/// shim. `[targets]` wins on name collisions; user entries beat shim names.
fn target_tables(raw: &BTreeMap<String, Value>) -> BTreeMap<String, Value> {
    let mut targets = BTreeMap::new();
    let empty = serde_json::Map::new();
    let raw_targets = raw
        .get("targets")
        .and_then(Value::as_object)
        .unwrap_or(&empty);
    for (name, spec) in raw_targets {
        let mut spec = spec.clone();
        if let Some(table) = spec.as_object_mut() {
            table
                .entry("kind".to_string())
                .or_insert_with(|| Value::from(DEFAULT_KIND));
        }
        targets.insert(name.clone(), spec);
    }
    if let Some(watchlists) = raw.get("watchlists").and_then(Value::as_object) {
        for (name, spec) in watchlists {
            if targets.contains_key(name) {
                continue;
            }
            let mut spec = spec.clone();
            if let Some(table) = spec.as_object_mut() {
                table
                    .entry("kind".to_string())
                    .or_insert_with(|| Value::from(LEGACY_KIND));
            }
            targets.insert(name.clone(), spec);
        }
    }
    if let Some(universe) = raw.get("universe").and_then(Value::as_object) {
        for (market, tickers) in universe {
            let market_name = format!("universe_{market}");
            if targets.contains_key(&market_name) {
                continue;
            }
            targets.insert(
                market_name,
                serde_json::json!({
                    "kind": LEGACY_KIND,
                    "market": market,
                    "tickers": tickers,
                    "legacy": true,
                }),
            );
        }
    }
    targets
}

/// Python `str()` coercion for scalar TOML values; missing -> default.
fn get_str(value: Option<&Value>, default: &str) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        _ => default.to_string(),
    }
}

/// Merge the raw TOML into an [`AppConfig`].
pub fn build_config(raw: Option<&BTreeMap<String, Value>>) -> AppConfig {
    let empty = BTreeMap::new();
    let raw = raw.unwrap_or(&empty);
    let mut cfg = AppConfig::default();

    cfg.base_currency = get_str(raw.get("base_currency"), &cfg.base_currency);
    cfg.db_path = get_str(raw.get("db_path"), &cfg.db_path);
    cfg.reports_dir = get_str(raw.get("reports_dir"), &cfg.reports_dir);

    if let Some(u) = raw.get("universe").and_then(Value::as_object) {
        cfg.universe = u
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    v.as_array()
                        .map(|a| {
                            a.iter()
                                .map(|x| match x {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                )
            })
            .collect();
    }
    cfg.targets = target_tables(raw);

    let empty = serde_json::Map::new();
    let llm = raw.get("llm").and_then(Value::as_object).unwrap_or(&empty);
    cfg.llm_provider = get_str(llm.get("provider"), &cfg.llm_provider);
    cfg.llm_model = get_str(llm.get("model"), &cfg.llm_model);
    if let Some(routing) = llm.get("routing").and_then(Value::as_object) {
        cfg.llm_routing = routing
            .iter()
            .map(|(k, v)| {
                (
                    k.clone(),
                    match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    },
                )
            })
            .collect();
    }
    if let Some(Value::Number(n)) = llm.get("max_output_tokens") {
        cfg.llm_max_output_tokens = n.as_i64().unwrap_or(cfg.llm_max_output_tokens);
    }
    cfg.llm_base_url = get_str(llm.get("base_url"), &cfg.llm_base_url);
    cfg.llm_api_key_env = get_str(llm.get("api_key_env"), &cfg.llm_api_key_env);

    if let Some(plugins) = raw.get("plugins").and_then(Value::as_object) {
        cfg.plugins = plugins
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
    }
    if let Some(markets) = raw.get("markets").and_then(Value::as_object) {
        for (name, values) in markets {
            if let Ok(m) = serde_json::from_value::<MarketConfig>(values.clone()) {
                cfg.markets.insert(name.to_lowercase(), m);
            }
        }
    }
    cfg
}

/// Secrets from the environment / `.env` (case-insensitive, matching
/// pydantic-settings). Process env wins over `.env`.
pub fn load_settings(env_path: &Path) -> Settings {
    let mut settings = Settings::default();
    for (field, name) in [
        (&mut settings.openrouter_api_key, "OPENROUTER_API_KEY"),
        (&mut settings.openai_api_key, "OPENAI_API_KEY"),
        (&mut settings.anthropic_api_key, "ANTHROPIC_API_KEY"),
    ] {
        *field = read_env_value_named(name, env_path);
    }
    settings
}

/// `load_config` from Python: `(Settings, AppConfig)` from `path` + `.env`.
pub fn load_config(path: &Path) -> Result<(Settings, AppConfig), ConfigError> {
    let settings = load_settings(Path::new(ENV_PATH));
    let raw = load_toml(path)?;
    Ok((settings, build_config(Some(&raw))))
}

/// Value of `name` from the environment or `.env`; `""` when unset.
///
/// The process environment wins over `.env`. Never raises.
pub fn read_env_value_named(name: &str, env_path: &Path) -> String {
    if name.is_empty() {
        return String::new();
    }
    if let Ok(v) = std::env::var(name) {
        if !v.is_empty() {
            return v;
        }
    }
    let Ok(contents) = std::fs::read_to_string(env_path) else {
        return String::new();
    };
    for line in contents.lines() {
        let stripped = line.trim();
        if let Some(rest) = stripped.strip_prefix(&format!("{name}=")) {
            return rest.trim().trim_matches('"').trim_matches('\'').to_string();
        }
    }
    String::new()
}

/// Write `name=value` to `.env`, replacing an existing line or appending.
///
/// Unrelated lines (and their order) are preserved; the file is created when
/// absent. Secrets stay in `.env` (gitignored), never in config.toml.
pub fn set_env_value(name: &str, value: &str, env_path: &Path) {
    let lines: Vec<String> = std::fs::read_to_string(env_path)
        .map(|c| c.lines().map(str::to_string).collect())
        .unwrap_or_default();
    let mut lines = lines;
    let needle = format!("{name}=");
    if let Some(slot) = lines.iter_mut().find(|l| {
        let t = l.trim_start();
        t.starts_with(&needle)
    }) {
        *slot = format!("{name}={value}");
    } else {
        lines.push(format!("{name}={value}"));
    }
    let _ = std::fs::write(env_path, format!("{}\n", lines.join("\n")));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_config_defaults() {
        let cfg = build_config(None);
        assert_eq!(cfg.base_currency, "AUD");
        assert_eq!(cfg.llm_provider, "openrouter");
        assert_eq!(cfg.markets["asx"].yahoo_suffix, ".AX");
    }

    #[test]
    fn build_config_merges_targets() {
        let raw: BTreeMap<String, Value> = serde_json::from_str(
            r#"{
                "targets": {"quality": {"market": "us", "tickers": ["AAPL"]}},
                "watchlists": {"legacy": {"tickers": ["BHP"]}, "quality": {"tickers": ["X"]}},
                "universe": {"us": ["MSFT"]}
            }"#,
        )
        .unwrap();
        let cfg = build_config(Some(&raw));
        assert_eq!(cfg.targets["quality"]["kind"], "company");
        assert_eq!(cfg.targets["legacy"]["kind"], "tickers");
        assert_eq!(cfg.targets["legacy"]["tickers"][0], "BHP");
        assert_eq!(cfg.targets["universe_us"]["kind"], "tickers");
        assert_eq!(cfg.targets["universe_us"]["legacy"], true);
        // [targets] wins over legacy same-name table.
        assert_eq!(cfg.targets["quality"]["tickers"][0], "AAPL");
    }

    #[test]
    fn build_config_llm_section() {
        let raw: BTreeMap<String, Value> = serde_json::from_str(
            r#"{
                "llm": {
                    "provider": "openai",
                    "model": 123,
                    "routing": {"report": "anthropic/claude"},
                    "max_output_tokens": 8192
                }
            }"#,
        )
        .unwrap();
        let cfg = build_config(Some(&raw));
        assert_eq!(cfg.llm_provider, "openai");
        assert_eq!(cfg.llm_model, "123"); // str() coercion, matching Python
        assert_eq!(cfg.llm_routing["report"], "anthropic/claude");
        assert_eq!(cfg.llm_max_output_tokens, 8192);
    }

    #[test]
    fn env_value_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let env = dir.path().join(".env");
        std::fs::write(&env, "KEEP=1\nFOO=old\n").unwrap();
        set_env_value("FOO", "new", &env);
        set_env_value("BAR", "fresh", &env);
        let text = std::fs::read_to_string(&env).unwrap();
        assert_eq!(text, "KEEP=1\nFOO=new\nBAR=fresh\n");
        assert_eq!(read_env_value_named("FOO", &env), "new");
        assert_eq!(read_env_value_named("MISSING", &env), "");
    }

    #[test]
    fn update_config_preserves_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "base_currency = \"USD\"\n[plugins.sec_edgar]\ncontact = \"a@b.c\"\n",
        )
        .unwrap();
        update_config(
            |raw| {
                raw.insert("base_currency".to_string(), Value::from("AUD"));
            },
            &path,
        )
        .unwrap();
        let raw = load_toml(&path).unwrap();
        assert_eq!(raw["base_currency"], "AUD");
        assert_eq!(raw["plugins"]["sec_edgar"]["contact"], "a@b.c");
    }

    /// D6 (finding rust-core #3): writes are `toml_edit` edits, so comments
    /// and key order of untouched entries survive the round trip.
    #[test]
    fn update_config_preserves_comments_and_key_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let original = "\
# The reporting currency for every portfolio figure.
base_currency = \"USD\"

[llm]
# Which provider serves the default route.
provider = \"openrouter\"
model = \"openai/gpt-4o-mini\"

[plugins.sec_edgar]
# SEC User-Agent rule: a real contact is required.
contact = \"a@b.c\"
";
        std::fs::write(&path, original).unwrap();

        update_config(
            |raw| {
                // Change an existing scalar under a comment.
                if let Some(llm) = raw.get_mut("llm").and_then(Value::as_object_mut) {
                    llm.insert("model".to_string(), Value::from("anthropic/claude"));
                }
                // Add a brand-new nested table.
                raw.insert(
                    "universe".to_string(),
                    serde_json::json!({"us": ["AAPL", "MSFT"]}),
                );
            },
            &path,
        )
        .unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        // Comments on untouched keys survive.
        assert!(text.contains("# The reporting currency for every portfolio figure."));
        assert!(text.contains("# Which provider serves the default route."));
        assert!(text.contains("# SEC User-Agent rule: a real contact is required."));
        // Untouched order preserved: base_currency before [llm] before plugins.
        let base = text.find("base_currency").unwrap();
        let llm = text.find("[llm]").unwrap();
        let model = text.find("model = \"anthropic/claude\"").unwrap();
        let sec = text.find("[plugins.sec_edgar]").unwrap();
        assert!(base < llm && llm < model && model < sec);
        // And the file still parses back to the expected values.
        let raw = load_toml(&path).unwrap();
        assert_eq!(raw["llm"]["model"], "anthropic/claude");
        assert_eq!(raw["llm"]["provider"], "openrouter");
        assert_eq!(raw["universe"]["us"][0], "AAPL");
        assert_eq!(raw["base_currency"], "USD");
    }

    #[test]
    fn update_config_removes_keys_and_new_file_writes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(
            &path,
            "[markets]\n[markets.custom]\nlabel = \"Custom\"\ncurrency = \"EUR\"\n",
        )
        .unwrap();
        update_config(
            |raw| {
                if let Some(markets) = raw.get_mut("markets").and_then(Value::as_object_mut) {
                    markets.remove("custom");
                }
            },
            &path,
        )
        .unwrap();
        let raw = load_toml(&path).unwrap();
        assert!(raw.get("markets").is_none_or(|m| m.get("custom").is_none()));

        // A missing file is created.
        let fresh = dir.path().join("fresh.toml");
        update_config(
            |raw| {
                raw.insert("base_currency".to_string(), Value::from("AUD"));
            },
            &fresh,
        )
        .unwrap();
        assert_eq!(load_toml(&fresh).unwrap()["base_currency"], "AUD");
    }
}
