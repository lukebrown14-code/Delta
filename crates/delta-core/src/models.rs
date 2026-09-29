//! Domain models. Port of `delta/core/models.py`.
//!
//! These are the canonical in-memory representations; persisted versions are
//! the tables in [`crate::db`] with the same field names. Datetimes are naive
//! UTC (`crate::time`).

use std::collections::{BTreeMap, BTreeSet};

use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Asset class tag for an [`Instrument`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum AssetClass {
    #[default]
    Equity,
    Etf,
    Bond,
    Fx,
    Commodity,
    Crypto,
    Cash,
    Other,
}

impl AssetClass {
    /// Serialised form stored in the DB and shown in the UI (snake_case,
    /// matching the Python `Literal` strings).
    pub fn as_str(self) -> &'static str {
        match self {
            AssetClass::Equity => "equity",
            AssetClass::Etf => "etf",
            AssetClass::Bond => "bond",
            AssetClass::Fx => "fx",
            AssetClass::Commodity => "commodity",
            AssetClass::Crypto => "crypto",
            AssetClass::Cash => "cash",
            AssetClass::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "equity" => AssetClass::Equity,
            "etf" => AssetClass::Etf,
            "bond" => AssetClass::Bond,
            "fx" => AssetClass::Fx,
            "commodity" => AssetClass::Commodity,
            "crypto" => AssetClass::Crypto,
            "cash" => AssetClass::Cash,
            "other" => AssetClass::Other,
            _ => return None,
        })
    }
}

/// A traded instrument. `id` format: `"<MARKET>:<SYMBOL>"`, e.g. `"US:AAPL"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Instrument {
    pub id: String,
    /// Plugin name: "us", "asx", "crypto".
    pub market: String,
    pub symbol: String,
    #[serde(default)]
    pub name: Option<String>,
    pub currency: String,
    #[serde(default)]
    pub sector: Option<String>,
    #[serde(default)]
    pub asset_class: AssetClass,
    #[serde(default)]
    pub watchlists: Vec<String>,
    #[serde(default)]
    pub tags: BTreeSet<String>,
    #[serde(default)]
    pub industry: Option<String>,
    #[serde(default)]
    pub meta: BTreeMap<String, Value>,
}

impl Instrument {
    pub fn sector_name(&self) -> &str {
        self.sector.as_deref().unwrap_or("unknown")
    }
}

/// One OHLCV bar; `ts` is the bar close, UTC.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bar {
    pub instrument_id: String,
    pub ts: NaiveDateTime,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
    /// Data plugin name.
    pub source: String,
}

/// A news article; `id` is a hash of url + published.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewsItem {
    pub id: String,
    #[serde(default)]
    pub instrument_ids: Vec<String>,
    pub published: NaiveDateTime,
    pub title: String,
    pub url: String,
    #[serde(default)]
    pub body: Option<String>,
    pub source: String,
}

/// Kind tag for an [`Event`]; strings match the Python `Literal` values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    Earnings,
    Guidance,
    Dividend,
    InsiderTrade,
    Ma,
    Regulatory,
    Macro,
    Other,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Earnings => "earnings",
            EventKind::Guidance => "guidance",
            EventKind::Dividend => "dividend",
            EventKind::InsiderTrade => "insider_trade",
            EventKind::Ma => "m&a",
            EventKind::Regulatory => "regulatory",
            EventKind::Macro => "macro",
            EventKind::Other => "other",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "earnings" => EventKind::Earnings,
            "guidance" => EventKind::Guidance,
            "dividend" => EventKind::Dividend,
            "insider_trade" => EventKind::InsiderTrade,
            "m&a" => EventKind::Ma,
            "regulatory" => EventKind::Regulatory,
            "macro" => EventKind::Macro,
            "other" => EventKind::Other,
            _ => return None,
        })
    }
}

/// An extracted company event with sentiment in `[-1, 1]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub instrument_id: String,
    pub ts: NaiveDateTime,
    pub kind: EventKind,
    pub summary: String,
    pub sentiment: f64,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
    /// Model id.
    pub extracted_by: String,
    pub prompt_version: String,
}

/// One metric observation as of a date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Fundamental {
    pub instrument_id: String,
    pub as_of: NaiveDate,
    pub metric: String,
    pub value: f64,
    pub source: String,
}

/// A logged LLM call (cost accounting and cache replay).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmCall {
    pub id: String,
    pub ts: NaiveDateTime,
    pub task: String,
    pub model: String,
    pub prompt_version: String,
    pub prompt_hash: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cost_usd: f64,
    pub latency_ms: i64,
    pub cached: bool,
    /// Cached response payload, used for backtest replay.
    #[serde(default)]
    pub response: Option<String>,
}
