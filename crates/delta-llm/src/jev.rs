//! TypeSafe Jev decision-model client over OpenRouter's Decisions API.
//! Port of `delta/llm/jev.py`.
//!
//! Jev is not a chat-completions model: it answers typed questions about a
//! state and returns probabilities instead of text, so it bypasses
//! `LlmClient`/`structured` entirely. Responses are cached by payload hash and
//! every call (live or replayed) is logged to `llmcall` with tokens, cost and
//! latency — the same guarantees the chat client provides.

use std::time::{Duration, Instant};

use chrono::Utc;
use delta_core::db::Db;
use delta_core::ids::stable_id;
use delta_core::models::LlmCall;
use serde_json::{json, Map, Value};
use uuid::Uuid;

pub const DECISIONS_URL: &str = "https://openrouter.ai/api/alpha/decisions";
pub const JEV_MODEL: &str = "typesafe/jev-1.13";
pub const PROMPT_VERSION: &str = "jev_v1";
pub const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// One typed question: which of these options fits the state?
#[derive(Debug, Clone)]
pub struct ChoiceQuestion {
    pub key: String,
    pub instructions: String,
    /// Option name -> when to pick it.
    pub criteria: Vec<(String, String)>,
}

impl ChoiceQuestion {
    pub fn new(
        key: impl Into<String>,
        instructions: impl Into<String>,
        criteria: Vec<(String, String)>,
    ) -> Self {
        Self {
            key: key.into(),
            instructions: instructions.into(),
            criteria,
        }
    }

    pub fn payload(&self) -> Value {
        json!({
            "type": "choice",
            "instructions": self.instructions,
            "criteria": Value::Object(
                self.criteria
                    .iter()
                    .cloned()
                    .map(|(k, v)| (k, Value::String(v)))
                    .collect::<Map<String, Value>>(),
            ),
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
}

/// One Decisions response: typed answers plus what the call cost.
#[derive(Debug, Clone)]
pub struct Decision {
    pub model: String,
    pub answers: Map<String, Value>,
    pub usage: Usage,
    pub cached: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum JevError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("jev decisions request failed ({status}): {body}")]
    Api { status: u16, body: String },
    #[error("jev decisions response was not JSON: {0}")]
    Parse(String),
}

/// `json.dumps(body, sort_keys=True)` byte-for-byte, so payload hashes stay
/// compatible with rows cached by the Python client. Separators are `", "` /
/// `": "`, object keys sort, arrays keep order.
pub fn python_dumps(value: &Value) -> String {
    match value {
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => python_json_string(s),
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().map(python_dumps).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| format!("{}: {}", python_json_string(k), python_dumps(&map[*k])))
                .collect();
            format!("{{{}}}", parts.join(", "))
        }
    }
}

fn python_json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// `stable_id` over the canonical payload body (`_payload_hash`).
pub fn payload_hash(model: &str, state: &Value, questions: &Value) -> String {
    let body = json!({
        "model": model,
        "prompt_version": PROMPT_VERSION,
        "state": state,
        "questions": questions,
    });
    stable_id(&[&python_dumps(&body)])
}

fn usage_of(data: &Value) -> Usage {
    let raw = &data["usage"];
    Usage {
        input_tokens: raw["input_tokens"].as_i64().unwrap_or(0),
        output_tokens: raw["output_tokens"].as_i64().unwrap_or(0),
        cost_usd: raw["cost"].as_f64().unwrap_or(0.0),
    }
}

/// One OpenRouter key: cached typed decisions, logged cost.
pub struct JevClient {
    api_key: String,
    client: reqwest::Client,
    decisions_url: String,
}

impl JevClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            client: reqwest::Client::builder()
                .timeout(REQUEST_TIMEOUT)
                .build()
                .expect("reqwest client"),
            decisions_url: DECISIONS_URL.to_string(),
        }
    }

    /// Point the client at a different Decisions endpoint (tests).
    pub fn with_base_url(mut self, base_url: &str) -> Self {
        self.decisions_url = format!("{}/api/alpha/decisions", base_url.trim_end_matches('/'));
        self
    }

    /// Ask Jev one batch of independent questions about one state.
    pub async fn decide(
        &self,
        db: &mut Db,
        task: &str,
        state: &Value,
        questions: &[ChoiceQuestion],
    ) -> Result<Decision, JevError> {
        self.decide_with_model(db, task, state, questions, JEV_MODEL)
            .await
    }

    pub async fn decide_with_model(
        &self,
        db: &mut Db,
        task: &str,
        state: &Value,
        questions: &[ChoiceQuestion],
        model: &str,
    ) -> Result<Decision, JevError> {
        let payload_questions: Map<String, Value> = questions
            .iter()
            .map(|q| (q.key.clone(), q.payload()))
            .collect();
        let questions_value = Value::Object(payload_questions.clone());
        let phash = payload_hash(model, state, &questions_value);

        if let Some(cached) = self.lookup(db, task, &phash) {
            return Ok(cached);
        }

        let started = Instant::now();
        let resp = self
            .client
            .post(&self.decisions_url)
            .header("Authorization", format!("Bearer {}", self.api_key))
            .json(&json!({
                "model": model,
                "state": state,
                "questions": payload_questions,
            }))
            .send()
            .await?;
        let latency_ms = started.elapsed().as_millis() as i64;
        let status = resp.status().as_u16();
        let body = resp.text().await?;
        if status != 200 {
            let snippet: String = body.chars().take(200).collect();
            return Err(JevError::Api {
                status,
                body: snippet,
            });
        }
        let data: Value =
            serde_json::from_str(&body).map_err(|e| JevError::Parse(e.to_string()))?;
        let decision = Decision {
            model: data["model"].as_str().unwrap_or(model).to_string(),
            answers: data["answers"].as_object().cloned().unwrap_or_default(),
            usage: usage_of(&data),
            cached: false,
        };
        db.llm_store_call(&LlmCall {
            id: Uuid::new_v4().simple().to_string(),
            ts: Utc::now().naive_utc(),
            task: task.to_string(),
            model: decision.model.clone(),
            prompt_version: PROMPT_VERSION.to_string(),
            prompt_hash: phash,
            input_tokens: decision.usage.input_tokens,
            output_tokens: decision.usage.output_tokens,
            cost_usd: decision.usage.cost_usd,
            latency_ms,
            cached: false,
            response: Some(body),
        })
        .ok();
        Ok(decision)
    }

    /// Cached decision for a payload hash; a bad body is a miss. A valid hit
    /// is re-logged as a `cached=true` row so replay is also accounted for.
    fn lookup(&self, db: &mut Db, task: &str, phash: &str) -> Option<Decision> {
        let row = db.llm_lookup(phash).ok()??;
        let data: Value = serde_json::from_str(row.response.as_deref().unwrap_or("{}")).ok()?;
        db.llm_store_call(&LlmCall {
            id: Uuid::new_v4().simple().to_string(),
            ts: Utc::now().naive_utc(),
            task: task.to_string(),
            model: row.model.clone(),
            prompt_version: PROMPT_VERSION.to_string(),
            prompt_hash: phash.to_string(),
            input_tokens: 0,
            output_tokens: 0,
            cost_usd: 0.0,
            latency_ms: 0,
            cached: true,
            response: row.response.clone(),
        })
        .ok()?;
        Some(Decision {
            model: data["model"].as_str().unwrap_or(&row.model).to_string(),
            answers: data["answers"].as_object().cloned().unwrap_or_default(),
            usage: Usage {
                input_tokens: 0,
                output_tokens: 0,
                cost_usd: 0.0,
            },
            cached: true,
        })
    }
}
