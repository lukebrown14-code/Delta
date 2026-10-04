//! The desk: the screen states the app paints, loaded from the real
//! `config.toml` + `data/delta.db` (via `delta-services` / `delta-core`),
//! with an explicit unavailable state when the configured data cannot load.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDateTime, Utc};
use delta_core::config::load_config;
use delta_core::db::Db;
use delta_core::models::{AssetClass, Bar, Instrument};
use delta_services::analytics;

use crate::braille::BrailleGraph;
use crate::screens::{grouped, HomeFeed, HomeState, MetricsData, WatchlistState};

/// Golden-exporter constants: 80 seeded bars.
pub const BAR_COUNT: usize = 80;

/// `_price(i)` — the shared bar seed (`tests/export_golden.py`).
pub fn price(i: usize) -> f64 {
    let v = 200.0 + i as f64 * 0.4 + 6.0 * (i as f64 / 4.0).sin();
    (v * 100.0).round() / 100.0
}

/// One watched instrument and its stored daily closes.
pub struct InstrumentDesk {
    pub instrument: Instrument,
    /// `(close, "YYYY-MM-DDT00:00:00+00:00")` ascending.
    pub bars: Vec<(f64, String)>,
}

impl InstrumentDesk {
    fn offline(symbol: &str) -> Self {
        let last_day = (Utc::now().naive_utc() - Duration::days(1)).date();
        let start = last_day - Duration::days(BAR_COUNT as i64 - 1);
        let bars = (0..BAR_COUNT)
            .map(|i| {
                let day = start + Duration::days(i as i64);
                (
                    price(i),
                    format!("{}T00:00:00+00:00", day.format("%Y-%m-%d")),
                )
            })
            .collect();
        Self {
            instrument: Instrument {
                id: format!("US:{symbol}"),
                market: "us".to_string(),
                symbol: symbol.to_string(),
                name: None,
                currency: "USD".to_string(),
                sector: None,
                asset_class: AssetClass::Equity,
                watchlists: vec!["offline".to_string()],
                tags: Default::default(),
                industry: None,
                meta: Default::default(),
            },
            bars,
        }
    }

    fn from_db(inst: Instrument, bars: Vec<Bar>) -> Self {
        let series = bars
            .into_iter()
            .map(|b| {
                (
                    b.close,
                    format!("{}T00:00:00+00:00", b.ts.format("%Y-%m-%d")),
                )
            })
            .collect();
        Self {
            instrument: inst,
            bars: series,
        }
    }
}

/// Where the desk's data came from (shown in the status overlay).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// `config.toml` + the configured SQLite DB.
    Real(PathBuf),
    /// The seeded in-memory golden environment.
    Offline,
    /// A real launch could not load configured watch data.
    Unavailable(String),
}

/// One desk: instruments, the selected one, the range, and live quotes.
pub struct Desk {
    pub last_seen: NaiveDateTime,
    pub instruments: Vec<InstrumentDesk>,
    pub selected: usize,
    pub range_index: usize,
    /// Live quotes keyed by instrument id (`regularMarketPrice`).
    pub live: BTreeMap<String, f64>,
    /// Inspector metric rows keyed by instrument id (metrics worker).
    pub metrics: BTreeMap<String, Vec<(String, String)>>,
    pub asset_metrics: BTreeMap<(String, String), delta_services::asset_metrics::AssetMetrics>,
    pub source: Source,
    /// Last ingest counts, when a gather has run this session.
    pub last_ingest: Option<BTreeMap<String, usize>>,
    /// Live Home overview values (headline, pulse, upcoming, staleness).
    pub feed: HomeFeed,
}

impl Default for Desk {
    fn default() -> Self {
        Self::open()
    }
}

impl Desk {
    /// Load the real desk from `config.toml` + its DB. The golden seed is only
    /// available through `offline()` and is never presented as live data.
    pub fn open() -> Desk {
        Self::open_at(std::path::Path::new("config.toml"))
    }

    pub fn open_at(config_path: &std::path::Path) -> Desk {
        match Self::try_real(config_path) {
            Ok((instruments, db_path, feed, last_seen)) => Desk {
                last_seen,
                instruments,
                selected: 0,
                range_index: 2, // "1m"
                live: BTreeMap::new(),
                metrics: BTreeMap::new(),
                asset_metrics: BTreeMap::new(),
                source: Source::Real(db_path),
                last_ingest: None,
                feed,
            },
            Err(reason) => Desk {
                last_seen: now_naive() - Duration::days(7),
                instruments: Vec::new(),
                selected: 0,
                range_index: 2,
                live: BTreeMap::new(),
                metrics: BTreeMap::new(),
                asset_metrics: BTreeMap::new(),
                source: Source::Unavailable(reason),
                last_ingest: None,
                feed: HomeFeed::default(),
            },
        }
    }

    fn try_real(
        config_path: &std::path::Path,
    ) -> Result<(Vec<InstrumentDesk>, PathBuf, HomeFeed, NaiveDateTime), String> {
        if !config_path.exists() {
            return Err("config.toml is missing".to_string());
        }
        let (_, app) = load_config(config_path).map_err(|e| e.to_string())?;
        let db_path = PathBuf::from(&app.db_path);
        let universe =
            delta_services::configured_universe(config_path).map_err(|e| e.to_string())?;
        if universe.is_empty() {
            return Err("no watch targets with tickers are configured".to_string());
        }
        let db = Db::open(&db_path).map_err(|e| e.to_string())?;
        let mut instruments: Vec<InstrumentDesk> = Vec::new();
        for inst in universe {
            let bars = db.bars(&inst.id).map_err(|e| e.to_string())?;
            instruments.push(InstrumentDesk::from_db(inst, bars));
        }
        let ids: Vec<String> = instruments
            .iter()
            .map(|d| d.instrument.id.clone())
            .collect();
        let last_seen = delta_core::state::read_last_seen(&app.db_path).naive_utc();
        let feed = load_feed_since(&db, &ids, last_seen);
        Ok((instruments, db_path, feed, last_seen))
    }

    /// The offline golden environment (in-memory; nothing touches disk).
    pub fn offline() -> Desk {
        Desk {
            last_seen: now_naive() - Duration::days(7),
            instruments: vec![InstrumentDesk::offline("AAPL")],
            selected: 0,
            range_index: 2, // "1m"
            live: BTreeMap::new(),
            metrics: BTreeMap::new(),
            asset_metrics: BTreeMap::new(),
            source: Source::Offline,
            last_ingest: None,
            feed: HomeFeed::seed(),
        }
    }

    /// The instrument the inspector shows.
    pub fn current(&self) -> &InstrumentDesk {
        &self.instruments[self.selected.min(self.instruments.len() - 1)]
    }

    pub fn cycle_instrument(&mut self, delta: isize) {
        if self.instruments.is_empty() {
            return;
        }
        let n = self.instruments.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(n)) as usize;
    }

    /// The watchlist `RANGES` entry currently selected.
    pub fn range(&self) -> &'static str {
        crate::screens::RANGES[self.range_index]
    }

    pub fn cycle_range(&mut self, delta: isize) {
        let n = crate::screens::RANGES.len() as isize;
        self.range_index = ((self.range_index as isize + delta).rem_euclid(n)) as usize;
    }

    /// The live quote for the current instrument, when a quote worker has
    /// reported one.
    pub fn live_price(&self) -> Option<f64> {
        self.instruments
            .get(self.selected)
            .and_then(|desk| self.live.get(&desk.instrument.id))
            .copied()
    }

    /// The inspector state for the current instrument and range.
    pub fn watch_state(&self) -> WatchlistState {
        if self.instruments.is_empty() {
            return WatchlistState {
                range: self.range(),
                metric: None,
            };
        }
        let cur = self.current();
        let range = self.range();
        if let Some(metric) = self
            .asset_metrics
            .get(&(cur.instrument.id.clone(), range.to_string()))
        {
            let label = if metric.profile == "bond" {
                "Current yield"
            } else {
                "Current price"
            };
            let current = self
                .live_price()
                .map(grouped)
                .or_else(|| metric.values.get(label).cloned())
                .unwrap_or_else(|| "—".into());
            let mut values = vec![(
                label.into(),
                metric
                    .values
                    .get(label)
                    .cloned()
                    .unwrap_or_else(|| current.clone()),
            )];
            for (_, group) in &metric.groups {
                values.extend(group.clone());
            }
            if values.len() == 1 {
                values.extend(
                    metric
                        .values
                        .iter()
                        .filter(|(key, _)| key.as_str() != label)
                        .map(|(key, value)| (key.clone(), value.clone())),
                );
            }
            return WatchlistState {
                range,
                metric: Some(MetricsData {
                    symbol: cur.instrument.symbol.clone(),
                    market: cur.instrument.market.clone(),
                    asset_class: cur.instrument.asset_class.as_str().into(),
                    currency: cur.instrument.currency.clone(),
                    current,
                    change_label: (!metric.change_label.is_empty())
                        .then(|| metric.change_label.clone()),
                    period_high: metric.period_high,
                    period_low: metric.period_low,
                    history_start: metric.history_start.clone(),
                    history_end: metric.history_end.clone(),
                    series: metric.series.clone(),
                    series_times: metric.series_times.clone(),
                    source: metric
                        .error
                        .as_ref()
                        .map(|error| format!("metrics unavailable: {error} — press Enter to retry"))
                        .unwrap_or_else(|| metric.source.clone()),
                    values,
                }),
            };
        }
        let window = crate::screens::range_window(range);
        let start = window.map_or(0, |n| cur.bars.len().saturating_sub(n));
        let picked: Vec<(f64, String)> = cur.bars[start..].to_vec();
        let series: Vec<f64> = picked.iter().map(|(c, _)| *c).collect();
        let times: Vec<String> = picked.iter().map(|(_, t)| t.clone()).collect();
        // Python keeps historical chart points and their timestamps paired;
        // the live quote updates the hero value without altering the history.
        let live = self.live_price();
        if series.is_empty() {
            return WatchlistState {
                range,
                metric: None,
            };
        }
        let last = series[series.len() - 1];
        let first = series[0];
        let change = if first != 0.0 {
            (last / first - 1.0) * 100.0
        } else {
            0.0
        };
        let hi = series.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = series.iter().cloned().fold(f64::INFINITY, f64::min);
        let mut values = vec![("Current price".to_string(), grouped(live.unwrap_or(last)))];
        if live.is_some() {
            values.push(("Source".to_string(), "live quote".to_string()));
        }
        // Provider metrics in table order; capped to the two-column grid's
        // visible rows so the source line never scrolls out of the pane.
        if let Some(rows) = self.metrics.get(&cur.instrument.id) {
            values.extend(rows.iter().take(24).cloned());
        }
        WatchlistState {
            range,
            metric: Some(MetricsData {
                symbol: cur.instrument.symbol.clone(),
                market: cur.instrument.market.clone(),
                asset_class: cur.instrument.asset_class.as_str().to_string(),
                currency: cur.instrument.currency.clone(),
                current: grouped(live.unwrap_or(last)),
                change_label: Some(format!("{change:+.1}%")),
                period_high: Some(hi),
                period_low: Some(lo),
                history_start: times.first().cloned(),
                history_end: times.last().cloned(),
                series,
                series_times: times,
                source: if live.is_some() {
                    "live quote".to_string()
                } else {
                    "Yahoo Finance".to_string()
                },
                values,
            }),
        }
    }

    /// The home screen state (clock, headline quote, sparkline).
    pub fn home_state(&self) -> HomeState {
        let cur = self.current();
        let mut closes: Vec<f64> = cur.bars.iter().map(|(c, _)| *c).collect();
        if let Some(px) = self.live_price() {
            closes.push(px);
        }
        let last = closes.last().copied();
        let prev = closes.iter().rev().nth(1).copied();
        let chg_label = match (last, prev) {
            (Some(last), Some(prev)) if prev != 0.0 => {
                format!("{:+.2}%", (last / prev - 1.0) * 100.0)
            }
            _ => "—".to_string(),
        };
        let spark = BrailleGraph::filled(closes.clone()).rows(17, 1)[0].clone();
        let now = Utc::now();
        HomeState {
            clock: now.format("%A %d %B %Y · %H:%M:%S UTC").to_string(),
            symbol: cur.instrument.symbol.clone(),
            last: last.map_or_else(|| "no bars".to_string(), grouped),
            chg_label,
            spark,
            since_stamp: format!("since {}", self.last_seen.format("%a %H:%M")),
            closes,
            feed: self.feed.clone(),
        }
    }

    /// Reload stored bars from `db_path` (after a gather).
    pub fn reload_from(&mut self, db_path: &Path) {
        let Ok(db) = Db::open(db_path) else {
            return;
        };
        for desk in &mut self.instruments {
            if let Ok(bars) = db.bars(&desk.instrument.id) {
                desk.bars = bars
                    .into_iter()
                    .map(|b| {
                        (
                            b.close,
                            format!("{}T00:00:00+00:00", b.ts.format("%Y-%m-%d")),
                        )
                    })
                    .collect();
            }
        }
        // The gather just wrote new rows; refresh the overview values too.
        let ids: Vec<String> = self
            .instruments
            .iter()
            .map(|d| d.instrument.id.clone())
            .collect();
        self.feed = load_feed_since(&db, &ids, self.last_seen);
    }
}

/// Build the Home overview feed from the analytics queries (all local SQL).
/// The seeded values stay wherever a query fails or finds nothing.
pub fn load_feed(db: &Db, instrument_ids: &[String]) -> HomeFeed {
    load_feed_since(db, instrument_ids, now_naive() - Duration::days(1))
}

pub fn load_feed_since(db: &Db, instrument_ids: &[String], last_seen: NaiveDateTime) -> HomeFeed {
    let now = now_naive();
    // Slots stay unset (and the painters fall back to the golden seed text)
    // unless the query succeeded and found something — a failed query must
    // not present stale seed strings as fresh data.
    let mut feed = HomeFeed::default();
    let due = delta_services::due_reviews(db, now.date())
        .map(|rows| rows.len())
        .unwrap_or(0);
    let fleet = delta_services::thesis_fleet(db, Some(now)).unwrap_or_default();
    let hits = fleet
        .iter()
        .filter(|row| {
            row.result
                .as_ref()
                .is_some_and(|health| health.state == delta_services::HealthState::Challenged)
        })
        .count();
    feed.fleet = fleet
        .iter()
        .map(|row| {
            (
                row.thesis.claim.clone(),
                format!("{:?}", row.state()).to_lowercase(),
            )
        })
        .collect();
    let earnings = analytics::upcoming_events(db, instrument_ids, usize::MAX, now)
        .unwrap_or_default()
        .iter()
        .filter(|event| event.kind == "earnings" && event.ts <= now + Duration::days(7))
        .count();
    let stale = analytics::data_health(db)
        .map(|health| {
            instrument_ids
                .iter()
                .filter(|id| {
                    health
                        .latest_bar
                        .get(*id)
                        .is_none_or(|ts| now - *ts > Duration::days(3))
                })
                .count()
        })
        .unwrap_or(0);
    feed.agenda = Some([
        if due == 0 {
            "✓ no decision reviews due".into()
        } else {
            format!("⚠ {due} decision reviews due")
        },
        if hits == 0 {
            "✓ no falsifier hits".into()
        } else {
            format!("⚠ {hits} theses with falsifier hits")
        },
        if earnings == 0 {
            "✓ no earnings in the next 7 days".into()
        } else {
            format!("⚠ {earnings} earnings in the next 7 days")
        },
        if stale == 0 {
            "✓ no stale price sources".into()
        } else {
            format!("⚠ {stale} stale price sources")
        },
    ]);
    if let Ok(health) = analytics::data_health(db) {
        if let Some(newest) = health.latest_bar.values().max() {
            let age = (now - *newest).num_days().max(0);
            feed.stale_age = Some(format!("{age}d old"));
        } else {
            feed.stale_age = Some("no bars".to_string());
        }
    }
    if let Ok(pulse) = analytics::pulse(db, instrument_ids, last_seen, 30, now) {
        let new_items = pulse.total();
        if new_items > 0 {
            feed.since_line = Some(truncate(
                &format!("{new_items} new items since your last visit"),
                54,
            ));
            feed.since_brief = Some(format!("{new_items} new"));
        }
        let window_total: usize = pulse.daily.iter().sum();
        if window_total > 0 {
            let spend = analytics::total_spend(db, Some(now - Duration::days(30))).max(0.0);
            feed.activity_line = Some(truncate(
                &format!("{window_total} items in the last 30 days · ${spend:.2} LLM spend"),
                54,
            ));
        }
    }
    if let Ok(Some(headline)) = analytics::latest_headline(db, instrument_ids, 50) {
        feed.newest_line = Some(format!("newest   {}", truncate(&headline.title, 45)));
    }
    if let Ok(events) = analytics::upcoming_events(db, instrument_ids, 1, now) {
        if let Some(event) = events.first() {
            let when = event.ts.format("%a %d %b").to_string();
            feed.upcoming_line = Some(truncate(
                &format!("{when}  {} · {}", event.kind, event.summary),
                54,
            ));
            feed.upcoming_brief = Some(truncate(&format!("{when} {}", event.kind), 20));
        }
    }
    feed
}

/// Clip `text` to at most `max` display columns (unicode width, so CJK and
/// emoji count double), marking a cut with an ellipsis.
fn truncate(text: &str, max: usize) -> String {
    use unicode_width::UnicodeWidthStr;
    if text.width() <= max {
        return text.to_string();
    }
    let mut cut = String::new();
    for ch in text.chars() {
        let mut buf = [0u8; 4];
        let w = ch.encode_utf8(&mut buf).width();
        if cut.width() + w > max - 1 {
            break;
        }
        cut.push(ch);
    }
    cut.push('\u{2026}');
    cut
}

/// Wall-clock now as the naive-UTC the pipeline speaks.
pub fn now_naive() -> NaiveDateTime {
    Utc::now().naive_utc()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use delta_core::db::StoreItem;
    use delta_core::models::{Event, EventKind, NewsItem};

    /// A bar timestamped `days_before` before "now", so ages are stable.
    fn bar(days_before: i64) -> Bar {
        Bar {
            instrument_id: "US:AAPL".to_string(),
            ts: now_naive() - Duration::days(days_before),
            open: 1.0,
            high: 2.0,
            low: 0.5,
            close: 1.5,
            volume: 100.0,
            source: "yahoo".to_string(),
        }
    }

    #[test]
    fn load_feed_reads_analytics_into_the_overview_lines() {
        let mut db = Db::open_memory().unwrap();
        let items: Vec<StoreItem> = vec![
            bar(3).into(),
            NewsItem {
                id: "n1".to_string(),
                instrument_ids: vec!["US:AAPL".to_string()],
                published: now_naive() - Duration::hours(2),
                title: "Apple announces new chip".to_string(),
                url: "https://example.com".to_string(),
                body: None,
                source: "rss".to_string(),
            }
            .into(),
            Event {
                id: "e1".to_string(),
                instrument_id: "US:AAPL".to_string(),
                ts: NaiveDate::from_ymd_opt(2099, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap(),
                kind: EventKind::Earnings,
                summary: "Q4 earnings".to_string(),
                sentiment: 0.0,
                evidence_ids: vec![],
                extracted_by: "m".to_string(),
                prompt_version: "1".to_string(),
            }
            .into(),
        ];
        db.store_items(&items).unwrap();

        let feed = load_feed(&db, &["US:AAPL".to_string()]);
        assert_eq!(feed.stale_age.as_deref(), Some("3d old"));
        assert_eq!(
            feed.since_line.as_deref(),
            Some("1 new items since your last visit")
        );
        assert_eq!(feed.since_brief.as_deref(), Some("1 new"));
        assert_eq!(
            feed.activity_line.as_deref(),
            Some("1 items in the last 30 days · $0.00 LLM spend")
        );
        assert_eq!(
            feed.newest_line.as_deref(),
            Some("newest   Apple announces new chip")
        );
        assert!(feed
            .upcoming_line
            .as_deref()
            .is_some_and(|l| l.starts_with("Thu 01 Jan  earnings · Q4 earnings")));
        assert_eq!(feed.upcoming_brief.as_deref(), Some("Thu 01 Jan earnings"));
    }

    #[test]
    fn load_feed_truncates_long_titles_to_the_pane_width() {
        let mut db = Db::open_memory().unwrap();
        let long_title = "X".repeat(80);
        db.store_items(&[NewsItem {
            id: "n1".to_string(),
            instrument_ids: vec!["US:AAPL".to_string()],
            published: now_naive() - Duration::hours(2),
            title: long_title,
            url: "u".to_string(),
            body: None,
            source: "rss".to_string(),
        }
        .into()])
            .unwrap();
        let feed = load_feed(&db, &["US:AAPL".to_string()]);
        // "newest   " + 44 X's + the ellipsis = exactly the pane's 54 cells.
        let newest = feed.newest_line.as_deref().unwrap_or_default();
        assert_eq!(newest.chars().count(), 54);
        assert!(newest.ends_with('\u{2026}'));
    }

    #[test]
    fn offline_desk_keeps_the_seeded_feed() {
        let desk = Desk::offline();
        assert_eq!(desk.feed, HomeFeed::seed());
        assert_eq!(desk.home_state().feed, HomeFeed::seed());
    }

    #[test]
    fn watched_instrument_without_bars_has_no_price() {
        let mut desk = Desk::offline();
        desk.instruments[0].bars.clear();
        assert!(desk.watch_state().metric.is_none());
        assert_eq!(desk.home_state().last, "no bars");
    }
}
