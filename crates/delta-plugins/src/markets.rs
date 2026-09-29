//! Market plugins: universe construction and session times
//! (ports of `delta/plugins/markets/us.py` and `asx.py`).

use chrono::{DateTime, Datelike, Duration, NaiveTime, Timelike, Utc};
use delta_core::ids::make_instrument_id;
use delta_core::models::Instrument;
use serde_json::Value;

use crate::plugin::DataPlugin;

const US_EASTERN_OFFSET_SECONDS: i32 = -5 * 3600; // EST; see finding rust-plugins #7

const MARKET_OPEN: (u32, u32) = (9, 30);
const MARKET_CLOSE: (u32, u32) = (16, 0);

const COMMON_SECTORS: &[(&str, &str)] = &[
    ("AAPL", "Technology"),
    ("MSFT", "Technology"),
    ("NVDA", "Technology"),
    ("GOOGL", "Communication Services"),
    ("AMZN", "Consumer Discretionary"),
    ("META", "Communication Services"),
    ("TSLA", "Consumer Discretionary"),
];

pub fn common_sector(symbol: &str) -> Option<&'static str> {
    COMMON_SECTORS
        .iter()
        .find(|(s, _)| *s == symbol)
        .map(|(_, sector)| *sector)
}

/// US equities market (`USMarket`).
#[derive(Default, Clone)]
pub struct UsMarket {
    pub tickers: Vec<String>,
}

impl UsMarket {
    fn local(&self, ts: DateTime<Utc>) -> DateTime<chrono::FixedOffset> {
        ts.with_timezone(&chrono::FixedOffset::east_opt(US_EASTERN_OFFSET_SECONDS).unwrap())
    }

    pub fn universe(&self) -> Vec<Instrument> {
        self.tickers
            .iter()
            .map(|s| Instrument {
                id: make_instrument_id("US", s),
                market: "us".to_string(),
                symbol: s.clone(),
                name: Some(s.clone()),
                currency: "USD".to_string(),
                sector: common_sector(s).map(str::to_string),
                asset_class: delta_core::models::AssetClass::Equity,
                watchlists: Vec::new(),
                tags: Default::default(),
                industry: None,
                meta: Default::default(),
            })
            .collect()
    }

    pub fn is_open(&self, ts: DateTime<Utc>) -> bool {
        let local = self.local(ts);
        if local.weekday().number_from_monday() >= 6 {
            return false;
        }
        let open = NaiveTime::from_hms_opt(MARKET_OPEN.0, MARKET_OPEN.1, 0).unwrap();
        let close = NaiveTime::from_hms_opt(MARKET_CLOSE.0, MARKET_CLOSE.1, 0).unwrap();
        open <= local.time() && local.time() <= close
    }

    pub fn next_open(&self, ts: DateTime<Utc>) -> DateTime<Utc> {
        let mut candidate = self.local(ts);
        for _ in 0..14 {
            candidate = candidate
                .with_hour(MARKET_OPEN.0)
                .and_then(|c| c.with_minute(MARKET_OPEN.1))
                .unwrap_or(candidate)
                .with_second(0)
                .unwrap_or(candidate)
                .with_nanosecond(0)
                .unwrap_or(candidate)
                + Duration::days(1);
            if candidate.weekday().number_from_monday() < 6 {
                return candidate.with_timezone(&Utc);
            }
        }
        ts
    }
}

/// ASX market (`AsxMarket`); sessions are 10:00–16:00 AEST (UTC+10).
#[derive(Default, Clone)]
pub struct AsxMarket {
    pub tickers: Vec<String>,
}

const ASX_OFFSET_SECONDS: i32 = 10 * 3600;

impl AsxMarket {
    fn local(&self, ts: DateTime<Utc>) -> DateTime<chrono::FixedOffset> {
        ts.with_timezone(&chrono::FixedOffset::east_opt(ASX_OFFSET_SECONDS).unwrap())
    }

    pub fn universe(&self) -> Vec<Instrument> {
        self.tickers
            .iter()
            .map(|s| Instrument {
                id: make_instrument_id("ASX", s),
                market: "asx".to_string(),
                symbol: s.clone(),
                name: Some(s.clone()),
                currency: "AUD".to_string(),
                sector: None,
                asset_class: delta_core::models::AssetClass::Equity,
                watchlists: Vec::new(),
                tags: Default::default(),
                industry: None,
                meta: Default::default(),
            })
            .collect()
    }

    pub fn is_open(&self, ts: DateTime<Utc>) -> bool {
        let local = self.local(ts);
        if local.weekday().number_from_monday() >= 6 {
            return false;
        }
        let open = NaiveTime::from_hms_opt(10, 0, 0).unwrap();
        let close = NaiveTime::from_hms_opt(16, 0, 0).unwrap();
        open <= local.time() && local.time() <= close
    }

    pub fn next_open(&self, ts: DateTime<Utc>) -> DateTime<Utc> {
        let mut candidate = self.local(ts);
        for _ in 0..14 {
            candidate = candidate
                .with_hour(10)
                .and_then(|c| c.with_minute(0))
                .unwrap_or(candidate)
                .with_second(0)
                .unwrap_or(candidate)
                .with_nanosecond(0)
                .unwrap_or(candidate)
                + Duration::days(1);
            if candidate.weekday().number_from_monday() < 6 {
                return candidate.with_timezone(&Utc);
            }
        }
        ts
    }
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl DataPlugin for UsMarket {
    fn name(&self) -> &'static str {
        "us"
    }
    fn market(&self) -> Option<&'static str> {
        Some("us")
    }
    fn configure(&mut self, cfg: &Value) {
        if let Some(t) = cfg.get("tickers").and_then(Value::as_array) {
            self.tickers = t
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
        }
    }
    async fn fetch(
        &self,
        _instruments: &[Instrument],
        _since: chrono::NaiveDateTime,
    ) -> Result<Vec<delta_core::db::StoreItem>, crate::plugin::PluginError> {
        Ok(Vec::new())
    }
}

#[async_trait::async_trait]
#[async_trait::async_trait]
impl DataPlugin for AsxMarket {
    fn name(&self) -> &'static str {
        "asx"
    }
    fn market(&self) -> Option<&'static str> {
        Some("asx")
    }
    fn configure(&mut self, cfg: &Value) {
        if let Some(t) = cfg.get("tickers").and_then(Value::as_array) {
            self.tickers = t
                .iter()
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .collect();
        }
    }
    async fn fetch(
        &self,
        _instruments: &[Instrument],
        _since: chrono::NaiveDateTime,
    ) -> Result<Vec<delta_core::db::StoreItem>, crate::plugin::PluginError> {
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn us_universe_carries_known_sectors() {
        let mut market = UsMarket::default();
        market.configure(&serde_json::json!({"tickers": ["AAPL", "OSTK"]}));
        let universe = market.universe();
        assert_eq!(universe.len(), 2);
        assert_eq!(universe[0].id, "US:AAPL");
        assert_eq!(universe[0].sector.as_deref(), Some("Technology"));
        assert_eq!(universe[1].sector, None);
    }

    #[test]
    fn us_session_boundaries() {
        let market = UsMarket::default();
        // Wed 2026-01-07 14:30 UTC = 09:30 EST: open.
        let open = DateTime::parse_from_rfc3339("2026-01-07T14:30:00Z").unwrap();
        assert!(market.is_open(open.with_timezone(&Utc)));
        // 14:29 UTC: one minute before open.
        let before = DateTime::parse_from_rfc3339("2026-01-07T14:29:00Z").unwrap();
        assert!(!market.is_open(before.with_timezone(&Utc)));
        // Saturday: closed.
        let saturday = DateTime::parse_from_rfc3339("2026-01-10T15:00:00Z").unwrap();
        assert!(!market.is_open(saturday.with_timezone(&Utc)));
        // Python advances a day before setting the clock, so a moment before
        // open today still yields tomorrow's open.
        let next = market.next_open(before.with_timezone(&Utc));
        assert_eq!(next.to_rfc3339(), "2026-01-08T14:30:00+00:00");
    }
}
