//! SEC EDGAR data plugin: recent filings as news and XBRL company facts as
//! fundamentals (port of `delta/plugins/data/sec_edgar.py`).
//!
//! Compliance with SEC fair-access rules:
//! * every request carries `User-Agent: Delta/0.1 (<contact email>)`;
//! * requests are throttled to at most 10 per second;
//! * `company_tickers.json` is fetched once per plugin instance and cached.

use chrono::{NaiveDate, NaiveDateTime};
use delta_core::db::StoreItem;
use delta_core::ids::stable_id;
#[allow(unused_imports)] // AssetClass is used only in tests
use delta_core::models::{AssetClass, Fundamental, Instrument, NewsItem};
use serde_json::Value;
use tokio::sync::Mutex as AsyncMutex;

use crate::http::user_agent;
use crate::plugin::{DataPlugin, PluginError, Rows};

pub const TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";
pub const SUBMISSIONS_URL: &str = "https://data.sec.gov/submissions/CIK{cik}.json";
pub const COMPANYFACTS_URL: &str = "https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json";

/// Filing forms surfaced as NewsItem rows.
pub const FORMS: &[&str] = &["8-K", "10-Q", "10-K", "4"];

/// XBRL tags to extract, as (namespace, primary tag, fallback tags).
pub const FACT_TAGS: &[(&str, &str, &[&str])] = &[
    (
        "us-gaap",
        "Revenues",
        &["RevenueFromContractWithCustomerExcludingAssessedTax"],
    ),
    ("us-gaap", "NetIncomeLoss", &[]),
    ("us-gaap", "EarningsPerShareDiluted", &[]),
    (
        "dei",
        "CommonStockSharesOutstanding",
        &["EntityCommonStockSharesOutstanding"],
    ),
];

pub const MAX_REQUESTS_PER_SECOND: usize = 10;

/// Transient statuses worth a bounded retry instead of aborting the whole fetch.
pub const RETRY_STATUSES: &[u16] = &[429, 500, 502, 503];
pub const MAX_RETRIES: u32 = 3;

/// Used when EDGAR gives no primaryDocDescription (routinely the case for
/// Form 4), so the brief and the extract model see what the filing is.
fn form_label(form: &str) -> &'static str {
    match form {
        "4" => "insider transaction report (Form 4)",
        "8-K" => "current report of a material event",
        "10-Q" => "quarterly report",
        "10-K" => "annual report",
        _ => "other",
    }
}

pub fn filing_url(cik: &str, accession: &str, document: &str) -> String {
    format!(
        "https://www.sec.gov/Archives/edgar/data/{}/{}/{}",
        cik.parse::<i64>().unwrap_or(0),
        accession.replace('-', ""),
        document
    )
}

/// SEC EDGAR -> filings as news, XBRL facts as fundamentals (`SECEdgar`).
pub struct SecEdgar {
    pub contact: String,
    /// Public so tests can construct the struct with `..Default::default()`
    /// once the private cache field lands behind it.
    #[doc(hidden)]
    pub cik_by_symbol: AsyncMutex<Option<std::collections::BTreeMap<String, String>>>,
    /// Test seam: the 1-second rate-limit hold and retry sleeps scale by this.
    pub sleep_scale: f64,
    /// Test seam: base URL overrides (empty = production).
    pub tickers_url: String,
    pub data_base_url: String,
}

impl Default for SecEdgar {
    fn default() -> Self {
        Self {
            contact: String::new(),
            cik_by_symbol: AsyncMutex::new(None),
            sleep_scale: 1.0,
            tickers_url: String::new(),
            data_base_url: String::new(),
        }
    }
}

impl SecEdgar {
    pub fn user_agent(&self) -> String {
        user_agent(&self.contact)
    }

    fn sleep(&self, secs: f64) -> std::time::Duration {
        std::time::Duration::from_secs_f64((secs * self.sleep_scale.max(0.0)).max(0.0))
    }

    fn url(&self, template: &str, cik: &str) -> String {
        if self.data_base_url.is_empty() {
            return template.replace("{cik}", cik);
        }
        // Keep the template's path after its host: the test seam replaces the
        // host only.
        let after_scheme = template
            .split_once("://")
            .map(|(_, rest)| rest)
            .unwrap_or(template);
        let path = after_scheme[after_scheme.find('/').unwrap_or(0)..].to_string();
        format!(
            "{}{}",
            self.data_base_url.trim_end_matches('/'),
            path.replace("{cik}", cik)
        )
    }

    /// GET JSON with the rate cap and bounded retry on 429/5xx
    /// (`_get_json`; the semaphore hold scales with `sleep_scale`).
    async fn get_json(
        &self,
        client: &reqwest::Client,
        url: &str,
        _permit: &tokio::sync::OwnedSemaphorePermit,
    ) -> Result<Value, PluginError> {
        for attempt in 0..=MAX_RETRIES {
            let resp = client.get(url).send().await?;
            let status = resp.status().as_u16();
            if status == 200 {
                tokio::time::sleep(self.sleep(1.0)).await; // hold the rate slot
                return resp.json::<Value>().await.map_err(Into::into);
            }
            if RETRY_STATUSES.contains(&status) && attempt < MAX_RETRIES {
                let retry_after = resp
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or_else(|| 2f64.powi(attempt as i32));
                tokio::time::sleep(self.sleep(retry_after)).await;
                continue;
            }
            return Err(PluginError::Other {
                plugin: "sec_edgar",
                message: format!("{url} returned {status}"),
            });
        }
        Err(PluginError::Other {
            plugin: "sec_edgar",
            message: format!("{url} exhausted retries"),
        })
    }
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl DataPlugin for SecEdgar {
    fn name(&self) -> &'static str {
        "sec_edgar"
    }

    fn market(&self) -> Option<&'static str> {
        Some("us")
    }

    fn configure(&mut self, cfg: &Value) {
        if let Some(c) = cfg.get("contact").and_then(Value::as_str) {
            self.contact = c.to_string();
        }
    }

    async fn fetch(
        &self,
        instruments: &[Instrument],
        since: NaiveDateTime,
    ) -> Result<Rows, PluginError> {
        if !self.contact.contains('@') {
            return Err(PluginError::Other {
                plugin: "sec_edgar",
                message: "set [plugins.sec_edgar] contact to a real email address".to_string(),
            });
        }
        let since_date = since.date();
        let semaphore = Arc::new(tokio::sync::Semaphore::new(MAX_REQUESTS_PER_SECOND));
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(30))
            .user_agent(self.user_agent())
            .build()?;

        let tickers_url = if self.tickers_url.is_empty() {
            TICKERS_URL.to_string()
        } else {
            self.tickers_url.clone()
        };
        let mut cik_guard = self.cik_by_symbol.lock().await;
        if cik_guard.is_none() {
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let payload = self.get_json(&client, &tickers_url, &permit).await?;
            drop(permit);
            let mut map = std::collections::BTreeMap::new();
            let rows: Vec<&Value> = match &payload {
                Value::Object(map_) => map_.values().collect(),
                Value::Array(items) => items.iter().collect(),
                _ => Vec::new(),
            };
            for row in rows {
                let ticker = row
                    .get("ticker")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_uppercase();
                let cik = row.get("cik_str").and_then(Value::as_i64).unwrap_or(0);
                map.insert(ticker, format!("{cik:010}"));
            }
            *cik_guard = Some(map);
        }
        let ciks = cik_guard.as_ref().unwrap();
        let mut out: Rows = Vec::new();
        for inst in instruments
            .iter()
            .filter(|i| Some(i.market.as_str()) == self.market())
        {
            let Some(cik) = ciks.get(&inst.symbol.to_uppercase()) else {
                log::warn!("sec_edgar: no CIK for {}, skipping", inst.symbol);
                continue;
            };
            let permit = semaphore.clone().acquire_owned().await.unwrap();
            let subs = self
                .get_json(&client, &self.url(SUBMISSIONS_URL, cik), &permit)
                .await?;
            let facts = self
                .get_json(&client, &self.url(COMPANYFACTS_URL, cik), &permit)
                .await?;
            drop(permit);
            out.extend(
                filings_to_news(inst, cik, &subs, since_date)
                    .into_iter()
                    .map(StoreItem::News),
            );
            out.extend(
                facts_to_fundamentals(inst, &facts)
                    .into_iter()
                    .map(StoreItem::Fundamental),
            );
        }
        Ok(out)
    }
}

/// Filings in the tracked forms filed at/after `since` -> NewsItems
/// (`_filings_to_news`).
pub fn filings_to_news(
    inst: &Instrument,
    cik: &str,
    submissions: &Value,
    since: NaiveDate,
) -> Vec<NewsItem> {
    let recent = &submissions["filings"]["recent"];
    let arr = |key: &str| -> Vec<String> {
        recent[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let (forms, dates, accessions, documents, descriptions) = (
        arr("form"),
        arr("filingDate"),
        arr("accessionNumber"),
        arr("primaryDocument"),
        arr("primaryDocDescription"),
    );
    let mut items = Vec::new();
    for ((((form, filed_raw), accession), document), description) in forms
        .iter()
        .zip(&dates)
        .zip(accessions.iter())
        .zip(documents.iter())
        .zip(descriptions.iter())
    {
        if !FORMS.contains(&form.as_str()) {
            continue;
        }
        let Some(filed) = NaiveDate::parse_from_str(filed_raw, "%Y-%m-%d").ok() else {
            continue;
        };
        if filed < since {
            continue;
        }
        let title = format!(
            "{form}: {}",
            if description.is_empty() {
                form_label(form).to_string()
            } else {
                description.clone()
            }
        );
        items.push(NewsItem {
            id: stable_id(&[accession]),
            instrument_ids: vec![inst.id.clone()],
            published: filed.and_hms_opt(0, 0, 0).unwrap_or_default(),
            title,
            url: filing_url(cik, accession, document),
            body: None,
            source: "sec_edgar".to_string(),
        });
    }
    items
}

/// Facts for every listed tag, searching `namespace` first
/// (`_find_series`; filers switch tags over time, so series are merged).
fn find_series<'a>(facts: &'a Value, namespace: &str, tags: &[&str]) -> Vec<&'a Value> {
    let mut series = Vec::new();
    let Some(map) = facts.as_object() else {
        return series;
    };
    let mut namespaces: Vec<&String> = map.keys().collect();
    namespaces.sort_by_key(|ns| (*ns != namespace, (*ns).clone()));
    for ns in namespaces {
        let concepts = map.get(ns).and_then(Value::as_object);
        for tag in tags {
            let units = concepts
                .and_then(|c| c.get(*tag))
                .and_then(|t| t.get("units"))
                .and_then(Value::as_object);
            for values in units.into_iter().flat_map(|u| u.values()) {
                if let Value::Array(items) = values {
                    series.extend(items.iter());
                }
            }
        }
    }
    series
}

/// Latest fact for `form` by `end` date (then `filed`), with duration
/// filtering so a 10-K yields the full-year figure (`_latest_fact`).
fn latest_fact<'a>(series: &[&'a Value], form: &str) -> Option<&'a Value> {
    let mut candidates: Vec<&Value> = series
        .iter()
        .copied()
        .filter(|fact| {
            let form_ok = fact.get("form").and_then(Value::as_str) == Some(form);
            let fp_ok = fact
                .get("fp")
                .and_then(Value::as_str)
                .is_some_and(|s| !s.is_empty());
            form_ok && fp_ok
        })
        .filter(|fact| {
            let (Some(start), Some(end)) = (
                fact.get("start").and_then(Value::as_str),
                fact.get("end").and_then(Value::as_str),
            ) else {
                return true; // no start: instant fact, keep
            };
            let Ok(start) = NaiveDate::parse_from_str(start, "%Y-%m-%d") else {
                return true;
            };
            let Ok(end) = NaiveDate::parse_from_str(end, "%Y-%m-%d") else {
                return true;
            };
            let days = (end - start).num_days();
            if form == "10-K" && days < 300 {
                return false;
            }
            if form == "10-Q" && days > 120 {
                return false;
            }
            true
        })
        .collect();
    candidates.sort_by(|a, b| {
        let key = |f: &Value| {
            (
                f.get("end")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
                f.get("filed")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string(),
            )
        };
        key(a).cmp(&key(b))
    });
    candidates.pop()
}

/// Company facts -> Fundamentals (`_facts_to_fundamentals`).
pub fn facts_to_fundamentals(inst: &Instrument, payload: &Value) -> Vec<Fundamental> {
    let facts = &payload["facts"];
    let mut out = Vec::new();
    for (namespace, tag, fallbacks) in FACT_TAGS {
        let mut tags = vec![*tag];
        tags.extend_from_slice(fallbacks);
        let series = find_series(facts, namespace, &tags);
        if series.is_empty() {
            continue;
        }
        for form in ["10-K", "10-Q"] {
            let Some(fact) = latest_fact(&series, form) else {
                continue;
            };
            let Some(end) = fact.get("end").and_then(Value::as_str) else {
                continue;
            };
            let Ok(as_of) = NaiveDate::parse_from_str(end, "%Y-%m-%d") else {
                continue;
            };
            let fp = fact.get("fp").and_then(Value::as_str).unwrap_or("");
            let value = fact.get("val").and_then(Value::as_f64).unwrap_or(0.0);
            out.push(Fundamental {
                instrument_id: inst.id.clone(),
                as_of,
                metric: format!("{tag}_{fp}"),
                value,
                source: "sec_edgar".to_string(),
            });
        }
    }
    out
}

use std::sync::Arc;

#[cfg(test)]
mod tests {
    use super::*;

    fn inst_aapl() -> Instrument {
        Instrument {
            id: "US:AAPL".to_string(),
            market: "us".to_string(),
            symbol: "AAPL".to_string(),
            name: None,
            currency: "USD".to_string(),
            sector: None,
            asset_class: AssetClass::Equity,
            watchlists: Vec::new(),
            tags: Default::default(),
            industry: None,
            meta: Default::default(),
        }
    }

    #[test]
    fn filings_map_to_news_with_labels() {
        let inst = inst_aapl();
        let submissions: Value = serde_json::json!({
            "filings": {"recent": {
                "form": ["8-K", "4", "10-Q", "S-1"],
                "filingDate": ["2026-08-01", "2026-08-02", "2026-01-01", "2026-08-03"],
                "accessionNumber": ["a-1", "a-2", "old", "a-3"],
                "primaryDocument": ["d1.htm", "d2.htm", "d3.htm", "d4.htm"],
                "primaryDocDescription": ["", "", "Q1 FY26", ""]
            }}
        });
        let items = filings_to_news(
            &inst,
            "0000320193",
            &submissions,
            NaiveDate::from_ymd_opt(2026, 8, 1).unwrap(),
        );
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].title, "8-K: current report of a material event");
        assert_eq!(items[1].title, "4: insider transaction report (Form 4)");
        assert_eq!(items[0].id, stable_id(&["a-1"]));
        assert_eq!(
            items[0].url,
            "https://www.sec.gov/Archives/edgar/data/320193/a1/d1.htm"
        );
    }

    #[test]
    fn facts_pick_latest_by_form_duration() {
        let inst = inst_aapl();
        let payload: Value = serde_json::json!({
            "facts": {"us-gaap": {"NetIncomeLoss": {"units": {"USD": [
                {"end": "2025-09-27", "val": 93700000000i64, "form": "10-K", "fp": "FY",
                 "start": "2024-09-29", "filed": "2025-10-31"},
                {"end": "2025-09-27", "val": 1.0, "form": "10-K", "fp": "FY",
                 "start": "2025-06-29", "filed": "2025-10-31"}
            ]}}}}
        });
        let out = facts_to_fundamentals(&inst, &payload);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].metric, "NetIncomeLoss_FY");
        assert_eq!(out[0].value, 93_700_000_000.0);
    }
}
