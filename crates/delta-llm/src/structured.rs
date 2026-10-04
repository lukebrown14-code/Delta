//! Prompt rendering (minijinja over the verbatim `prompts/` directory) and
//! JSON-schema enforced structured calls. Port of `delta/llm/structured.py`.

use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::client::{CacheValidator, CompleteParams, LlmClient, LlmResult};
use crate::json::extract_json;
use crate::providers::ProviderError;

/// Render a prompt template from the crate's `prompts/` directory with strict
/// undefined semantics (matching the Python `StrictUndefined` environment).
pub fn render_prompt(template: &str, vars: &Value) -> Result<String, ProviderError> {
    let mut env = minijinja::Environment::new();
    env.set_undefined_behavior(minijinja::UndefinedBehavior::Strict);
    for (name, text) in crate::prompts::ALL {
        env.add_template(name, text)
            .map_err(|e| ProviderError::Status {
                status: 0,
                body: format!("template {name}: {e}"),
            })?;
    }
    let tpl = env
        .get_template(template)
        .map_err(|e| ProviderError::Status {
            status: 0,
            body: format!("template {template}: {e}"),
        })?;
    tpl.render(vars.clone()).map_err(|e| ProviderError::Status {
        status: 0,
        body: format!("render {template}: {e}"),
    })
}

/// True when `text` parses as JSON and validates against `T`.
///
/// Used to gate the cache: a response that fails this is not a usable hit, so
/// an earlier bad answer is recalled live instead of replaying the same
/// failure every call (the poisoned-JSON loop).
fn valid_as<T: DeserializeOwned>(text: &str) -> bool {
    extract_json(text)
        .ok()
        .and_then(|v| serde_json::from_value::<T>(v).ok())
        .is_some()
}

/// Render the template, call the model, validate the result into `T`.
///
/// On validation failure, re-prompts once with the error appended. Returns the
/// validated object and the [`LlmResult`]. Cached responses are only reused
/// when they still validate, so a bad cached answer is not re-served.
pub async fn structured<T: DeserializeOwned>(
    client: &LlmClient,
    db: &mut delta_core::db::Db,
    task: &str,
    model: &str,
    template: &str,
    vars: &Value,
    schema: Option<&Value>,
) -> Result<(T, LlmResult), ProviderError> {
    let prompt = render_prompt(template, vars)?;
    let prompt_version = template.strip_suffix(".j2").unwrap_or(template);
    let validator = |text: &str| valid_as::<T>(text);
    let validator_ref: CacheValidator<'_> = &validator;

    let params = CompleteParams {
        task,
        model,
        prompt_version,
        prompt: Some(&prompt),
        messages: None,
        response_format: schema,
        cache_validator: Some(validator_ref),
    };
    let result = client.complete(db, params).await?;
    match parse_or_empty::<T>(&result.text) {
        Ok(obj) => Ok((obj, result)),
        Err(err) => {
            let retry_prompt = format!(
                "{prompt}\n\nYour previous answer was invalid JSON. Fix the following errors and \
                 return valid JSON only:\n{err}"
            );
            let params = CompleteParams {
                task,
                model,
                prompt_version,
                prompt: Some(&retry_prompt),
                messages: None,
                response_format: schema,
                cache_validator: Some(validator_ref),
            };
            let result2 = client.complete(db, params).await?;
            let obj = parse_or_empty::<T>(&result2.text)?;
            Ok((obj, result2))
        }
    }
}

fn parse_or_empty<T: DeserializeOwned>(text: &str) -> Result<T, ProviderError> {
    // Parse JSON, or return `{}` so validation reports a clean error.
    let value = extract_json(text).unwrap_or(Value::Object(Default::default()));
    serde_json::from_value::<T>(value).map_err(|e| ProviderError::Validation(e.to_string()))
}
