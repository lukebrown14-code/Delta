//! Market plugins: universe construction and session times
//! (ports of `delta/plugins/markets/us.py` and `asx.py`).

use chrono::{DateTime, Datelike, Duration, NaiveDate, NaiveTime, Utc};
use delta_core::ids::make_instrument_id;
use delta_core::models::Instrument;
use serde_json::Value;

use crate::plugin::DataPlugin;

fn nth_sunday(year: i32, month: u32, nth: u32) -> NaiveDate {
    let first = NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let delta = (7 - first.weekday().num_days_from_sunday()) % 7;
    first + Duration::days(i64::from(delta + 7 * (nth - 1)))
}

fn us_offset(ts: DateTime<Utc>) -> i32 {
    let year = ts.year();
    let start = nth_sunday(year, 3, 2).and_hms_opt(7, 0, 0).unwrap();
    let end = nth_sunday(year, 11, 1).and_hms_opt(6, 0, 0).unwrap();
    if ts.naive_utc() >= start && ts.naive_utc() < end {
        -4 * 3600
    } else {
        -5 * 3600
    }
}

fn us_offset_for_open(day: NaiveDate) -> i32 {
    if day >= nth_sunday(day.year(), 3, 2) && day < nth_sunday(day.year(), 11, 1) {
        -4 * 3600
    } else {
        -5 * 3600
    }
}

fn asx_offset(ts: DateTime<Utc>) -> i32 {
    let year = ts.year();
    let end = nth_sunday(year, 4, 1).and_hms_opt(16, 0, 0).unwrap() - Duration::days(1);
    let start = nth_sunday(year, 10, 1).and_hms_opt(16, 0, 0).unwrap() - Duration::days(1);
    if ts.naive_utc() < end || ts.naive_utc() >= start {
        11 * 3600
    } else {
        10 * 3600
    }
}

fn asx_offset_for_open(day: NaiveDate) -> i32 {
    if day >= nth_sunday(day.year(), 10, 1) || day < nth_sunday(day.year(), 4, 1) {
        11 * 3600
    } else {
        10 * 3600
    }
}

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
        ts.with_timezone(&chrono::FixedOffset::east_opt(us_offset(ts)).unwrap())
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
        let mut day = self.local(ts).date_naive();
        for _ in 0..14 {
            day += Duration::days(1);
            if day.weekday().number_from_monday() < 6 {
                let local = day.and_hms_opt(MARKET_OPEN.0, MARKET_OPEN.1, 0).unwrap();
                return DateTime::from_naive_utc_and_offset(
                    local - Duration::seconds(i64::from(us_offset_for_open(day))),
                    Utc,
                );
            }
        }
        ts
    }
}

/// ASX market (`AsxMarket`); sessions follow Sydney daylight saving time.
#[derive(Default, Clone)]
pub struct AsxMarket {
    pub tickers: Vec<String>,
}

impl AsxMarket {
    fn local(&self, ts: DateTime<Utc>) -> DateTime<chrono::FixedOffset> {
        ts.with_timezone(&chrono::FixedOffset::east_opt(asx_offset(ts)).unwrap())
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
        let mut day = self.local(ts).date_naive();
        for _ in 0..14 {
            day += Duration::days(1);
            if day.weekday().number_from_monday() < 6 {
                let local = day.and_hms_opt(10, 0, 0).unwrap();
                return DateTime::from_naive_utc_and_offset(
                    local - Duration::seconds(i64::from(asx_offset_for_open(day))),
                    Utc,
                );
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

    #[test]
    fn summer_sessions_follow_daylight_saving() {
        let us = UsMarket::default();
        let summer = DateTime::parse_from_rfc3339("2026-07-06T13:30:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(us.is_open(summer));
        assert_eq!(
            us.next_open(summer).to_rfc3339(),
            "2026-07-07T13:30:00+00:00"
        );
        let asx = AsxMarket::default();
        let summer = DateTime::parse_from_rfc3339("2026-01-05T23:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        assert!(asx.is_open(summer));
        assert_eq!(
            asx.next_open(summer).to_rfc3339(),
            "2026-01-06T23:00:00+00:00"
        );
    }
}
