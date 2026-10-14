//! Market plugins: universe construction and session times
//! (ports of `delta/plugins/markets/us.py` and `asx.py`).

use chrono::{DateTime, Datelike, Duration, MappedLocalTime, NaiveTime, TimeZone, Timelike, Utc};
use chrono_tz::{America::New_York, Australia::Sydney, Tz};
use delta_core::ids::make_instrument_id;
use delta_core::models::Instrument;
use serde_json::Value;

use crate::plugin::DataPlugin;

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
    /// Sessions follow `America/New_York`, DST included (finding rust-plugins #6).
    fn tz(&self) -> Tz {
        New_York
    }

    fn local(&self, ts: DateTime<Utc>) -> DateTime<Tz> {
        ts.with_timezone(&self.tz())
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

    /// Port of `USMarket.next_open`: advance one wall-clock day, stamp the
    /// local open on it (so a DST shift moves the UTC instant with the local
    /// open), then skip weekends.
    pub fn next_open(&self, ts: DateTime<Utc>) -> DateTime<Utc> {
        let tz = self.tz();
        let mut naive = ts.with_timezone(&tz).naive_local();
        for _ in 0..14 {
            naive = (naive + Duration::days(1))
                .with_hour(MARKET_OPEN.0)
                .and_then(|c| c.with_minute(MARKET_OPEN.1))
                .and_then(|c| c.with_second(0))
                .and_then(|c| c.with_nanosecond(0))
                .unwrap_or(naive + Duration::days(1));
            // 09:30 local is never inside a 2–3 AM transition, so the local
            // resolution is unambiguous in both zones.
            if let MappedLocalTime::Single(candidate) = tz.from_local_datetime(&naive) {
                if candidate.weekday().number_from_monday() < 6 {
                    return candidate.with_timezone(&Utc);
                }
            }
        }
        ts
    }
}

/// ASX market (`AsxMarket`); sessions are 10:00–16:00 Sydney time, DST
/// included (finding rust-plugins #6).
#[derive(Default, Clone)]
pub struct AsxMarket {
    pub tickers: Vec<String>,
}

impl AsxMarket {
    fn tz(&self) -> Tz {
        Sydney
    }

    fn local(&self, ts: DateTime<Utc>) -> DateTime<Tz> {
        ts.with_timezone(&self.tz())
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

    /// Port of `ASXMarket.next_open`: stamp today's wall-clock open first; if
    /// that moment has already passed, advance a day, then skip weekends. Day
    /// steps are wall-clock (`timedelta(days=1)`), so 10:00 local stays 10:00
    /// across an offset change.
    pub fn next_open(&self, ts: DateTime<Utc>) -> DateTime<Utc> {
        let tz = self.tz();
        let local_now = ts.with_timezone(&tz);
        let mut naive = local_now
            .naive_local()
            .with_hour(10)
            .and_then(|c| c.with_minute(0))
            .and_then(|c| c.with_second(0))
            .and_then(|c| c.with_nanosecond(0))
            .unwrap_or_else(|| local_now.naive_local());
        // 10:00 local never falls inside a 2–3 AM transition, so resolution
        // is unambiguous and never None.
        let mut candidate = match tz.from_local_datetime(&naive) {
            MappedLocalTime::Single(dt) => dt,
            _ => local_now,
        };
        if candidate <= local_now {
            naive += Duration::days(1);
            if let MappedLocalTime::Single(dt) = tz.from_local_datetime(&naive) {
                candidate = dt;
            }
        }
        while candidate.weekday().number_from_monday() >= 6 {
            naive += Duration::days(1);
            if let MappedLocalTime::Single(dt) = tz.from_local_datetime(&naive) {
                candidate = dt;
            }
        }
        candidate.with_timezone(&Utc)
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
