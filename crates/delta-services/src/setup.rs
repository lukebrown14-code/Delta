//! Setup checks and data-provider configuration.
//! Ports of `services.setup_checks`, `services.data_provider_status` and
//! `services.configure_data_provider` (delta/services.py).
//!
//! Python reaches these through the `Delta` rig; the Rust port takes the
//! same inputs explicitly (settings, config, db, paths) plus the provider
//! spec table, which the settings screen resolves from
//! `delta_plugins::provider_specs()`.

use std::collections::BTreeMap;
use std::path::Path;

use delta_core::config::{read_env_value_named, set_env_value, update_config, AppConfig, Settings};
use delta_core::db::Db;
use delta_plugins::DataProviderSpec;
use rusqlite::OptionalExtension;
use serde_json::{json, Value};

use crate::error::ServiceError;

/// One named, boolean setup check with its remedy (`Check`).
#[derive(Debug, Clone)]
pub struct Check {
    pub name: String,
    pub ok: bool,
    pub fix: String,
}

/// Settings attribute serving each fixed provider's env var; custom reads
/// `.env` (`_ENV_TO_ATTR`).
fn env_attr(env_var: &str) -> Option<&'static str> {
    match env_var {
        "OPENROUTER_API_KEY" => Some("openrouter_api_key"),
        "OPENAI_API_KEY" => Some("openai_api_key"),
        "ANTHROPIC_API_KEY" => Some("anthropic_api_key"),
        _ => None,
    }
}

/// The configured key for `spec`: Settings field, or .env for custom
/// (`_provider_key`).
fn provider_key(settings: &Settings, spec_env_var: &str, env_path: &Path) -> String {
    match env_attr(spec_env_var) {
        Some(attr) => {
            let value = match attr {
                "openrouter_api_key" => &settings.openrouter_api_key,
                "openai_api_key" => &settings.openai_api_key,
                "anthropic_api_key" => &settings.anthropic_api_key,
                _ => "",
            };
            value.trim().to_string()
        }
        None => read_env_value_named(spec_env_var, env_path),
    }
}

/// The home screen's setup checklist (`setup_checks`).
pub fn setup_checks(
    settings: &Settings,
    cfg: &AppConfig,
    db: &Db,
    config_path: &Path,
    env_path: &Path,
) -> Result<Vec<Check>, ServiceError> {
    let provider = cfg.llm_provider.as_str();
    let mut checks: Vec<Check> = Vec::new();
    match delta_llm::providers::provider_spec(provider) {
        None => checks.push(Check {
            name: format!("LLM provider ({provider})"),
            ok: false,
            fix: format!(
                "unknown provider; valid: {}",
                delta_llm::providers::PROVIDERS
                    .iter()
                    .map(|spec| spec.name)
                    .collect::<std::collections::BTreeSet<&str>>()
                    .into_iter()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        }),
        Some(spec) if spec.name == "custom" => {
            let ok = !provider_key(settings, spec.env_var, env_path).is_empty()
                && !cfg.llm_base_url.is_empty();
            checks.push(Check {
                name: format!("LLM provider ({provider})"),
                ok,
                fix: "Press p on the Config screen to connect a custom endpoint".to_string(),
            });
        }
        Some(spec) => {
            checks.push(Check {
                name: format!("LLM provider ({provider})"),
                ok: !provider_key(settings, spec.env_var, env_path).is_empty(),
                fix: format!(
                    "Set {} in .env or press p on the Config screen",
                    spec.env_var
                ),
            });
        }
    }
    checks.push(Check {
        name: "Config file".to_string(),
        ok: config_path.exists(),
        fix: "Create config.toml".to_string(),
    });
    // Never matches; just checks reachability (a missing row is fine —
    // Python's session.get returns None without raising).
    let reachable = db
        .conn()
        .query_row("SELECT id FROM llmcall WHERE id = 'probe'", [], |row| {
            row.get::<_, String>(0)
        })
        .optional()
        .is_ok();
    checks.push(Check {
        name: "Database".to_string(),
        ok: reachable,
        fix: "Check db_path in config.toml".to_string(),
    });
    let contact = cfg
        .plugins
        .get("sec_edgar")
        .and_then(|table| table.get("contact"))
        .and_then(Value::as_str)
        .unwrap_or("");
    checks.push(Check {
        name: "SEC EDGAR contact".to_string(),
        ok: !contact.is_empty(),
        fix: "Set [plugins.sec_edgar].contact".to_string(),
    });
    let health = crate::analytics::data_health(db)?;
    checks.push(Check {
        name: "Price history".to_string(),
        ok: !health.latest_bar.is_empty(),
        fix: "Gather evidence".to_string(),
    });
    Ok(checks)
}

/// One configured data plugin's setup state (`DataProviderStatus`).
#[derive(Debug, Clone)]
pub struct DataProviderStatus {
    pub name: String,
    pub label: String,
    pub configured: bool,
    pub enabled: bool,
    pub primary_disclosure: bool,
    pub notice: String,
}

/// A plugin's live `enabled` flag, as the registry applies it from
/// `[plugins.<name>] enabled` (default true). Python stores `bool(value)`.
fn plugin_enabled(cfg: &AppConfig, name: &str) -> bool {
    match cfg.plugins.get(name).and_then(|t| t.get("enabled")) {
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_none_or(|v| v != 0.0),
        Some(Value::String(s)) => !s.is_empty(),
        Some(Value::Null) | None => true,
        Some(Value::Array(a)) => !a.is_empty(),
        Some(Value::Object(_)) => true,
    }
}

/// Configured data plugins that declare a safe setup contract
/// (`data_provider_status`), sorted by label.
pub fn data_provider_status(
    cfg: &AppConfig,
    env_path: &Path,
    specs: &BTreeMap<String, &'static DataProviderSpec>,
) -> Vec<DataProviderStatus> {
    let mut result: Vec<DataProviderStatus> = Vec::new();
    for (name, spec) in specs {
        let table = cfg.plugins.get(name).cloned().unwrap_or(json!({}));
        let configured = spec.fields.iter().all(|field| {
            !field.required
                || (if field.secret && !field.env_var.is_empty() {
                    !read_env_value_named(field.env_var, env_path).is_empty()
                } else {
                    // bool(table.get(field.name)): missing/None/false are falsy.
                    table
                        .get(field.name)
                        .map(|value| !value.is_null() && value != &Value::Bool(false))
                        .unwrap_or(false)
                })
        });
        result.push(DataProviderStatus {
            name: name.clone(),
            label: spec.label.to_string(),
            configured,
            enabled: plugin_enabled(cfg, name),
            primary_disclosure: spec.primary_disclosure,
            notice: spec.notice.to_string(),
        });
    }
    result.sort_by(|a, b| a.label.cmp(&b.label));
    result
}

/// The Settings screen's provider dot: the provider's key is readable from
/// `.env` (`config.py::_refresh_ai`'s `connected`). A secret is only ever
/// read, never drawn or logged.
pub fn provider_connected(cfg: &AppConfig, env_path: &Path) -> bool {
    delta_llm::providers::provider_spec(cfg.llm_provider.as_str())
        .map(|spec| {
            let env_var = if cfg.llm_api_key_env.is_empty() {
                spec.env_var
            } else {
                &cfg.llm_api_key_env
            };
            !read_env_value_named(env_var, env_path).trim().is_empty()
        })
        .unwrap_or(false)
}

/// Save an LLM provider selection and optional key through the same config
/// and environment write paths used by the Python setup flow. A custom
/// endpoint requires a URL; its key can be empty for local servers.
pub fn save_provider_choice(
    config_path: &Path,
    env_path: &Path,
    name: &str,
    key: &str,
    base_url: &str,
    api_key_env: &str,
) -> Result<(), ServiceError> {
    let spec = delta_llm::providers::provider_spec(name)
        .ok_or_else(|| ServiceError::invalid(format!("unknown provider {name:?}")))?;
    let key = key.trim();
    if key.chars().any(|ch| ch == '\n' || ch == '\r') {
        return Err(ServiceError::invalid("API key must be a single line"));
    }
    if name == "custom" {
        if base_url.trim().is_empty() {
            return Err(ServiceError::invalid("base URL is required"));
        }
        let env_var = if api_key_env.trim().is_empty() {
            spec.env_var
        } else {
            api_key_env.trim()
        };
        if !env_var
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch == '_' || ch.is_ascii_digit())
            || env_var.starts_with(|ch: char| ch.is_ascii_digit())
        {
            return Err(ServiceError::invalid(
                "invalid API key environment variable",
            ));
        }
        delta_llm::catalog::set_llm_custom(config_path, base_url.trim(), env_var)
            .map_err(ServiceError::invalid)?;
        if !key.is_empty() {
            set_env_value(env_var, key, env_path);
        }
    } else {
        delta_llm::catalog::set_llm_provider(config_path, name).map_err(ServiceError::invalid)?;
        if !key.is_empty() {
            set_env_value(spec.env_var, key, env_path);
        }
    }
    Ok(())
}

/// Select the model used for all tasks; the catalog is advisory and a
/// free-text ID remains valid when offline.
pub fn save_model_choice(config_path: &Path, model: &str) -> Result<(), ServiceError> {
    let model = model.trim();
    if model.is_empty() {
        return Err(ServiceError::invalid("model ID is required"));
    }
    delta_llm::catalog::set_llm_model(config_path, model).map_err(ServiceError::invalid)
}

/// Persist adapter settings and activate the source
/// (`configure_data_provider`).
///
/// A secret field must declare its fixed environment-variable name; it goes
/// to `.env`, never to `config.toml`. `markets` set the plugin's market scope
/// when given. The reload of live plugin instances is a runtime concern and
/// stays with the caller.
pub fn configure_data_provider(
    config_path: &Path,
    env_path: &Path,
    specs: &BTreeMap<String, &'static DataProviderSpec>,
    name: &str,
    values: &BTreeMap<String, String>,
    markets: Option<&[String]>,
) -> Result<(), ServiceError> {
    let Some(spec) = specs.get(name) else {
        return Err(ServiceError::invalid(format!(
            "unknown configurable data provider: {name}"
        )));
    };
    let fields: BTreeMap<&str, &delta_plugins::DataProviderField> =
        spec.fields.iter().map(|f| (f.name, f)).collect();
    let mut unknown: Vec<&String> = values
        .keys()
        .filter(|key| !fields.contains_key(key.as_str()))
        .collect();
    unknown.sort();
    if !unknown.is_empty() {
        let names: Vec<String> = unknown.into_iter().map(|s| s.to_string()).collect();
        return Err(ServiceError::invalid(format!(
            "unknown settings for {name}: {}",
            names.join(", ")
        )));
    }
    for (field_name, value) in values {
        let field = fields[field_name.as_str()];
        if field.secret && !value.trim().is_empty() && field.env_var.is_empty() {
            return Err(ServiceError::invalid(format!(
                "{} has no declared environment variable",
                field.label
            )));
        }
        // Python raises this from inside the config mutation (for secret and
        // plain fields alike), before any file is written; validating here
        // has the same effect.
        if field.required && value.trim().is_empty() {
            return Err(ServiceError::invalid(format!(
                "{} is required",
                field.label
            )));
        }
    }
    if let Some(markets) = markets {
        let known = crate::config_ops::market_profiles(config_path)?;
        let mut unknown_markets: Vec<&String> = markets
            .iter()
            .filter(|market| !known.contains_key(market.as_str()))
            .collect();
        unknown_markets.sort();
        if !unknown_markets.is_empty() {
            let names: Vec<String> = unknown_markets.into_iter().map(|s| s.to_string()).collect();
            return Err(ServiceError::invalid(format!(
                "unknown markets: {}",
                names.join(", ")
            )));
        }
    }
    // Secrets are collected now and written to `.env` after the config
    // write, matching Python's ordering.
    let secrets: Vec<(&str, String)> = values
        .iter()
        .filter_map(|(field_name, value)| {
            let field = fields[field_name.as_str()];
            if field.secret && !value.trim().is_empty() {
                Some((field.env_var, value.trim().to_string()))
            } else {
                None
            }
        })
        .collect();
    update_config(
        |raw| {
            let table = raw
                .entry("plugins".to_string())
                .or_insert_with(|| json!({}));
            if let Some(map) = table.as_object_mut() {
                let plugin_table = map.entry(name.to_string()).or_insert_with(|| json!({}));
                if let Some(spec_table) = plugin_table.as_object_mut() {
                    for (field_name, value) in values {
                        let field = fields[field_name.as_str()];
                        let value = value.trim();
                        if field.secret {
                            continue;
                        }
                        if field.required && value.is_empty() {
                            continue; // validated above
                        }
                        spec_table.insert(field.name.to_string(), json!(value));
                    }
                    if let Some(markets) = markets {
                        spec_table
                            .entry("scope".to_string())
                            .or_insert_with(|| json!({}))
                            .as_object_mut()
                            .expect("scope table")
                            .insert("markets".to_string(), json!(markets));
                    }
                    spec_table.insert("enabled".to_string(), Value::Bool(true));
                }
            }
        },
        config_path,
    )
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    for (env_var, value) in secrets {
        delta_core::config::set_env_value(env_var, &value, env_path);
    }
    Ok(())
}
