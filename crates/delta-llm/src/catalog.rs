//! Model catalog: what each provider offers, plus config.toml route writeback.
//! Port of `delta/llm/catalog.py`.
//!
//! The catalog is best-effort everywhere: a failed fetch or an unwritable
//! cache yields an empty list, and the picker degrades to free-text model
//! entry. Unlike Python (which resolves `data/model_catalog.json` relative to
//! the cwd), the cache path is an explicit argument here.

use std::path::{Path, PathBuf};

use delta_core::config::update_config;
use serde_json::{json, Value};

use crate::providers::{ModelInfo, Provider};

pub const CATALOG_FILENAME: &str = "model_catalog.json";
pub const CACHE_TTL_SECONDS: f64 = 86400.0;

/// The default cache location: `data/model_catalog.json` under `base`.
pub fn catalog_path(base: &Path) -> PathBuf {
    base.join("data").join(CATALOG_FILENAME)
}

fn model_from_json(m: &Value) -> Option<ModelInfo> {
    let id = m["id"].as_str()?;
    Some(ModelInfo {
        id: id.to_string(),
        name: m["name"].as_str().unwrap_or(id).to_string(),
        context_length: m["context_length"].as_i64(),
        prompt_price: m["prompt_price"].as_f64().unwrap_or(0.0),
        completion_price: m["completion_price"].as_f64().unwrap_or(0.0),
    })
}

fn model_to_json(m: &ModelInfo) -> Value {
    json!({
        "id": m.id,
        "name": m.name,
        "context_length": m.context_length,
        "prompt_price": m.prompt_price,
        "completion_price": m.completion_price,
    })
}

/// `(fetched_at, models)` for `provider` from the disk cache, `None` when
/// unusable (`_read_cache_entry`).
pub fn read_cache_entry(path: &Path, provider: &str) -> Option<(f64, Vec<ModelInfo>)> {
    let raw: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let entry = raw.get(provider)?;
    let models = entry["models"]
        .as_array()?
        .iter()
        .filter_map(model_from_json)
        .collect();
    Some((entry["fetched_at"].as_f64()?, models))
}

/// Stamp and store `models` for `provider`, keeping other providers' entries
/// (`_write_cache`). Best-effort: failures are ignored.
pub fn write_cache(path: &Path, provider: &str, models: &[ModelInfo]) {
    let write = || -> Option<()> {
        let mut raw: Value = if path.exists() {
            serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?
        } else {
            json!({})
        };
        let map = raw.as_object_mut()?;
        map.insert(
            provider.to_string(),
            json!({
                "fetched_at": now_secs(),
                "models": models.iter().map(model_to_json).collect::<Vec<_>>(),
            }),
        );
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok()?;
        }
        std::fs::write(path, serde_json::to_string_pretty(&raw).ok()?).ok()?;
        Some(())
    };
    let _ = write();
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// Models for `provider` from the disk cache, whatever their age; `[]` when
/// absent (`cached_catalog`).
pub fn cached_catalog(path: &Path, provider: &str) -> Vec<ModelInfo> {
    read_cache_entry(path, provider)
        .map(|(_, models)| models)
        .unwrap_or_default()
}

/// A provider's models: the disk cache within 24h, else a fetch; never fails.
///
/// A failed fetch must not overwrite a good catalog: writing `[]` here would
/// also stamp a fresh `fetched_at` and hide the real models for a full TTL.
/// Providers report a failure as an empty list, so an empty result is treated
/// the same way and the cache is served instead.
pub async fn catalog(provider: &dyn Provider, path: &Path, force: bool) -> Vec<ModelInfo> {
    let entry = read_cache_entry(path, provider.name());
    if let Some((fetched_at, models)) = &entry {
        if !force && now_secs() - fetched_at < CACHE_TTL_SECONDS {
            return models.clone();
        }
    }
    let models = provider.models(force).await;
    if models.is_empty() {
        if let Some((_, cached)) = entry {
            return cached;
        }
    }
    write_cache(path, provider.name(), &models);
    models
}

/// Write `[llm.routing].<task> = model` to the config at `config_path`
/// (`set_llm_route`). Comments in config.toml are lost on write.
pub fn set_llm_route(config_path: &Path, task: &str, model: &str) -> Result<(), String> {
    update_config(
        |raw| {
            let llm = raw.entry("llm".to_string()).or_insert_with(|| json!({}));
            if let Some(map) = llm.as_object_mut() {
                let routing = map
                    .entry("routing".to_string())
                    .or_insert_with(|| json!({}));
                if let Some(r) = routing.as_object_mut() {
                    r.insert(task.to_string(), Value::String(model.to_string()));
                }
            }
        },
        config_path,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Write `[llm] model = model` — the one model used by every task
/// (`set_llm_model`). Existing `[llm.routing]` entries are left alone.
pub fn set_llm_model(config_path: &Path, model: &str) -> Result<(), String> {
    update_config(
        |raw| {
            let llm = raw.entry("llm".to_string()).or_insert_with(|| json!({}));
            if let Some(map) = llm.as_object_mut() {
                map.insert("model".to_string(), Value::String(model.to_string()));
            }
        },
        config_path,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Write `[llm] provider = name` to the config (`set_llm_provider`).
pub fn set_llm_provider(config_path: &Path, name: &str) -> Result<(), String> {
    update_config(
        |raw| {
            let llm = raw.entry("llm".to_string()).or_insert_with(|| json!({}));
            if let Some(map) = llm.as_object_mut() {
                map.insert("provider".to_string(), Value::String(name.to_string()));
            }
        },
        config_path,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Point the `custom` provider at a self-hosted or other endpoint
/// (`set_llm_custom`). Writes `[llm] provider/base_url/api_key_env`.
pub fn set_llm_custom(config_path: &Path, base_url: &str, api_key_env: &str) -> Result<(), String> {
    update_config(
        |raw| {
            let llm = raw.entry("llm".to_string()).or_insert_with(|| json!({}));
            if let Some(map) = llm.as_object_mut() {
                map.insert("provider".to_string(), Value::String("custom".into()));
                map.insert("base_url".to_string(), Value::String(base_url.into()));
                map.insert("api_key_env".to_string(), Value::String(api_key_env.into()));
            }
        },
        config_path,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Write `[plugins.<plugin_name>].model`; `None` removes the override
/// (`set_plugin_model`). `plugin_name` is the plugin's config table key.
pub fn set_plugin_model(
    config_path: &Path,
    plugin_name: &str,
    model: Option<&str>,
) -> Result<(), String> {
    update_config(
        |raw| {
            let plugins = raw
                .entry("plugins".to_string())
                .or_insert_with(|| json!({}));
            if let Some(map) = plugins.as_object_mut() {
                let table = map
                    .entry(plugin_name.to_string())
                    .or_insert_with(|| json!({}));
                if let Some(spec) = table.as_object_mut() {
                    match model {
                        Some(m) => {
                            spec.insert("model".to_string(), Value::String(m.to_string()));
                        }
                        None => {
                            spec.remove("model");
                        }
                    }
                }
            }
        },
        config_path,
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
