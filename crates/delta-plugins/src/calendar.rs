//! yfinance calendar data plugin: upcoming earnings and ex-dividend dates as
//! Event rows. Port of `delta/plugins/data/yfinance_calendar.py`.
//!
//! The brief's calendar section reads events with `ts > as_of`, so these rows
//! surface as "Upcoming events" without any further wiring. Ticker mapping is
//! shared with the yfinance bars plugin via `[plugins.<name>].suffixes`.
//!
//! There is no Rust `yfinance`: the calendar is read from Yahoo's
//! quoteSummary `calendarEvents` module (what yfinance's `.calendar` property
//! wraps) through the shared client with its cookie/crumb handshake. The
//! parsed dates keep yfinance's display keys ("Earnings Date",
//! "Ex-Dividend Date") so the event-building rules port one for one.

use std::collections::BTreeMap;

use chrono::{NaiveDate, TimeZone, Utc};
use delta_core::db::StoreItem;
use delta_core::ids::stable_id;
use delta_core::models::{Event, EventKind, Instrument};
use serde_json::Value;

use crate::plugin::{DataPlugin, PluginError, Rows};
use crate::yahoo::{yf_symbol, YahooClient};

/// (event kind, calendar key, summary label) — `CALENDAR_KINDS`.
const CALENDAR_KINDS: [(&str, &str, &str); 2] = [
    ("earnings", "Earnings Date", "Earnings expected"),
    ("dividend", "Ex-Dividend Date", "Ex-dividend"),
];

pub fn calendar_event_id(instrument_id: &str, kind: &str, day: NaiveDate) -> String {
    stable_id(&[instrument_id, kind, &day.format("%Y-%m-%d").to_string()])
}

/// yfinance's `.calendar` shape: display key -> candidate dates.
pub type Calendar = BTreeMap<String, Vec<NaiveDate>>;

/// yfinance gives a list of candidate dates for earnings; take the earliest
/// (`_first_date`).
fn first_date(candidates: Option<&Vec<NaiveDate>>) -> Option<NaiveDate> {
    candidates?.iter().copied().min()
}

/// Normalise a quoteSummary `calendarEvents` payload into yfinance's calendar
/// dict (`_read_calendar`).
pub fn normalize_calendar(payload: &Value) -> Calendar {
    let mut cal: Calendar = BTreeMap::new();
    let events = &payload["quoteSummary"]["result"][0]["calendarEvents"];
    let epoch_date = |value: &Value| -> Option<NaiveDate> {
        let raw = value["raw"].as_i64().or_else(|| value.as_i64())?;
        Utc.timestamp_opt(raw, 0).single().map(|d| d.date_naive())
    };
    let mut push = |key: &str, day: Option<NaiveDate>| {
        if let Some(day) = day {
            cal.entry(key.to_string()).or_default().push(day);
        }
    };
    if let Some(dates) = events["earnings"]["earningsDate"].as_array() {
        for date in dates {
            push("Earnings Date", epoch_date(date));
        }
    }
    push("Ex-Dividend Date", epoch_date(&events["exDividendDate"]));
    push("Dividend Date", epoch_date(&events["dividendDate"]));
    cal
}

/// Calendar rows for one instrument, dropping past days (`fetch`'s body).
pub fn events_from_calendar(inst: &Instrument, cal: &Calendar, today: NaiveDate) -> Vec<Event> {
    let mut events = Vec::new();
    for (kind, key, label) in CALENDAR_KINDS {
        let Some(day) = first_date(cal.get(key)) else {
            continue;
        };
        if day < today {
            continue;
        }
        events.push(Event {
            id: calendar_event_id(&inst.id, kind, day),
            instrument_id: inst.id.clone(),
            ts: day.and_hms_opt(0, 0, 0).unwrap_or_default(),
            kind: EventKind::parse(kind).unwrap_or(EventKind::Other),
            summary: format!("{label} {}", day.format("%Y-%m-%d")),
            sentiment: 0.0,
            evidence_ids: Vec::new(),
            extracted_by: "yfinance".to_string(),
            prompt_version: "n/a".to_string(),
        });
    }
    events
}

/// Upcoming earnings and ex-dividend dates (`YFinanceCalendar`).
#[derive(Default)]
pub struct YfinanceCalendar {
    pub suffixes: BTreeMap<String, String>,
    /// Test seam: empty string means the production URL.
    pub quote_base_url: String,
}

impl YfinanceCalendar {
    fn client(&self) -> YahooClient {
        YahooClient {
            suffixes: self.suffixes.clone(),
            quote_base_url: self.quote_base_url.clone(),
            ..YahooClient::default()
        }
    }

    /// Read one symbol's calendar dict (`_read_calendar`).
    async fn read_calendar(
        &self,
        client: &YahooClient,
        symbol: &str,
    ) -> Result<Calendar, PluginError> {
        let payload = client.quote_summary(symbol, "calendarEvents").await?;
        Ok(normalize_calendar(&payload))
    }
}

impl YfinanceCalendar {
    pub fn configure_suffixes(&mut self, cfg: &Value) {
        if let Some(table) = cfg.get("suffixes").and_then(Value::as_object) {
            for (k, v) in table {
                if let Some(suffix) = v.as_str() {
                    self.suffixes.insert(k.clone(), suffix.to_string());
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl DataPlugin for YfinanceCalendar {
    fn name(&self) -> &'static str {
        "yfinance_calendar"
    }

    fn configure(&mut self, cfg: &Value) {
        self.configure_suffixes(cfg);
    }

    async fn fetch(
        &self,
        instruments: &[Instrument],
        _since: chrono::NaiveDateTime,
    ) -> Result<Rows, PluginError> {
        let today = Utc::now().date_naive();
        let client = self.client();
        // Independent blocking reads in Python run in threads concurrently;
        // the Rust client is async already, so join the per-symbol futures.
        let reads = instruments.iter().map(|inst| {
            let client = &client;
            let symbol = yf_symbol(inst, &self.suffixes);
            async move {
                self.read_calendar(client, &symbol)
                    .await
                    .map(|cal| (inst, cal))
            }
        });
        let results = futures::future::join_all(reads).await;
        let mut events: Vec<Event> = Vec::new();
        for result in results {
            let (inst, cal) = match result {
                Ok(pair) => pair,
                Err(err) => {
                    // One failed symbol must not abort the whole batch.
                    log::error!("calendar fetch failed: {err}");
                    continue;
                }
            };
            if cal.is_empty() {
                continue;
            }
            events.extend(events_from_calendar(inst, &cal, today));
        }
        Ok(events.into_iter().map(StoreItem::Event).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use delta_core::models::Instrument;

    fn inst(id: &str, market: &str, symbol: &str) -> Instrument {
        Instrument {
            id: id.to_string(),
            market: market.to_string(),
            symbol: symbol.to_string(),
            name: None,
            currency: "USD".to_string(),
            sector: None,
            asset_class: delta_core::models::AssetClass::Equity,
            watchlists: Vec::new(),
            tags: Default::default(),
            industry: None,
            meta: Default::default(),
        }
    }

    fn today() -> NaiveDate {
        Utc::now().date_naive()
    }

    #[test]
    fn emits_earnings_and_dividend_events() {
        let aapl = inst("US:AAPL", "us", "AAPL");
        let today = today();
        let earnings = today + chrono::Duration::days(30);
        let ex_div = today + chrono::Duration::days(10);
        let mut cal: Calendar = BTreeMap::new();
        // Earliest of the candidate dates wins.
        cal.insert(
            "Earnings Date".to_string(),
            vec![earnings + chrono::Duration::days(4), earnings],
        );
        cal.insert("Ex-Dividend Date".to_string(), vec![ex_div]);
        cal.insert(
            "Dividend Date".to_string(),
            vec![ex_div + chrono::Duration::days(14)],
        );

        let events = events_from_calendar(&aapl, &cal, today);

        assert_eq!(events.len(), 2);
        let (earnings_event, dividend) = (&events[0], &events[1]);
        assert_eq!(earnings_event.kind, EventKind::Earnings);
        assert_eq!(earnings_event.ts.date(), earnings);
        assert_eq!(
            earnings_event.summary,
            format!("Earnings expected {earnings}")
        );
        assert_eq!(
            earnings_event.id,
            calendar_event_id(&aapl.id, "earnings", earnings)
        );
        assert_eq!(dividend.kind, EventKind::Dividend);
        assert_eq!(dividend.ts.date(), ex_div);
        assert_eq!(dividend.summary, format!("Ex-dividend {ex_div}"));
        assert_eq!(dividend.id, calendar_event_id(&aapl.id, "dividend", ex_div));
        for event in &events {
            assert_eq!(event.instrument_id, aapl.id);
            assert_eq!(event.ts.time().to_string(), "00:00:00");
            assert_eq!(event.sentiment, 0.0);
            assert!(event.evidence_ids.is_empty());
            assert_eq!(event.extracted_by, "yfinance");
            assert_eq!(event.prompt_version, "n/a");
        }
    }

    #[test]
    fn no_calendar_yields_nothing_and_past_dates_are_dropped() {
        let aapl = inst("US:AAPL", "us", "AAPL");
        let today = today();
        assert!(events_from_calendar(&aapl, &Calendar::new(), today).is_empty());
        let mut past: Calendar = BTreeMap::new();
        past.insert(
            "Ex-Dividend Date".to_string(),
            vec![today - chrono::Duration::days(5)],
        );
        assert!(events_from_calendar(&aapl, &past, today).is_empty());
    }

    #[test]
    fn normalize_reads_quote_summary_calendar_events() {
        let earnings = Utc::now().date_naive() + chrono::Duration::days(30);
        let earnings_epoch = earnings.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        let ex_div = Utc::now().date_naive() + chrono::Duration::days(10);
        let ex_div_epoch = ex_div.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        let payload = serde_json::json!({
            "quoteSummary": {
                "result": [{
                    "calendarEvents": {
                        "earnings": {
                            "earningsDate": [{"raw": earnings_epoch}, {"raw": earnings_epoch + 4 * 86400}],
                            "earningsAverage": {"raw": 1.5},
                        },
                        "exDividendDate": {"raw": ex_div_epoch},
                    }
                }]
            }
        });
        let cal = normalize_calendar(&payload);
        assert_eq!(first_date(cal.get("Earnings Date")), Some(earnings));
        assert_eq!(first_date(cal.get("Ex-Dividend Date")), Some(ex_div));
        assert!(!cal.contains_key("Dividend Date"));
        // A payload without calendarEvents is an empty calendar.
        assert!(normalize_calendar(&serde_json::json!({})).is_empty());
    }

    /// quoteSummary needs the cookie/crumb handshake; mock both plus the
    /// module endpoint (`_patch`/`_FakeTicker` in the Python tests).
    async fn wiremock_calendar(
        symbols: &[(&str, serde_json::Value)],
    ) -> (wiremock::MockServer, Vec<(&'static str, String)>) {
        use wiremock::matchers::{method, path, query_param};
        use wiremock::{Mock, ResponseTemplate};

        let server = wiremock::MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/cookie"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/getcrumb"))
            .respond_with(ResponseTemplate::new(200).set_body_string("crumb"))
            .mount(&server)
            .await;
        let mut requested: Vec<(&'static str, String)> = Vec::new();
        for (symbol, calendar) in symbols {
            Mock::given(method("GET"))
                .and(path(format!("/v10/finance/quoteSummary/{symbol}")))
                .and(query_param("modules", "calendarEvents"))
                .respond_with(ResponseTemplate::new(200).set_body_json(calendar))
                .mount(&server)
                .await;
            requested.push(("quoteSummary", (*symbol).to_string()));
        }
        (server, requested)
    }

    fn calendar_payload(earnings_epoch: i64, ex_div_epoch: i64) -> serde_json::Value {
        serde_json::json!({
            "quoteSummary": {"result": [{"calendarEvents": {
                "earnings": {"earningsDate": [{"raw": earnings_epoch}]},
                "exDividendDate": {"raw": ex_div_epoch},
            }}]}
        })
    }

    #[tokio::test]
    async fn fetch_maps_suffixes_and_skips_failed_symbols() {
        let aapl = inst("US:AAPL", "us", "AAPL");
        let bhp = inst("ASX:BHP", "asx", "BHP");
        let epoch = |day: NaiveDate| day.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp();
        let today = today();
        let earnings = today + chrono::Duration::days(30);
        let ex_div = today + chrono::Duration::days(10);

        let (server, _) = wiremock_calendar(&[
            ("AAPL", serde_json::json!({"quoteSummary": {"result": []}})),
            ("BHP.AX", calendar_payload(epoch(earnings), epoch(ex_div))),
        ])
        .await;

        let mut plugin = YfinanceCalendar {
            quote_base_url: server.uri(),
            ..YfinanceCalendar::default()
        };
        plugin.configure(&serde_json::json!({"suffixes": {"asx": ".AX"}}));

        let rows = plugin
            .fetch(
                &[bhp.clone(), aapl.clone()],
                today.and_hms_opt(0, 0, 0).unwrap(),
            )
            .await
            .expect("fetch");

        // The failed/empty AAPL calendar yields nothing; BHP emits both events.
        assert_eq!(rows.len(), 2);
        for row in &rows {
            match row {
                StoreItem::Event(event) => assert_eq!(event.instrument_id, bhp.id),
                other => panic!("unexpected row: {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn suffixes_are_shared_with_the_bars_plugin() {
        let shel = inst("LSE:SHEL", "lse", "SHEL");
        let mut bars = crate::yahoo::YfinanceBars::default();
        let mut cal = YfinanceCalendar::default();
        let cfg = serde_json::json!({"suffixes": {"lse": ".L"}});
        bars.configure(&cfg);
        cal.configure(&cfg);
        assert_eq!(yf_symbol(&shel, &bars.suffixes), "SHEL.L");
        assert_eq!(yf_symbol(&shel, &cal.suffixes), "SHEL.L");
    }
}
