//! Grounded conversation over the stored evidence pool (port of `delta/chat.py`).

use std::collections::BTreeSet;

use delta_core::config::{read_env_value_named, AppConfig, ENV_PATH};
use delta_core::db::Db;
use delta_llm::client::{CompleteParams, LlmClient};
use delta_llm::providers::Message;
use delta_llm::router::model_for;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::Path;

use crate::error::ServiceError;
use crate::evidence::{cite, evidence, EvidenceItem};

pub const CHAT_PROMPT_VERSION: &str = "chat_v1";
pub const UNSUPPORTED_NOTE: &str =
    "This answer could not be supported from your stored data; treat it as AI inference.";
const EVIDENCE_LIMIT: usize = 50;
const SYSTEM_PROMPT: &str = "You are Delta, a research assistant answering questions about the user's watch
targets using only the evidence supplied in this conversation.

Rules:
- Use only the supplied evidence. Never reason from memory; when the evidence is
  insufficient, say what is missing instead of guessing.
- Support every fact with a citation: the id of a stored evidence item (for example
  \"bar:5\") or the url of a web result.
- Stored evidence is established fact. Web results are unverified context and must be
  treated as such.
- Do not recommend buying or selling anything.

Reply with JSON only, no surrounding text:
{\"answer\": \"<your reply>\", \"citations\": [\"<evidence id or web url>\", ...], \"web_queries\": [...]}";
const WEB_RULE: &str =
    "Web search is enabled: web_queries lists at most three short search queries that
would help answer the question; leave it empty when none are needed.";
const NO_WEB_RULE: &str = "Web search is disabled: web_queries must always be an empty list.";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub text: String,
    #[serde(default)]
    pub citations: Vec<String>,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebHit {
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub snippet: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatDraft {
    pub answer: String,
    #[serde(default)]
    pub citations: Vec<String>,
    #[serde(default)]
    pub web_queries: Vec<String>,
}

#[async_trait::async_trait]
pub trait SearchTool: Send + Sync {
    async fn search(&self, query: &str) -> Vec<WebHit>;
}

pub struct OfflineSearchTool;

#[async_trait::async_trait]
impl SearchTool for OfflineSearchTool {
    async fn search(&self, _query: &str) -> Vec<WebHit> {
        Vec::new()
    }
}

fn parse_draft(text: &str) -> Result<ChatDraft, ServiceError> {
    let value = delta_llm::json::extract_json(text)
        .map_err(|err| ServiceError::invalid(err.to_string()))?;
    serde_json::from_value(value).map_err(|err| ServiceError::invalid(err.to_string()))
}

fn valid_draft(text: &str) -> bool {
    parse_draft(text).is_ok()
}

async fn draft(
    db: &mut Db,
    client: &LlmClient,
    model: &str,
    messages: &[Message],
) -> Result<ChatDraft, ServiceError> {
    let response_format = json!({
        "type": "json_schema",
        "json_schema": {
            "name": "ChatDraft",
            "schema": {
                "type": "object",
                "properties": {
                    "answer": {"type": "string"},
                    "citations": {"type": "array", "items": {"type": "string"}},
                    "web_queries": {"type": "array", "items": {"type": "string"}}
                },
                "required": ["answer"]
            }
        }
    });
    let params = CompleteParams {
        task: "chat",
        model,
        prompt_version: CHAT_PROMPT_VERSION,
        prompt: None,
        messages: Some(messages),
        response_format: Some(&response_format),
        cache_validator: Some(&valid_draft),
    };
    let first = client
        .chat(db, params)
        .await
        .map_err(|err| ServiceError::invalid(err.to_string()))?;
    match parse_draft(&first.text) {
        Ok(draft) => Ok(draft),
        Err(err) => {
            let mut retry = messages.to_vec();
            retry.push(Message::new(
                "user",
                &format!(
                    "Your previous reply was not valid JSON for the required schema. \
                     Return valid JSON only, fixing: {err}"
                ),
            ));
            let params = CompleteParams {
                messages: Some(&retry),
                ..params
            };
            let second = client
                .chat(db, params)
                .await
                .map_err(|err| ServiceError::invalid(err.to_string()))?;
            parse_draft(&second.text)
        }
    }
}

fn stored_context(items: &[EvidenceItem]) -> String {
    let mut lines =
        vec!["Stored evidence for the selected targets (cite items by id):".to_string()];
    if items.is_empty() {
        lines.push("(no stored evidence for these targets)".to_string());
    }
    for item in items {
        let mut line = format!("- {}: {}", item.id, cite(item));
        if let Some(body) = &item.body {
            let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
            line.push_str(&format!(
                " — {}",
                collapsed.chars().take(200).collect::<String>()
            ));
        }
        lines.push(line);
    }
    lines.join("\n")
}

fn web_context(hits: &[WebHit]) -> String {
    let mut lines =
        vec!["Web search results (not stored; unverified context, cite by url):".to_string()];
    lines.extend(
        hits.iter()
            .map(|hit| format!("- {}: {} — {}", hit.url, hit.title, hit.snippet)),
    );
    lines.join("\n")
}

fn verified(draft: ChatDraft, items: &[EvidenceItem], hits: &[WebHit]) -> ChatMessage {
    let stored: BTreeSet<&str> = items.iter().map(|item| item.id.as_str()).collect();
    let web: BTreeSet<&str> = hits.iter().map(|hit| hit.url.as_str()).collect();
    let mut citations = Vec::new();
    let mut seen = BTreeSet::new();
    let mut dropped = false;
    let mut used_web = false;
    for citation in draft.citations {
        if stored.contains(citation.as_str()) {
            if seen.insert(citation.clone()) {
                citations.push(citation);
            }
        } else if web.contains(citation.as_str()) {
            used_web = true;
            if seen.insert(citation.clone()) {
                citations.push(citation);
            }
        } else {
            dropped = true;
        }
    }
    let text = if citations.is_empty() {
        format!("{}\n\n{UNSUPPORTED_NOTE}", draft.answer.trim_end())
    } else {
        draft.answer
    };
    let source = if citations.is_empty() || dropped {
        "inference"
    } else if used_web {
        "web"
    } else {
        "stored"
    };
    ChatMessage {
        role: "assistant".to_string(),
        text,
        citations,
        source: source.to_string(),
    }
}

/// Answer the latest user turn using only evidence gathered for selected targets.
pub async fn chat(
    db: &mut Db,
    client: &LlmClient,
    cfg: &AppConfig,
    history: &[ChatMessage],
    targets: &[String],
    allow_web: bool,
    search: Option<&dyn SearchTool>,
) -> Result<ChatMessage, ServiceError> {
    let model =
        model_for(cfg, "chat", None).map_err(|err| ServiceError::invalid(err.to_string()))?;
    let mut seen = BTreeSet::new();
    let mut items = Vec::new();
    for target in targets {
        for item in evidence(db, Some(target), None, None, EVIDENCE_LIMIT, None)? {
            if seen.insert(item.id.clone()) {
                items.push(item);
            }
        }
    }
    let rule = if allow_web { WEB_RULE } else { NO_WEB_RULE };
    let mut head = vec![
        Message::new("system", &format!("{SYSTEM_PROMPT}\n\n{rule}")),
        Message::new("user", &stored_context(&items)),
    ];
    let mut tail: Vec<Message> = history
        .iter()
        .map(|turn| {
            let mut text = turn.text.clone();
            if !turn.citations.is_empty() {
                text.push_str(&format!(
                    "\n{}",
                    turn.citations
                        .iter()
                        .map(|id| format!("({id})"))
                        .collect::<Vec<_>>()
                        .join(" ")
                ));
            }
            if turn.role == "tool" {
                text = format!("[web search result] {text}");
            }
            Message::new(
                if turn.role == "assistant" {
                    "assistant"
                } else {
                    "user"
                },
                &text,
            )
        })
        .collect();
    let mut messages = head.clone();
    messages.append(&mut tail);
    let mut answer = draft(db, client, &model, &messages).await?;
    let mut hits = Vec::new();
    if allow_web && !answer.web_queries.is_empty() {
        let fallback = OfflineSearchTool;
        let tool: &dyn SearchTool = search.unwrap_or(&fallback);
        let mut seen_urls = BTreeSet::new();
        for query in &answer.web_queries {
            for hit in tool.search(query).await {
                if seen_urls.insert(hit.url.clone()) {
                    hits.push(hit);
                }
            }
        }
        if !hits.is_empty() {
            head.push(Message::new("user", &web_context(&hits)));
            head.extend(messages.into_iter().skip(2));
            answer = draft(db, client, &model, &head).await?;
        }
    }
    Ok(verified(answer, &items, &hits))
}

/// Build the configured provider and answer one TUI turn.
pub async fn chat_configured(
    db: &mut Db,
    cfg: &AppConfig,
    history: &[ChatMessage],
    targets: &[String],
) -> Result<ChatMessage, ServiceError> {
    let client = delta_llm::client::build_client(
        &cfg.llm_provider,
        &|name| read_env_value_named(name, Path::new(ENV_PATH)),
        std::time::Duration::from_secs(60),
        Some(cfg.llm_max_output_tokens),
        &cfg.llm_base_url,
        &cfg.llm_api_key_env,
    )
    .map_err(|err| ServiceError::invalid(err.to_string()))?;
    chat(db, &client, cfg, history, targets, false, None).await
}
