//! Task name -> model id mapping from config. Port of `delta/llm/router.py`.

use serde_json::Value;

/// Model id for `task`: `[llm] model` first, then plugin, then routing.
///
/// A single `[llm] model` set in config is the one model for all tasks and
/// beats everything, including per-plugin overrides. Without it,
/// `[plugins.<plugin>].model` beats `[llm.routing]`. Strategies must not fall
/// back to hard-coded model literals: a missing route fails loudly rather
/// than silently diverging from config.toml. An empty-string value is treated
/// as unset at every level.
pub fn model_for(
    cfg: &delta_core::config::AppConfig,
    task: &str,
    plugin: Option<&str>,
) -> Result<String, RouterError> {
    if !cfg.llm_model.is_empty() {
        return Ok(cfg.llm_model.clone());
    }
    if let Some(plugin) = plugin {
        let override_model = cfg
            .plugins
            .get(plugin)
            .and_then(Value::as_object)
            .and_then(|spec| spec.get("model"))
            .and_then(Value::as_str)
            .unwrap_or("");
        if !override_model.is_empty() {
            return Ok(override_model.to_string());
        }
    }
    cfg.llm_routing
        .get(task)
        .cloned()
        .ok_or_else(|| RouterError {
            task: task.to_string(),
        })
}

#[derive(Debug, thiserror::Error)]
#[error(
    "no model routed for task {task:?}; set [llm] model or add [llm.routing].{task} to config.toml"
)]
pub struct RouterError {
    /// The task that had no route.
    pub task: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use delta_core::config::AppConfig;

    #[test]
    fn single_model_beats_everything() {
        let mut cfg = AppConfig {
            llm_model: "single".to_string(),
            ..AppConfig::default()
        };
        cfg.llm_routing
            .insert("task".to_string(), "routed".to_string());
        assert_eq!(model_for(&cfg, "task", None).unwrap(), "single");
    }

    #[test]
    fn plugin_override_beats_routing() {
        let mut cfg = AppConfig::default();
        cfg.llm_routing
            .insert("task".to_string(), "routed".to_string());
        cfg.plugins.insert(
            "sec_edgar".to_string(),
            serde_json::json!({"model": "plugin-model"}),
        );
        assert_eq!(
            model_for(&cfg, "task", Some("sec_edgar")).unwrap(),
            "plugin-model"
        );
        assert_eq!(model_for(&cfg, "task", None).unwrap(), "routed");
    }

    #[test]
    fn empty_values_are_unset() {
        let mut cfg = AppConfig {
            llm_model: " ".to_string(),
            ..AppConfig::default()
        };
        cfg.llm_model.clear();
        cfg.plugins
            .insert("p".to_string(), serde_json::json!({"model": ""}));
        assert!(model_for(&cfg, "task", Some("p")).is_err());
    }

    #[test]
    fn missing_route_fails_loudly() {
        let cfg = AppConfig::default();
        let err = model_for(&cfg, "report", None).unwrap_err().to_string();
        assert!(err.contains("[llm.routing].report"), "{err}");
    }
}
