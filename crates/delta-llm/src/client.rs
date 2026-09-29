//! Provider-agnostic LLM client: caching, cost + latency logging.
//! Port of `delta/llm/client.py`.

use std::sync::Arc;
use std::time::Instant;

use chrono::Utc;
use delta_core::db::Db;
use delta_core::ids::stable_id;
use delta_core::models::LlmCall;
use serde_json::Value;
use uuid::Uuid;

use crate::providers::{
    CompletionRequest, Message, OpenAiCompatProvider, OpenRouterProvider, Provider, ProviderError,
    ProviderSpec, PROVIDERS,
};

pub const SYSTEM_PROMPT: &str = "You are an investment analyst.";

/// A predicate deciding whether a cached response is still a valid hit. When
/// given, a cached response failing the check is treated as a miss (and never
/// re-served), so a previously-poisoned key is re-attempted live rather than
/// replaying the same failure on every call.
pub type CacheValidator<'a> = &'a dyn Fn(&str) -> bool;

/// Arguments for [`LlmClient::complete`] / [`LlmClient::chat`].
#[derive(Clone, Copy, Default)]
pub struct CompleteParams<'a> {
    pub task: &'a str,
    pub model: &'a str,
    pub prompt_version: &'a str,
    /// A single user turn, wrapped with the system prompt.
    pub prompt: Option<&'a str>,
    /// The full transcript (takes precedence over `prompt`).
    pub messages: Option<&'a [Message]>,
    pub response_format: Option<&'a Value>,
    pub cache_validator: Option<CacheValidator<'a>>,
}

#[derive(Debug, Clone)]
pub struct LlmResult {
    pub text: String,
    pub call_id: String,
    pub cost_usd: f64,
    pub cached: bool,
}

pub struct LlmClient {
    provider: Arc<dyn Provider>,
}

impl LlmClient {
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        Self { provider }
    }

    pub fn provider_name(&self) -> &'static str {
        self.provider.name()
    }

    /// Cache key: `stable_id(model, prompt_version, prompt)`.
    pub fn prompt_hash(model: &str, prompt_version: &str, prompt: &str) -> String {
        stable_id(&[model, prompt_version, prompt])
    }

    /// Run one completion, caching the result by prompt hash.
    ///
    /// Either `prompt` (a single user turn, wrapped with the system prompt) or
    /// `messages` (the full transcript) is required. Cache hits are logged as
    /// `cached=true` rows so every call — live or replayed — is accounted for.
    /// When `cache_validator` is supplied, a cached response that fails it is a
    /// miss, so invalid output is never re-served.
    pub async fn complete(
        &self,
        db: &mut Db,
        params: CompleteParams<'_>,
    ) -> Result<LlmResult, ProviderError> {
        let (messages, prompt_text) = match params.messages {
            Some(m) => (m.to_vec(), transcript(m)),
            None => (
                vec![
                    Message::new("system", SYSTEM_PROMPT),
                    Message::new("user", params.prompt.unwrap_or("")),
                ],
                params.prompt.unwrap_or("").to_string(),
            ),
        };

        let phash = Self::prompt_hash(params.model, params.prompt_version, &prompt_text);

        if let Ok(Some(cached)) = db.llm_lookup(&phash) {
            let response = cached.response.clone().unwrap_or_default();
            let accepted = params.cache_validator.is_none_or(|valid| valid(&response));
            if accepted {
                db.llm_store_call(&LlmCall {
                    id: Uuid::new_v4().simple().to_string(),
                    ts: Utc::now().naive_utc(),
                    task: params.task.to_string(),
                    model: params.model.to_string(),
                    prompt_version: params.prompt_version.to_string(),
                    prompt_hash: phash,
                    input_tokens: 0,
                    output_tokens: 0,
                    cost_usd: 0.0,
                    latency_ms: 0,
                    cached: true,
                    response: cached.response,
                })
                .map_err(db_error)?;
                return Ok(LlmResult {
                    text: response,
                    call_id: String::new(),
                    cost_usd: 0.0,
                    cached: true,
                });
            }
        }

        let started = Instant::now();
        let result = self
            .provider
            .complete(CompletionRequest {
                model: params.model,
                messages: &messages,
                response_format: params.response_format,
                max_tokens: None,
            })
            .await?;
        let latency_ms = started.elapsed().as_millis() as i64;

        let call_id = Uuid::new_v4().simple().to_string();
        db.llm_store_call(&LlmCall {
            id: call_id.clone(),
            ts: Utc::now().naive_utc(),
            task: params.task.to_string(),
            model: params.model.to_string(),
            prompt_version: params.prompt_version.to_string(),
            prompt_hash: phash,
            input_tokens: result.input_tokens,
            output_tokens: result.output_tokens,
            cost_usd: result.cost_usd,
            latency_ms,
            cached: false,
            response: Some(result.text.clone()),
        })
        .map_err(db_error)?;

        Ok(LlmResult {
            text: result.text,
            call_id,
            cost_usd: result.cost_usd,
            cached: false,
        })
    }

    /// Full-history chat completion; a thin alias for [`LlmClient::complete`].
    pub async fn chat(
        &self,
        db: &mut Db,
        mut params: CompleteParams<'_>,
    ) -> Result<LlmResult, ProviderError> {
        params.prompt = None;
        self.complete(db, params).await
    }
}

fn db_error(e: delta_core::db::DbError) -> ProviderError {
    ProviderError::Status {
        status: 0,
        body: e.to_string(),
    }
}

fn transcript(messages: &[Message]) -> String {
    messages
        .iter()
        .map(|m| format!("{}: {}", m.role, m.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// Construct an [`LlmClient`] from a provider name + credentials.
///
/// `api_keys` maps env-var names (see [`PROVIDERS`]) to values, keeping
/// secrets out of config.toml. `custom_*` carry the custom provider's
/// `[llm] base_url` / `api_key_env` overrides. Unknown provider names fail
/// loudly with the valid options.
pub fn build_client(
    provider: &str,
    api_keys: &dyn Fn(&str) -> String,
    timeout: std::time::Duration,
    max_output_tokens: Option<i64>,
    custom_base_url: &str,
    custom_api_key_env: &str,
) -> Result<LlmClient, ProviderError> {
    let spec: &ProviderSpec = crate::providers::provider_spec(provider).ok_or_else(|| {
        let mut names: Vec<&str> = PROVIDERS.iter().map(|s| s.name).collect();
        names.sort_unstable();
        ProviderError::Status {
            status: 0,
            body: format!(
                "unknown llm provider {provider:?}; valid: {}",
                names.join(", ")
            ),
        }
    })?;
    let p: Arc<dyn Provider> = if spec.kind == "openrouter" {
        Arc::new(OpenRouterProvider::new(
            &api_keys(spec.env_var),
            timeout,
            max_output_tokens,
            "delta",
            "https://github.com/lukebrown14-code/Delta",
        )?)
    } else {
        let is_custom = spec.name == "custom";
        let base_url = if is_custom { custom_base_url } else { "" };
        let env_var = if is_custom && !custom_api_key_env.is_empty() {
            custom_api_key_env
        } else {
            spec.env_var
        };
        Arc::new(OpenAiCompatProvider::new(
            spec,
            &api_keys(env_var),
            timeout,
            max_output_tokens,
            base_url,
        )?)
    };
    Ok(LlmClient::new(p))
}
