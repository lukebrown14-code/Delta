//! RSS data plugin: business news feeds -> NewsItem rows
//! (port of `delta/plugins/data/rss.py`).

use std::sync::Arc;

use chrono::{NaiveDateTime, Utc};
use delta_core::db::StoreItem;
use delta_core::ids::stable_id;
use delta_core::models::{Instrument, NewsItem};
use regex::RegexBuilder;
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::http::user_agent;
use crate::plugin::{DataPlugin, PluginError};

/// Cap concurrent feed requests so a large feed list does not fan out unbounded.
pub const MAX_CONCURRENCY: usize = 4;

/// Drop tags, unescape entities and collapse whitespace.
pub fn strip_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        if c == '<' {
            // Skip to the closing '>'.
            for (j, d) in text[i + 1..].char_indices() {
                if d == '>' {
                    // Advance the iterator past '>'.
                    let _ = chars.nth(j);
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    collapse_ws(&unescape_html(&out)).trim().to_string()
}

fn collapse_ws(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The HTML entities `html.unescape` handles in feed content, plus numeric
/// references. Unknown entities are left as written (finding: rust-plugins).
fn unescape_html(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail.find(';').map(|e| e + 1).unwrap_or(0);
        if end == 0 {
            out.push('&');
            rest = &rest[start + 1..];
            continue;
        }
        let entity = &tail[..end];
        let decoded = match entity {
            "&amp;" => "&",
            "&lt;" => "<",
            "&gt;" => ">",
            "&quot;" => "\"",
            "&apos;" | "&#39;" | "&#x27;" => "'",
            "&nbsp;" | "&#160;" | "&#xa0;" => "\u{a0}",
            _ => {
                // Numeric: &#123; / &#x7f;
                let body = &entity[1..end - 1];
                let code = if let Some(hex) =
                    body.strip_prefix("#x").or_else(|| body.strip_prefix("#X"))
                {
                    u32::from_str_radix(hex, 16).ok()
                } else {
                    body.parse::<u32>().ok()
                };
                match code.and_then(char::from_u32) {
                    Some(c) => {
                        out.push(c);
                        rest = &rest[start + end..];
                        continue;
                    }
                    None => entity,
                }
            }
        };
        out.push_str(decoded);
        rest = &rest[start + end..];
    }
    out.push_str(rest);
    out
}

/// `stable_id(link, published.isoformat())` — the Python id contract.
/// Python's `isoformat()` omits the fraction when the microsecond is zero.
pub fn news_id(link: &str, published: NaiveDateTime) -> String {
    let mut ts = published.format("%Y-%m-%dT%H:%M:%S").to_string();
    let micros = published.and_utc().timestamp_subsec_micros();
    if micros != 0 {
        ts.push_str(&format!(".{micros:06}"));
    }
    ts.push_str("+00:00");
    stable_id(&[link, &ts])
}

/// Compiled per-instrument patterns: `\bAAPL\b` and the first name token.
pub struct Matcher {
    patterns: Vec<(String, Vec<regex::Regex>)>,
}

impl Matcher {
    pub fn new(instruments: &[Instrument]) -> Self {
        let mut patterns = Vec::new();
        for inst in instruments {
            let mut pats = Vec::new();
            if !inst.symbol.is_empty() {
                // Tickers are upper-case in prose; a case-insensitive match on
                // short symbols such as "F" or "IT" would hit ordinary words.
                if let Ok(re) = word_regex(&inst.symbol, false) {
                    pats.push(re);
                }
            }
            if let Some(name) = &inst.name {
                if let Some(token) = name.split_whitespace().next() {
                    let token = token.trim_matches(|c: char| ".,;:()'\"".contains(c));
                    if !token.is_empty() {
                        if let Ok(re) = word_regex(token, true) {
                            pats.push(re);
                        }
                    }
                }
            }
            if !pats.is_empty() {
                patterns.push((inst.id.clone(), pats));
            }
        }
        Self { patterns }
    }

    pub fn matches(&self, text: &str) -> Vec<String> {
        self.patterns
            .iter()
            .filter(|(_, pats)| pats.iter().any(|p| p.is_match(text)))
            .map(|(id, _)| id.clone())
            .collect()
    }
}

fn word_regex(token: &str, ignore_case: bool) -> Result<regex::Regex, regex::Error> {
    let escaped = regex::escape(token);
    RegexBuilder::new(&format!(r"\b{escaped}\b"))
        .case_insensitive(ignore_case)
        .build()
}

/// Turn feed bytes into NewsItems published at or after `since`.
///
/// `now` is the timestamp given to undated entries; pass one value for a whole
/// fetch so the same undated story in two feeds gets the same id.
pub fn parse_feed(
    raw: &[u8],
    matcher: &Matcher,
    since: NaiveDateTime,
    now: NaiveDateTime,
) -> Vec<NewsItem> {
    let Ok(feed) = feed_rs::parser::parse(raw) else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for entry in &feed.entries {
        let Some(link) = entry.links.first().map(|l| l.href.as_str()) else {
            continue;
        };
        let link = link.trim();
        if link.is_empty() {
            continue;
        }
        let published = entry
            .published
            .or(entry.updated)
            .map(|d| d.with_timezone(&Utc).naive_utc())
            .unwrap_or(now);
        if published < since {
            continue;
        }
        let title = strip_html(
            entry
                .title
                .as_ref()
                .map(|t| t.content.as_str())
                .unwrap_or(""),
        );
        let body_raw = entry
            .summary
            .as_ref()
            .map(|t| t.content.as_str())
            .unwrap_or("");
        let body = strip_html(body_raw);
        let body = if body.is_empty() { None } else { Some(body) };
        let matched = matcher.matches(&format!("{title}\n{}", body.clone().unwrap_or_default()));
        items.push(NewsItem {
            id: news_id(link, published),
            instrument_ids: matched,
            published,
            title,
            url: link.to_string(),
            body,
            source: "rss".to_string(),
        });
    }
    items
}

/// RSS feeds -> NewsItems (port of `RSSData`).
#[derive(Default)]
pub struct RssData {
    pub feeds: Vec<String>,
    pub timeout_secs: u64,
    pub rate_limit_hold_secs: f64,
}

impl RssData {
    fn timeout(&self) -> std::time::Duration {
        if self.timeout_secs == 0 {
            std::time::Duration::from_secs(20)
        } else {
            std::time::Duration::from_secs(self.timeout_secs)
        }
    }
}

#[async_trait::async_trait]
impl DataPlugin for RssData {
    fn name(&self) -> &'static str {
        "rss"
    }

    fn configure(&mut self, cfg: &Value) {
        if let Some(feeds) = cfg.get("feeds").and_then(Value::as_array) {
            self.feeds = feeds
                .iter()
                .map(|f| match f {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
        }
        if let Some(t) = cfg.get("timeout").and_then(Value::as_f64) {
            self.timeout_secs = t as u64;
        }
    }

    async fn fetch(
        &self,
        instruments: &[Instrument],
        since: NaiveDateTime,
    ) -> Result<Vec<StoreItem>, PluginError> {
        let now = Utc::now().naive_utc();
        let matcher = Matcher::new(instruments);
        let client = reqwest::Client::builder()
            .timeout(self.timeout())
            .redirect(reqwest::redirect::Policy::limited(10))
            .user_agent(user_agent("research harness"))
            .build()?;
        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENCY));
        let mut gets = Vec::new();
        for url in &self.feeds {
            let permit = semaphore.clone().acquire_owned().await.ok();
            let client = client.clone();
            let url = url.clone();
            gets.push(tokio::spawn(async move {
                let _permit = permit;
                match client.get(&url).send().await {
                    Ok(resp) => match resp.error_for_status() {
                        Ok(resp) => resp.bytes().await.ok().map(|b| b.to_vec()),
                        Err(err) => {
                            log::warn!("rss: skipping feed {url}: {err}");
                            None
                        }
                    },
                    Err(err) => {
                        log::warn!("rss: skipping feed {url}: {err}");
                        None
                    }
                }
            }));
        }
        let mut items: Vec<NewsItem> = Vec::new();
        let mut seen: std::collections::BTreeSet<String> = Default::default();
        for handle in gets {
            if let Ok(Some(raw)) = handle.await {
                for item in parse_feed(&raw, &matcher, since, now) {
                    // The same story syndicated in two feeds has the same id.
                    if seen.insert(item.id.clone()) {
                        items.push(item);
                    }
                }
            }
        }
        Ok(items.into_iter().map(StoreItem::News).collect())
    }
}
