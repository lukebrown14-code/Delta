//! Upcoming earnings and ex-dividend events from Yahoo calendar data.

use std::collections::BTreeMap;

use chrono::{DateTime, NaiveDate, NaiveDateTime, Utc};
use delta_core::{
    db::StoreItem,
    ids::stable_id,
    models::{Event, EventKind, Instrument},
};
use serde_json::Value;

use crate::{
    plugin::{DataPlugin, PluginError},
    yahoo::{default_suffixes, yf_symbol, YahooClient},
};

pub fn calendar_event_id(instrument_id: &str, kind: EventKind, day: NaiveDate) -> String {
    stable_id(&[instrument_id, kind.as_str(), &day.to_string()])
}

fn epoch_date(value: &Value) -> Option<NaiveDate> {
    let seconds = value.get("raw").unwrap_or(value).as_i64()?;
    DateTime::<Utc>::from_timestamp(seconds, 0).map(|dt| dt.date_naive())
}

pub fn calendar_events(inst: &Instrument, payload: &Value, today: NaiveDate) -> Vec<Event> {
    let Some(root) = payload.pointer("/quoteSummary/result/0/calendarEvents") else {
        return Vec::new();
    };
    let earnings = root
        .pointer("/earnings/earningsDate")
        .and_then(Value::as_array)
        .and_then(|dates| dates.iter().filter_map(epoch_date).min());
    let dividend = root.get("exDividendDate").and_then(epoch_date);
    [
        (EventKind::Earnings, earnings, "Earnings expected"),
        (EventKind::Dividend, dividend, "Ex-dividend"),
    ]
    .into_iter()
    .filter_map(|(kind, day, label)| {
        let day = day.filter(|d| *d >= today)?;
        Some(Event {
            id: calendar_event_id(&inst.id, kind, day),
            instrument_id: inst.id.clone(),
            ts: day.and_hms_opt(0, 0, 0).unwrap(),
            kind,
            summary: format!("{label} {day}"),
            sentiment: 0.0,
            evidence_ids: Vec::new(),
            extracted_by: "yfinance".into(),
            prompt_version: "n/a".into(),
        })
    })
    .collect()
}

pub struct YfinanceCalendar {
    pub suffixes: BTreeMap<String, String>,
}

impl Default for YfinanceCalendar {
    fn default() -> Self {
        Self {
            suffixes: default_suffixes(),
        }
    }
}

#[async_trait::async_trait]
impl DataPlugin for YfinanceCalendar {
    fn name(&self) -> &'static str {
        "yfinance_calendar"
    }
    fn configure(&mut self, cfg: &Value) {
        if let Some(suffixes) = cfg.get("suffixes").and_then(Value::as_object) {
            for (market, suffix) in suffixes {
                if let Some(suffix) = suffix.as_str() {
                    self.suffixes.insert(market.clone(), suffix.to_string());
                }
            }
        }
    }
    async fn fetch(
        &self,
        instruments: &[Instrument],
        _since: NaiveDateTime,
    ) -> Result<Vec<StoreItem>, PluginError> {
        let client = YahooClient {
            suffixes: self.suffixes.clone(),
            ..YahooClient::default()
        };
        let today = Utc::now().date_naive();
        let mut rows = Vec::new();
        for inst in instruments {
            let symbol = yf_symbol(inst, &client.suffixes);
            match client.quote_summary(&symbol, "calendarEvents").await {
                Ok(payload) => rows.extend(
                    calendar_events(inst, &payload, today)
                        .into_iter()
                        .map(StoreItem::Event),
                ),
                Err(error) => log::warn!("calendar fetch failed for {}: {error}", inst.id),
            }
        }
        Ok(rows)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delta_core::models::AssetClass;

    #[test]
    fn keeps_upcoming_calendar_dates_and_stable_ids() {
        let inst = Instrument {
            id: "US:AAPL".into(),
            market: "us".into(),
            symbol: "AAPL".into(),
            name: None,
            currency: "USD".into(),
            sector: None,
            asset_class: AssetClass::Equity,
            watchlists: vec![],
            tags: Default::default(),
            industry: None,
            meta: Default::default(),
        };
        let payload = serde_json::json!({"quoteSummary":{"result":[{"calendarEvents":{"earnings":{"earningsDate":[{"raw":1798761600},{"raw":1796083200}]},"exDividendDate":{"raw":1796083200}}}]}});
        let today = NaiveDate::from_ymd_opt(2026, 10, 1).unwrap();
        let rows = calendar_events(&inst, &payload, today);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].kind, EventKind::Earnings);
        assert_eq!(
            rows[0].id,
            calendar_event_id("US:AAPL", EventKind::Earnings, rows[0].ts.date())
        );
    }
}
