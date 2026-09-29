//! ASX company announcements data plugin (port of
//! `delta/plugins/data/asx_announcements.py`).
//!
//! Pulls the announcements feed ASX serves through Markit Digital and turns
//! each announcement into a NewsItem. Price-sensitive announcements are
//! marked with a `[PS] ` title prefix.

use chrono::{NaiveDateTime, Utc};
use delta_core::db::StoreItem;
use delta_core::ids::stable_id;
use delta_core::models::{Instrument, NewsItem};
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::Semaphore;

use crate::http::user_agent;
use crate::plugin::{DataPlugin, PluginError, Rows};

pub const BASE_URL: &str =
    "https://asx.api.markitdigital.com/asx-research/1.0/companies/{code}/announcements";
/// The API returns no per-document URL; this page lists the company's
/// announcements and the fragment keeps the link unique per document.
pub const PAGE_URL: &str =
    "https://www.asx.com.au/markets/trade-our-cash-market/announcements.{code}#{key}";
pub const PS_PREFIX: &str = "[PS] ";
pub const RETRY_STATUSES: &[u16] = &[429, 500, 502, 503, 504];
/// Cap concurrent announcement fetches so a full watchlist does not fan out
/// without bound against Markit Digital.
pub const MAX_CONCURRENCY: usize = 4;

/// Stable id from ASX's own document key.
pub fn announcement_id(document_key: &str) -> String {
    stable_id(&["asx_announcement", document_key])
}

/// ASX announcements -> NewsItems (`ASXAnnouncements`).
#[derive(Debug, Clone)]
pub struct AsxAnnouncements {
    pub count: i64,
    pub timeout_secs: u64,
    pub max_retries: u32,
    pub backoff_seconds: f64,
    pub user_agent: String,
    /// Test seam: base URL override (empty = production).
    pub base_url: String,
    /// Test seam: `now` for the `toDate` param.
    pub today: Option<NaiveDateTime>,
}

impl Default for AsxAnnouncements {
    fn default() -> Self {
        // Python's __init__ defaults; `configure` overrides from config.
        Self {
            count: 20,
            timeout_secs: 0, // resolved to 20s in `timeout()`
            max_retries: 3,
            backoff_seconds: 1.0,
            user_agent: String::new(),
            base_url: String::new(),
            today: None,
        }
    }
}

impl AsxAnnouncements {
    fn timeout(&self) -> std::time::Duration {
        if self.timeout_secs == 0 {
            std::time::Duration::from_secs(20)
        } else {
            std::time::Duration::from_secs(self.timeout_secs)
        }
    }

    fn backoff(&self, attempt: u32) -> f64 {
        self.backoff_seconds * 2f64.powi(attempt as i32)
    }

    fn retry_delay(&self, headers: &reqwest::header::HeaderMap, attempt: u32) -> f64 {
        if let Some(v) = headers.get("Retry-After").and_then(|v| v.to_str().ok()) {
            if let Ok(secs) = v.parse::<f64>() {
                return secs.max(0.0);
            }
        }
        self.backoff(attempt)
    }

    fn scale(&self, secs: f64) -> std::time::Duration {
        std::time::Duration::from_secs_f64(secs)
    }

    async fn get_one_inner(
        client: &reqwest::Client,
        config: &AsxAnnouncements,
        code: &str,
        since: NaiveDateTime,
    ) -> Option<Vec<Value>> {
        let url = if config.base_url.is_empty() {
            BASE_URL.replace("{code}", &code.to_lowercase())
        } else {
            format!(
                "{}/asx-research/1.0/companies/{}/announcements",
                config.base_url.trim_end_matches('/'),
                code.to_lowercase()
            )
        };
        let today = config.today.unwrap_or_else(|| Utc::now().naive_utc());
        let params = [
            ("fromDate", since.date().format("%Y-%m-%d").to_string()),
            ("toDate", today.date().format("%Y-%m-%d").to_string()),
            ("itemsPerPage", config.count.to_string()),
            ("page", "0".to_string()),
        ];
        for attempt in 0..=config.max_retries {
            let resp = match client.get(&url).query(&params).send().await {
                Ok(resp) => resp,
                Err(err) => {
                    log::warn!("asx_announcements: {code} request failed: {err}");
                    if attempt >= config.max_retries {
                        return None;
                    }
                    tokio::time::sleep(config.scale(config.backoff(attempt))).await;
                    continue;
                }
            };
            let status = resp.status().as_u16();
            if status == 404 {
                log::info!("asx_announcements: {code} not found (404); skipping");
                return None;
            }
            if RETRY_STATUSES.contains(&status) && attempt < config.max_retries {
                let delay = config.retry_delay(resp.headers(), attempt);
                log::info!("asx_announcements: {code} got {status}; retrying in {delay:.1}s");
                tokio::time::sleep(config.scale(delay)).await;
                continue;
            }
            if status != 200 {
                log::warn!("asx_announcements: {code} returned {status}; skipping");
                return None;
            }
            let data: Value = match resp.json().await {
                Ok(data) => data,
                Err(err) => {
                    log::warn!("asx_announcements: {code} returned non-JSON body; skipping: {err}");
                    return None;
                }
            };
            let rows = data
                .get("data")
                .and_then(|d| d.get("items"))
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            return Some(rows.into_iter().filter(Value::is_object).collect());
        }
        None
    }

    /// Rows -> NewsItems (`_to_news`).
    fn to_news(&self, inst: &Instrument, rows: &[Value], since: NaiveDateTime) -> Vec<NewsItem> {
        let mut out = Vec::new();
        for row in rows {
            let Some(key) = row.get("documentKey").and_then(Value::as_str) else {
                continue;
            };
            let Some(raw_published) = row.get("date").and_then(Value::as_str) else {
                continue;
            };
            let Some(published) = parse_iso_lenient(raw_published) else {
                log::warn!(
                    "asx_announcements: {} bad timestamp {:?}; skipping",
                    inst.symbol,
                    raw_published
                );
                continue;
            };
            if published < since {
                continue;
            }
            let mut title = row
                .get("headline")
                .and_then(Value::as_str)
                .unwrap_or("")
                .trim()
                .to_string();
            if title.is_empty() {
                title = "Untitled announcement".to_string();
            }
            if row
                .get("isPriceSensitive")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                title = format!("{PS_PREFIX}{title}");
            }
            let url = match row.get("url").and_then(Value::as_str) {
                Some(u) if !u.is_empty() => u.to_string(),
                _ => PAGE_URL
                    .replace("{code}", &inst.symbol.to_lowercase())
                    .replace("{key}", key),
            };
            out.push(NewsItem {
                id: announcement_id(key),
                instrument_ids: vec![inst.id.clone()],
                published,
                title,
                url,
                body: None,
                source: "asx_announcements".to_string(),
            });
        }
        out
    }
}

/// `datetime.fromisoformat` accepts `2026-08-01T09:30:00+10:00` and the
/// space-separated variant; naive stamps are UTC by convention.
fn parse_iso_lenient(text: &str) -> Option<NaiveDateTime> {
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(text) {
        return Some(dt.with_timezone(&Utc).naive_utc());
    }
    chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok()
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl DataPlugin for AsxAnnouncements {
    fn name(&self) -> &'static str {
        "asx_announcements"
    }

    fn market(&self) -> Option<&'static str> {
        Some("asx")
    }

    fn configure(&mut self, cfg: &Value) {
        if let Some(v) = cfg.get("count").and_then(Value::as_i64) {
            self.count = v;
        }
        if let Some(v) = cfg.get("timeout").and_then(Value::as_f64) {
            self.timeout_secs = v as u64;
        }
        if let Some(v) = cfg.get("max_retries").and_then(Value::as_u64) {
            self.max_retries = v as u32;
        }
        if let Some(v) = cfg.get("backoff_seconds").and_then(Value::as_f64) {
            self.backoff_seconds = v;
        }
        if let Some(v) = cfg.get("user_agent").and_then(Value::as_str) {
            self.user_agent = v.to_string();
        }
    }

    async fn fetch(
        &self,
        instruments: &[Instrument],
        since: NaiveDateTime,
    ) -> Result<Rows, PluginError> {
        let targets: Vec<&Instrument> = instruments
            .iter()
            .filter(|i| Some(i.market.as_str()) == self.market())
            .collect();
        let agent = if self.user_agent.is_empty() {
            user_agent("+https://github.com/lukebrown14-code/Delta")
        } else {
            self.user_agent.clone()
        };
        let client = reqwest::Client::builder()
            .timeout(self.timeout())
            .user_agent(agent)
            .build()?;
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
        let gets = targets.iter().map(|inst| {
            let permit = semaphore.clone().acquire_owned();
            let client = &client;
            async move {
                let _permit = permit.await.ok();
                Self::get_one_inner(client, self, &inst.symbol, since).await
            }
        });
        let payloads = futures::future::join_all(gets).await;
        let mut items: Rows = Vec::new();
        for (inst, rows) in targets.iter().zip(payloads) {
            if let Some(rows) = rows {
                items.extend(
                    self.to_news(inst, &rows, since)
                        .into_iter()
                        .map(StoreItem::News),
                );
            }
        }
        Ok(items)
    }
}
