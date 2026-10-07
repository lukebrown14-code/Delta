//! LLM providers: OpenRouter plus any OpenAI-compatible endpoint.
//! Port of `delta/llm/providers.py`.
//!
//! Finding (rust-llm): Python delegates the HTTP call to the `openai` SDK; the
//! Rust port speaks the chat-completions protocol directly with `reqwest`
//! (no official Rust SDK exists — an accepted rewrite trade-off). The wire
//! protocol, headers, retry and auto-fit behaviour match the Python paths —
//! including the SDK's `max_retries` retry policy, ported as [`RETRY_MAX_ATTEMPTS`]
//! attempts on 429/5xx/timeouts with exponential backoff, jitter and
//! `Retry-After` support (decision D7).

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::{json, Value};

pub const OPENROUTER_BASE_URL: &str = "https://openrouter.ai/api/v1";

/// Credit-balance cache lifetime; auto-fit reads `/api/v1/credits` at most this often.
pub const CREDITS_TTL_SECONDS: f64 = 60.0;

/// OpenRouter pre-reserves `prompt + max_tokens * completion_price` against
/// the credit balance. Below this cap a structured report cannot fit.
pub const MAX_TOKENS_FLOOR: i64 = 512;

/// Safety margin on the balance: the prompt-cost estimate is a chars/4
/// heuristic, so auto-fit only commits 95% of the visible remainder.
const BALANCE_MARGIN: f64 = 0.95;

const PRICING_TTL_SECONDS: f64 = 86400.0;

/// Total attempts per chat-completion call (the `openai` SDK's
/// `max_retries=5` port; D7). 429s, 5xx and transport timeouts are retried;
/// other statuses fail on the first attempt.
pub const RETRY_MAX_ATTEMPTS: u32 = 5;

/// Exponential backoff: `RETRY_BASE_DELAY * 2^(attempt-1)`, capped.
pub const RETRY_BASE_DELAY: Duration = Duration::from_millis(500);
pub const RETRY_MAX_DELAY: Duration = Duration::from_secs(8);
/// Uniform jitter added on top of each computed backoff.
pub const RETRY_JITTER: Duration = Duration::from_millis(250);

/// The pause between attempt `attempt` (1-based) and the next one.
///
/// `retry_after` (the parsed `Retry-After` header, seconds) takes precedence
/// over the computed exponential backoff; jitter is added only to backoff.
pub fn retry_delay(attempt: u32, retry_after: Option<f64>) -> Duration {
    if let Some(seconds) = retry_after.filter(|s| *s >= 0.0 && s.is_finite()) {
        return Duration::from_secs_f64(seconds);
    }
    let factor = 2u32.saturating_pow(attempt.saturating_sub(1));
    let backoff = RETRY_BASE_DELAY.saturating_mul(factor).min(RETRY_MAX_DELAY);
    backoff + Duration::from_millis(jitter_ms())
}

/// Bounded jitter without a `rand` dependency: hash the clock's nanoseconds.
fn jitter_ms() -> u64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| u64::from(d.subsec_nanos()))
        .unwrap_or(0);
    let mixed = nanos.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 33;
    mixed % (RETRY_JITTER.as_millis() as u64 + 1)
}

/// The sleep between retry attempts: a seam so tests observe backoff instead
/// of waiting for it (D7). Production uses [`TokioSleep`].
#[async_trait::async_trait]
pub trait RetrySleep: Send + Sync {
    async fn sleep(&self, delay: Duration);
}

/// Production sleeper: park the task on the tokio timer.
pub struct TokioSleep;

#[async_trait::async_trait]
impl RetrySleep for TokioSleep {
    async fn sleep(&self, delay: Duration) {
        tokio::time::sleep(delay).await;
    }
}

/// Whether `status` is worth another attempt (D7 mirrors the SDK: throttling
/// and server-side faults are transient, client errors are not).
fn retryable_status(status: u16) -> bool {
    status == 429 || status >= 500
}

#[derive(Debug, Clone)]
pub struct ProviderResult {
    pub text: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
}

#[derive(Debug, Clone)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub context_length: Option<i64>,
    pub prompt_price: f64,
    pub completion_price: f64,
}

#[derive(Debug, Clone)]
pub struct ProviderSpec {
    pub name: &'static str,
    pub base_url: &'static str,
    pub env_var: &'static str,
    /// `openai-compat` covers OpenAI, Anthropic's compat layer and self-hosted
    /// servers; `openrouter` gets its own impl. `verify_path` is the cheap
    /// authenticated GET used to prove a key works.
    pub kind: &'static str,
    pub verify_path: &'static str,
}

pub const PROVIDERS: &[ProviderSpec] = &[
    ProviderSpec {
        name: "openrouter",
        base_url: OPENROUTER_BASE_URL,
        env_var: "OPENROUTER_API_KEY",
        kind: "openrouter",
        verify_path: "/credits",
    },
    ProviderSpec {
        name: "openai",
        base_url: "https://api.openai.com/v1",
        env_var: "OPENAI_API_KEY",
        kind: "openai-compat",
        verify_path: "/models",
    },
    ProviderSpec {
        name: "anthropic",
        base_url: "https://api.anthropic.com/v1",
        env_var: "ANTHROPIC_API_KEY",
        kind: "openai-compat",
        verify_path: "/models",
    },
    ProviderSpec {
        name: "custom",
        base_url: "",
        env_var: "CUSTOM_API_KEY",
        kind: "openai-compat",
        verify_path: "/models",
    },
];

pub fn provider_spec(name: &str) -> Option<&'static ProviderSpec> {
    PROVIDERS.iter().find(|spec| spec.name == name)
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("{0} provider requires a base URL; set [llm] base_url in config.toml")]
    MissingBaseUrl(&'static str),
    #[error("{0} is not set.")]
    MissingKey(String),
    #[error("provider request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("provider returned status {status}: {body}")]
    Status { status: u16, body: String },
    #[error(
        "OpenRouter credits ({balance}) cannot cover this request{detail}. Add credits at \
         https://openrouter.ai/settings/credits, lower [llm] max_output_tokens in config.toml, \
         or route a cheaper model in [llm.routing]."
    )]
    Budget { balance: String, detail: String },
}

impl ProviderError {
    pub fn is_status(&self, code: u16) -> bool {
        matches!(self, ProviderError::Status { status, .. } if *status == code)
    }
}

/// A message in a completion request (`role` + `content`).
#[derive(Debug, Clone, serde::Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    pub content: String,
}

impl Message {
    pub fn new(role: &str, content: &str) -> Self {
        Self {
            role: role.to_string(),
            content: content.to_string(),
        }
    }
}

/// Auth headers for `spec`'s endpoints; empty when unauthenticated.
///
/// Anthropic's native API authenticates with `x-api-key`, not Bearer; local
/// servers (Ollama) need no header at all.
pub fn auth_headers(spec: &ProviderSpec, api_key: &str) -> BTreeMap<String, String> {
    let mut headers = BTreeMap::new();
    if api_key.is_empty() {
        return headers;
    }
    headers.insert("Authorization".to_string(), format!("Bearer {api_key}"));
    if spec.name == "anthropic" {
        headers.insert("x-api-key".to_string(), api_key.to_string());
        headers.insert("anthropic-version".to_string(), "2023-06-01".to_string());
    }
    headers
}

/// Everything needed to perform one chat completion.
#[derive(Debug, Clone, Default)]
pub struct CompletionRequest<'a> {
    pub model: &'a str,
    pub messages: &'a [Message],
    pub response_format: Option<&'a Value>,
    pub max_tokens: Option<i64>,
}

/// The provider abstraction: one raw chat-completion call normalised into
/// text, token counts, and a USD cost estimate.
#[async_trait::async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;

    async fn complete(&self, req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError>;

    /// Models this provider offers. Never errors: an empty catalog degrades to
    /// free-text model entry.
    async fn models(&self, force: bool) -> Vec<ModelInfo> {
        let _ = force;
        Vec::new()
    }
}

/// One connection to any OpenAI chat-completions compatible endpoint.
pub struct OpenAiCompatProvider {
    pub spec: &'static ProviderSpec,
    pub base_url: String,
    pub api_key: String,
    timeout: Duration,
    max_tokens: Option<i64>,
    /// Additional request headers (OpenRouter app attribution).
    pub extra: BTreeMap<String, String>,
    client: reqwest::Client,
    /// Retry-backoff seam; tests record pauses instead of sleeping (D7).
    pub sleep: Arc<dyn RetrySleep>,
}

impl OpenAiCompatProvider {
    pub fn new(
        spec: &'static ProviderSpec,
        api_key: &str,
        timeout: Duration,
        max_tokens: Option<i64>,
        base_url: &str,
    ) -> Result<Self, ProviderError> {
        let base_url = if base_url.is_empty() {
            spec.base_url
        } else {
            base_url
        };
        Ok(Self {
            spec,
            base_url: base_url.to_string(),
            api_key: api_key.to_string(),
            timeout,
            max_tokens,
            extra: BTreeMap::new(),
            client: reqwest::Client::new(),
            sleep: Arc::new(TokioSleep),
        })
    }

    fn extra_headers(&self) -> BTreeMap<String, String> {
        self.extra.clone()
    }

    fn build_body(&self, req: &CompletionRequest<'_>, max_tokens: Option<i64>) -> Value {
        let mut body = json!({
            "model": req.model,
            "messages": req.messages,
        });
        let max_tokens = max_tokens.or(self.max_tokens);
        if let Some(mt) = max_tokens {
            body["max_tokens"] = json!(mt);
        }
        if let Some(rf) = req.response_format {
            body["response_format"] = rf.clone();
        }
        body
    }

    /// POST the chat-completion body, retrying 429/5xx/timeout failures
    /// (D7): at most [`RETRY_MAX_ATTEMPTS`] attempts, exponential backoff
    /// with jitter, and a server `Retry-After` honoured verbatim. Other
    /// statuses — 402 credit-budget rejections among them — fail on the
    /// first attempt so OpenRouter's refit path stays unchanged.
    async fn post_chat(&self, body: &Value) -> Result<Value, ProviderError> {
        if self.base_url.is_empty() {
            return Err(ProviderError::MissingBaseUrl(self.spec.name));
        }
        let mut attempt = 0;
        loop {
            attempt += 1;
            let mut builder = self
                .client
                .post(format!(
                    "{}/chat/completions",
                    self.base_url.trim_end_matches('/')
                ))
                .timeout(self.timeout)
                .json(body);
            for (k, v) in auth_headers(self.spec, &self.api_key).chain(self.extra_headers()) {
                builder = builder.header(k, v);
            }
            match builder.send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if status.is_success() {
                        return Ok(resp.json().await?);
                    }
                    let retry_after = retry_after_seconds(resp.headers());
                    let err = ProviderError::Status {
                        status: status.as_u16(),
                        body: error_body(resp).await,
                    };
                    if attempt >= RETRY_MAX_ATTEMPTS || !retryable_status(status.as_u16()) {
                        return Err(err);
                    }
                    self.sleep.sleep(retry_delay(attempt, retry_after)).await;
                }
                Err(err) => {
                    let retryable = err.is_timeout() || err.is_connect();
                    let err = ProviderError::Http(err);
                    if attempt >= RETRY_MAX_ATTEMPTS || !retryable {
                        return Err(err);
                    }
                    self.sleep.sleep(retry_delay(attempt, None)).await;
                }
            }
        }
    }

    /// Normalise a chat-completion response into text/tokens/cost.
    fn result(&self, resp: &Value, model: &str) -> ProviderResult {
        let text = resp["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let input_tokens = resp["usage"]["prompt_tokens"].as_i64().unwrap_or(0);
        let output_tokens = resp["usage"]["completion_tokens"].as_i64().unwrap_or(0);
        ProviderResult {
            text,
            input_tokens,
            output_tokens,
            cost_usd: self.compute_cost(model, input_tokens, output_tokens),
        }
    }

    /// No pricing table for generic compat providers.
    fn compute_cost(&self, _model: &str, _in: i64, _out: i64) -> f64 {
        0.0
    }

    /// GET {base_url}/models; ids only, prices unknown (0.0), never errors.
    pub async fn fetch_models(&self) -> Vec<ModelInfo> {
        if self.base_url.is_empty() {
            return Vec::new();
        }
        let mut builder = self
            .client
            .get(format!("{}/models", self.base_url.trim_end_matches('/')))
            .timeout(Duration::from_secs(15));
        for (k, v) in auth_headers(self.spec, &self.api_key) {
            builder = builder.header(k, v);
        }
        let Ok(resp) = builder.send().await else {
            return Vec::new();
        };
        let Ok(value) = resp.json::<Value>().await else {
            return Vec::new();
        };
        value["data"]
            .as_array()
            .map(|items| {
                items
                    .iter()
                    .filter_map(|m| {
                        let id = m["id"].as_str()?;
                        Some(ModelInfo {
                            id: id.to_string(),
                            name: m["name"].as_str().unwrap_or(id).to_string(),
                            context_length: parse_context_length(m.get("context_length")),
                            prompt_price: 0.0,
                            completion_price: 0.0,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }
}

#[async_trait::async_trait]
impl Provider for OpenAiCompatProvider {
    fn name(&self) -> &'static str {
        self.spec.name
    }

    async fn complete(&self, req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        let body = self.build_body(&req, None);
        let resp = self.post_chat(&body).await?;
        Ok(self.result(&resp, req.model))
    }

    async fn models(&self, _force: bool) -> Vec<ModelInfo> {
        self.fetch_models().await
    }
}

/// Helper so auth + extra headers can be iterated in one `for` loop.
trait ChainMap {
    fn chain(self, other: BTreeMap<String, String>) -> BTreeMap<String, String>;
}
impl ChainMap for BTreeMap<String, String> {
    fn chain(mut self, other: BTreeMap<String, String>) -> BTreeMap<String, String> {
        self.extend(other);
        self
    }
}

/// OpenRouter: one balance, every vendor's models.
///
/// Adds on top of the compat call path: live per-token pricing, credit
/// auto-fit (OpenRouter pre-reserves `prompt + max_tokens * price` against
/// the balance), and app attribution headers.
pub struct OpenRouterProvider {
    compat: OpenAiCompatProvider,
    api_key: String,
    /// App attribution headers (HTTP-Referer / X-Title).
    app_name: String,
    app_url: String,
    pricing: std::sync::Mutex<PricingState>,
}

#[derive(Default)]
struct PricingState {
    pricing: BTreeMap<String, (f64, f64)>, // model -> (prompt, completion) per token
    models: Vec<ModelInfo>,
    loaded_at: Option<Instant>,
    credits: Option<f64>,
    credits_at: Option<Instant>,
}

impl OpenRouterProvider {
    pub fn new(
        api_key: &str,
        timeout: Duration,
        max_tokens: Option<i64>,
        app_name: &str,
        app_url: &str,
    ) -> Result<Self, ProviderError> {
        let mut provider = Self::with_base_url(api_key, timeout, max_tokens, OPENROUTER_BASE_URL)?;
        provider.app_name = app_name.to_string();
        provider.app_url = app_url.to_string();
        provider
            .compat
            .extra
            .insert("HTTP-Referer".to_string(), provider.app_url.clone());
        provider
            .compat
            .extra
            .insert("X-Title".to_string(), provider.app_name.clone());
        Ok(provider)
    }

    /// Test/alternative-endpoint constructor: same provider, other base URL.
    pub fn with_base_url(
        api_key: &str,
        timeout: Duration,
        max_tokens: Option<i64>,
        base_url: &str,
    ) -> Result<Self, ProviderError> {
        let spec = provider_spec("openrouter").expect("openrouter spec exists");
        let mut compat = OpenAiCompatProvider::new(spec, api_key, timeout, max_tokens, base_url)?;
        compat.base_url = base_url.to_string();
        Ok(Self {
            compat,
            api_key: api_key.to_string(),
            app_name: "delta".to_string(),
            app_url: "https://github.com/lukebrown14-code/Delta".to_string(),
            pricing: std::sync::Mutex::new(PricingState::default()),
        })
    }

    async fn fetch_credits(&self, force: bool) -> Option<f64> {
        {
            let state = self.pricing.lock().unwrap();
            if let (Some(credits), Some(at)) = (state.credits, state.credits_at) {
                if !force && elapsed_secs(at) < CREDITS_TTL_SECONDS {
                    return Some(credits);
                }
            }
        }
        if self.api_key.is_empty() {
            return None;
        }
        let resp = self
            .compat
            .client
            .get(format!(
                "{}/credits",
                self.compat.base_url.trim_end_matches('/')
            ))
            .timeout(Duration::from_secs(10))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
            .ok();
        let value = resp?.json::<Value>().await.ok()?;
        let data = &value["data"];
        let total = data["total_credits"].as_f64().unwrap_or(0.0);
        let usage = data["total_usage"].as_f64().unwrap_or(0.0);
        let credits = total - usage;
        let mut state = self.pricing.lock().unwrap();
        state.credits = Some(credits);
        state.credits_at = Some(Instant::now());
        Some(credits)
    }

    async fn maybe_load_pricing(&self, force: bool) {
        {
            let state = self.pricing.lock().unwrap();
            if let Some(at) = state.loaded_at {
                if !force && elapsed_secs(at) < PRICING_TTL_SECONDS {
                    return;
                }
            }
        }
        if self.api_key.is_empty() {
            return;
        }
        let Ok(resp) = self
            .compat
            .client
            .get(format!(
                "{}/models",
                self.compat.base_url.trim_end_matches('/')
            ))
            .timeout(Duration::from_secs(15))
            .header("Authorization", format!("Bearer {}", self.api_key))
            .send()
            .await
        else {
            self.pricing.lock().unwrap().loaded_at = Some(Instant::now());
            return;
        };
        let Ok(value) = resp.json::<Value>().await else {
            self.pricing.lock().unwrap().loaded_at = Some(Instant::now());
            return;
        };
        let mut pricing = BTreeMap::new();
        let mut models = Vec::new();
        for m in value["data"]
            .as_array()
            .map(|a| a.as_slice())
            .unwrap_or(&[])
        {
            let Some(id) = m["id"].as_str() else { continue };
            let p = &m["pricing"];
            let prompt_price = parse_price(p.get("prompt"));
            let completion_price = parse_price(p.get("completion"));
            pricing.insert(id.to_string(), (prompt_price, completion_price));
            models.push(ModelInfo {
                id: id.to_string(),
                name: m["name"].as_str().unwrap_or(id).to_string(),
                context_length: parse_context_length(m.get("context_length")),
                prompt_price,
                completion_price,
            });
        }
        let mut state = self.pricing.lock().unwrap();
        state.pricing = pricing;
        state.models = models;
        state.loaded_at = Some(Instant::now());
    }

    fn compute_cost(&self, model: &str, input_tokens: i64, output_tokens: i64) -> f64 {
        let state = self.pricing.lock().unwrap();
        match state.pricing.get(model) {
            Some(&(prompt, completion)) => {
                input_tokens as f64 * prompt + output_tokens as f64 * completion
            }
            None => 0.0,
        }
    }

    /// The `max_tokens` to send, or None when the provider default should apply.
    ///
    /// Fits the configured cap into the remaining OpenRouter credit balance
    /// using the cached pricing table. Auto-fit is skipped when the balance or
    /// the model's pricing is unknown or the model is free.
    async fn request_cap(
        &self,
        model: &str,
        messages: &[Message],
        force_credits: bool,
    ) -> Result<Option<i64>, ProviderError> {
        let max_tokens = self.compat.max_tokens;
        let Some(credits) = self.fetch_credits(force_credits).await else {
            return Ok(max_tokens);
        };
        self.maybe_load_pricing(false).await;
        let pricing = self.pricing.lock().unwrap().pricing.get(model).copied();
        let Some((prompt_price, completion_price)) = pricing else {
            return Ok(max_tokens);
        };
        if completion_price <= 0.0 {
            return Ok(max_tokens);
        }
        let prompt_cost = estimate_prompt_cost(messages, prompt_price);
        let budget = credits * BALANCE_MARGIN - prompt_cost;
        let affordable = if budget > 0.0 {
            (budget / completion_price) as i64
        } else {
            0
        };
        if affordable < MAX_TOKENS_FLOOR {
            return Err(self.budget_error(
                model,
                messages,
                Some(credits),
                Some((prompt_price, completion_price)),
            ));
        }
        Ok(Some(match max_tokens {
            None => affordable,
            Some(cap) => cap.min(affordable),
        }))
    }

    /// Actionable 402-style error: numbers first, remedies second.
    fn budget_error(
        &self,
        model: &str,
        messages: &[Message],
        credits: Option<f64>,
        pricing: Option<(f64, f64)>,
    ) -> ProviderError {
        let credits = credits.or_else(|| self.pricing.lock().unwrap().credits);
        let pricing = pricing.or_else(|| self.pricing.lock().unwrap().pricing.get(model).copied());
        let mut detail = String::new();
        if let (Some(credits), Some((prompt, completion))) = (credits, pricing) {
            if completion > 0.0 {
                let _ = credits;
                let need = estimate_prompt_cost(messages, prompt)
                    + (self.compat.max_tokens.unwrap_or(MAX_TOKENS_FLOOR) as f64) * completion;
                detail = format!(" (needs up to ~${need:.2} for {model})");
            }
        }
        let balance = match credits {
            Some(c) => format!("${c:.2} remaining"),
            None => "unknown balance".to_string(),
        };
        ProviderError::Budget { balance, detail }
    }
}

#[async_trait::async_trait]
impl Provider for OpenRouterProvider {
    fn name(&self) -> &'static str {
        "openrouter"
    }

    async fn complete(&self, req: CompletionRequest<'_>) -> Result<ProviderResult, ProviderError> {
        let cap = self.request_cap(req.model, req.messages, false).await?;
        let body = self.compat.build_body(&req, cap);
        let resp = match self.compat.post_chat(&body).await {
            Ok(resp) => resp,
            Err(err) if err.is_status(402) => {
                let previous = cap;
                let retry_cap = self.request_cap(req.model, req.messages, true).await?;
                match retry_cap {
                    Some(retry) if previous.is_none_or(|prev| retry < prev) => {
                        let body = self.compat.build_body(&req, retry_cap);
                        self.compat.post_chat(&body).await?
                    }
                    _ => return Err(self.budget_error(req.model, req.messages, None, None)),
                }
            }
            Err(err) => return Err(err),
        };
        self.maybe_load_pricing(false).await;
        let mut result = self.compat.result(&resp, req.model);
        result.cost_usd = self.compute_cost(req.model, result.input_tokens, result.output_tokens);
        Ok(result)
    }

    async fn models(&self, force: bool) -> Vec<ModelInfo> {
        self.maybe_load_pricing(force).await;
        self.pricing.lock().unwrap().models.clone()
    }
}

/// One authenticated GET proving a key works; never errors.
pub async fn verify_key(spec: &ProviderSpec, api_key: &str, base_url: &str) -> bool {
    if api_key.is_empty() {
        return false;
    }
    let base = if base_url.is_empty() {
        spec.base_url
    } else {
        base_url
    };
    if base.is_empty() {
        return false;
    }
    let client = reqwest::Client::new();
    let mut builder = client
        .get(format!(
            "{}{}",
            base.trim_end_matches('/'),
            spec.verify_path
        ))
        .timeout(Duration::from_secs(10));
    for (k, v) in auth_headers(spec, api_key) {
        builder = builder.header(k, v);
    }
    matches!(builder.send().await, Ok(resp) if resp.status() == 200)
}

fn elapsed_secs(at: Instant) -> f64 {
    at.elapsed().as_secs_f64()
}

/// `Retry-After` as seconds, when the server sends a parseable delay
/// (D7); HTTP-date forms fall back to exponential backoff.
fn retry_after_seconds(headers: &reqwest::header::HeaderMap) -> Option<f64> {
    headers
        .get(reqwest::header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()
}

/// The error response body: stringified JSON when parseable, raw text
/// otherwise (same `Status { body }` shape as before the retry loop).
async fn error_body(resp: reqwest::Response) -> String {
    let text = resp.text().await.unwrap_or_default();
    serde_json::from_str::<Value>(&text)
        .map(|v| v.to_string())
        .unwrap_or(text)
}

fn parse_context_length(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.parse().ok(),
        _ => None,
    }
}

fn parse_price(value: Option<&Value>) -> f64 {
    match value {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(0.0),
        Some(Value::String(s)) => s.parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

/// Rough prompt cost: chars/4 as the token count, times the per-token price.
fn estimate_prompt_cost(messages: &[Message], prompt_price: f64) -> f64 {
    let chars: usize = messages.iter().map(|m| m.content.len()).sum();
    (chars as f64 / 4.0) * prompt_price
}
