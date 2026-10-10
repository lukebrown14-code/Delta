//! The desk: the screen states the app paints, loaded from the real
//! `config.toml` + `data/delta.db` (via `delta-services` / `delta-core`),
//! falling back to the offline golden seed when either is absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDateTime, Utc};
use delta_core::config::load_config;
use delta_core::db::Db;
use delta_core::models::{AssetClass, Bar, Instrument};
use delta_services::{analytics, asset_metrics::AssetMetrics};

use crate::braille::BrailleGraph;
use crate::screens::{
    grouped, Footer, HomeFeed, HomeState, MetricsData, SettingsData, WatchEntry, WatchlistState,
};

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
}

/// One desk: instruments, the selected one, the range, and live quotes.
pub struct Desk {
    pub instruments: Vec<InstrumentDesk>,
    pub selected: usize,
    pub range_index: usize,
    pub scrub: Option<usize>,
    /// Live quotes keyed by instrument id (`regularMarketPrice`).
    pub live: BTreeMap<String, f64>,
    /// Inspector metric rows keyed by instrument id (metrics worker).
    pub metrics: BTreeMap<String, Vec<(String, String)>>,
    pub asset_metrics: BTreeMap<(String, String), AssetMetrics>,
    pub source: Source,
    /// Last ingest counts, when a gather has run this session.
    pub last_ingest: Option<BTreeMap<String, usize>>,
    /// Live Home overview values (headline, pulse, upcoming, staleness).
    pub feed: HomeFeed,
    /// The Settings screen's data (config, providers, diagnostics).
    pub settings: SettingsData,
    /// The footer cells implied by `settings` at load time.
    pub settings_footer: Footer,
    /// Where edits write (`config.toml`); `None` in the offline environment.
    pub config_path: Option<PathBuf>,
}

impl Default for Desk {
    fn default() -> Self {
        Self::open()
    }
}

impl Desk {
    /// Load the real desk from `config.toml` + its DB; fall back to the
    /// offline seed when either is missing or empty.
    pub fn open() -> Desk {
        match Self::try_real(Path::new("config.toml")) {
            Some(desk) => desk,

            None => Self::offline(),
        }
    }

    fn try_real(config_path: &Path) -> Option<Desk> {
        if !config_path.exists() {
            return None;
        }
        let (_, app) = load_config(config_path).ok()?;
        let db_path = PathBuf::from(&app.db_path);
        let specs = delta_services::config_ops::target_specs(config_path).ok()?;
        let universe: Vec<Instrument> = specs.into_values().flat_map(|t| t.instruments()).collect();
        let db = Db::open(&db_path).ok()?;
        let mut instruments: Vec<InstrumentDesk> = Vec::new();
        for inst in universe {
            let bars = db.bars(&inst.id).ok()?;
            instruments.push(InstrumentDesk::from_db(inst, bars));
        }
        let ids: Vec<String> = instruments
            .iter()
            .map(|d| d.instrument.id.clone())
            .collect();
        let feed = load_feed(&db, &ids);
        let env_path = PathBuf::from(".env");
        let now = now_naive();
        let settings = SettingsData::load(&app, &db_path, &env_path, now);
        let settings_footer = settings.footer(now);
        Some(Desk {
            instruments,
            selected: 0,
            range_index: 2, // "1m"
            scrub: None,
            live: BTreeMap::new(),
            metrics: BTreeMap::new(),
            asset_metrics: BTreeMap::new(),
            source: Source::Real(db_path),
            last_ingest: None,
            feed,
            settings,
            settings_footer,
            config_path: Some(config_path.to_path_buf()),
        })
    }

    /// Reload the configured targets after an add/remove operation. The
    /// service owns persistence; this only rebuilds the displayed universe.
    pub fn reload_targets_from(&mut self, config_path: &Path) -> Result<(), String> {
        let refreshed = Self::try_real(config_path)
            .ok_or_else(|| "unable to load targets from config and database".to_string())?;
        let selected_id = self.current().map(|current| current.instrument.id.clone());
        self.instruments = refreshed.instruments;
        self.selected = selected_id
            .and_then(|id| {
                self.instruments
                    .iter()
                    .position(|row| row.instrument.id == id)
            })
            .unwrap_or(0);
        self.scrub = None;
        self.live
            .retain(|id, _| self.instruments.iter().any(|row| row.instrument.id == *id));
        self.metrics
            .retain(|id, _| self.instruments.iter().any(|row| row.instrument.id == *id));
        self.asset_metrics
            .retain(|(id, _), _| self.instruments.iter().any(|row| row.instrument.id == *id));
        self.source = refreshed.source;
        self.feed = refreshed.feed;
        self.settings = refreshed.settings;
        self.settings_footer = refreshed.settings_footer;
        self.config_path = refreshed.config_path;
        Ok(())
    }

    /// The offline golden environment (in-memory; nothing touches disk).
    pub fn offline() -> Desk {
        let settings = crate::screens::exporter_world();
        let settings_footer = settings.footer(now_naive());
        Desk {
            instruments: vec![InstrumentDesk::offline("AAPL")],
            selected: 0,
            range_index: 2, // "1m"
            scrub: None,
            live: BTreeMap::new(),
            metrics: BTreeMap::new(),
            asset_metrics: BTreeMap::new(),
            source: Source::Offline,
            last_ingest: None,
            feed: HomeFeed::seed(),
            settings,
            settings_footer,
            config_path: None,
        }
    }

    /// Reload the Settings data (after `r` or an edit).
    pub fn reload_settings(&mut self) {
        let Some(config_path) = self.config_path.clone() else {
            return;
        };
        let Ok((_, app)) = load_config(&config_path) else {
            return;
        };
        let db_path = PathBuf::from(&app.db_path);
        let now = now_naive();
        self.settings = SettingsData::load(&app, &db_path, Path::new(".env"), now);
        self.settings_footer = self.settings.footer(now);
    }

    /// The instrument the inspector shows.
    pub fn current(&self) -> Option<&InstrumentDesk> {
        self.instruments.get(self.selected)
    }

    pub fn cycle_instrument(&mut self, delta: isize) {
        if self.instruments.is_empty() {
            return;
        }
        let n = self.instruments.len() as isize;
        self.selected = ((self.selected as isize + delta).rem_euclid(n)) as usize;
        self.scrub = None;
    }

    /// The watchlist `RANGES` entry currently selected.
    pub fn range(&self) -> &'static str {
        crate::screens::RANGES[self.range_index]
    }

    pub fn cycle_range(&mut self, delta: isize) {
        let n = crate::screens::RANGES.len() as isize;
        self.range_index = ((self.range_index as isize + delta).rem_euclid(n)) as usize;
        self.scrub = None;
    }

    /// Move the chart cursor over the selected range. A left move from the
    /// first point dismisses it; a right move from the inactive state lands
    /// on the most recent point.
    pub fn move_scrub(&mut self, delta: isize) {
        let n = self
            .watch_state()
            .metric
            .as_ref()
            .map_or(0, |m| m.series.len());
        if n == 0 {
            self.scrub = None;
            return;
        }
        self.scrub = match (self.scrub, delta) {
            (None, d) if d < 0 => Some(0),
            (None, _) => Some(n - 1),
            (Some(0), d) if d < 0 => None,
            (Some(index), d) => Some(index.saturating_add_signed(d).min(n - 1)),
        };
    }

    /// The live quote for the current instrument, when a quote worker has
    /// reported one.
    pub fn live_price(&self) -> Option<f64> {
        self.current()
            .and_then(|current| self.live.get(&current.instrument.id).copied())
    }

    /// The inspector state for the current instrument and range.
    pub fn watch_state(&self) -> WatchlistState {
        let range = self.range();
        let entries = self
            .instruments
            .iter()
            .map(|row| WatchEntry {
                symbol: row.instrument.symbol.clone(),
                asset_class: row.instrument.asset_class.as_str().to_string(),
            })
            .collect();
        let Some(cur) = self.current() else {
            return WatchlistState {
                range,
                scrub: None,
                entries,
                selected: 0,
                metric: None,
            };
        };
        if let Some(provider) = self
            .asset_metrics
            .get(&(cur.instrument.id.clone(), range.to_string()))
        {
            let (series, times) = if provider.series.is_empty() {
                let closes: Vec<f64> = cur.bars.iter().map(|(close, _)| *close).collect();
                let stamps: Vec<String> = cur.bars.iter().map(|(_, stamp)| stamp.clone()).collect();
                crate::screens::chart_window(&closes, crate::screens::range_window(range), &stamps)
            } else {
                crate::screens::chart_window(
                    &provider.series,
                    crate::screens::range_window(range),
                    &provider.series_times,
                )
            };
            let current = series.last().copied();
            return WatchlistState {
                range,
                scrub: self.scrub,
                entries,
                selected: self.selected,
                metric: Some(MetricsData {
                    symbol: cur.instrument.symbol.clone(),
                    market: cur.instrument.market.clone(),
                    asset_class: provider.profile.clone(),
                    currency: cur.instrument.currency.clone(),
                    current: current.map(grouped).unwrap_or_else(|| "—".into()),
                    change_label: (!provider.change_label.is_empty())
                        .then(|| provider.change_label.clone()),
                    period_high: provider.period_high,
                    period_low: provider.period_low,
                    history_start: times.first().cloned(),
                    history_end: times.last().cloned(),
                    series,
                    series_times: times,
                    source: provider.source.clone(),
                    values: provider
                        .groups
                        .iter()
                        .flat_map(|(_, rows)| rows.iter().cloned())
                        .take(24)
                        .collect(),
                }),
            };
        }
        let window = crate::screens::range_window(range);
        let start = window.map_or(0, |n| cur.bars.len().saturating_sub(n));
        let picked: Vec<(f64, String)> = cur.bars[start..].to_vec();
        let mut series: Vec<f64> = picked.iter().map(|(c, _)| *c).collect();
        let times: Vec<String> = picked.iter().map(|(_, t)| t.clone()).collect();
        // A live quote appends itself to the series head (labelled below).
        let live = self.live_price();
        if let Some(px) = live {
            series.push(px);
        }
        let Some(&last) = series.last() else {
            return WatchlistState {
                range,
                scrub: None,
                entries,
                selected: self.selected,
                metric: Some(MetricsData {
                    symbol: cur.instrument.symbol.clone(),
                    market: cur.instrument.market.clone(),
                    asset_class: cur.instrument.asset_class.as_str().to_string(),
                    currency: cur.instrument.currency.clone(),
                    current: "—".to_string(),
                    change_label: None,
                    period_high: None,
                    period_low: None,
                    history_start: None,
                    history_end: None,
                    series: Vec::new(),
                    series_times: Vec::new(),
                    source: "No history yet".to_string(),
                    values: Vec::new(),
                }),
            };
        };
        let first = series[0];
        let change = if first != 0.0 {
            (last / first - 1.0) * 100.0
        } else {
            0.0
        };
        let hi = series.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = series.iter().cloned().fold(f64::INFINITY, f64::min);
        let bond = cur.instrument.asset_class == AssetClass::Bond;
        let current_label = if bond {
            "Current yield"
        } else {
            "Current price"
        };
        let mut values = vec![(current_label.to_string(), grouped(last))];
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
            scrub: self.scrub,
            entries,
            selected: self.selected,
            metric: Some(MetricsData {
                symbol: cur.instrument.symbol.clone(),
                market: cur.instrument.market.clone(),
                asset_class: cur.instrument.asset_class.as_str().to_string(),
                currency: cur.instrument.currency.clone(),
                current: grouped(last),
                change_label: Some(if bond {
                    format!("{:+.1} bps", (last - first) * 100.0)
                } else {
                    format!("{change:+.1}%")
                }),
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
        let Some(cur) = self.current() else {
            return HomeState {
                clock: Utc::now().format("%A %d %B %Y · %H:%M:%S UTC").to_string(),
                symbol: "—".to_string(),
                last: "—".to_string(),
                chg_label: "—".to_string(),
                spark: String::new(),
                since_stamp: String::new(),
                closes: Vec::new(),
                feed: self.feed.clone(),
            };
        };
        let mut closes: Vec<f64> = cur.bars.iter().map(|(c, _)| *c).collect();
        if let Some(px) = self.live_price() {
            closes.push(px);
        }
        let last = closes
            .last()
            .map(|value| grouped(*value))
            .unwrap_or_else(|| "—".into());
        let chg_label = if closes.len() > 1 && closes[closes.len() - 2] != 0.0 {
            format!(
                "{:+.2}%",
                (closes[closes.len() - 1] / closes[closes.len() - 2] - 1.0) * 100.0
            )
        } else {
            "—".into()
        };
        let spark = if closes.is_empty() {
            String::new()
        } else {
            BrailleGraph::filled(closes.clone()).rows(17, 1)[0].clone()
        };
        let now = Utc::now();
        HomeState {
            clock: now.format("%A %d %B %Y · %H:%M:%S UTC").to_string(),
            symbol: cur.instrument.symbol.clone(),
            last,
            chg_label,
            spark,
            since_stamp: format!("since {}", now.format("%a %H:%M")),
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
        self.feed = load_feed(&db, &ids);
    }
}

/// Build the Home overview feed from the analytics queries (all local SQL).
/// The seeded values stay wherever a query fails or finds nothing.
pub fn load_feed(db: &Db, instrument_ids: &[String]) -> HomeFeed {
    let now = now_naive();
    // Slots stay unset (and the painters fall back to the golden seed text)
    // unless the query succeeded and found something — a failed query must
    // not present stale seed strings as fresh data.
    let mut feed = HomeFeed::default();
    if let Ok(health) = analytics::data_health(db) {
        if let Some(newest) = health.latest_bar.values().max() {
            let age = (now - *newest).num_days().max(0);
            feed.stale_age = Some(format!("{age}d old"));
        } else {
            feed.stale_age = Some("no bars".to_string());
        }
    }
    if let Ok(pulse) = analytics::pulse(db, instrument_ids, now - Duration::days(1), 30, now) {
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
            let spend = clamp_spend(analytics::total_spend(db, Some(now - Duration::days(30))));
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

/// Clamp a spend value at zero, normalizing IEEE negative zero: `f64::max`
/// may return either zero, and `{:.2}` would render `$-0.00`.
fn clamp_spend(spend: f64) -> f64 {
    if spend == 0.0 {
        0.0
    } else {
        spend.max(0.0)
    }
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
    use delta_core::models::{Event, EventKind, LlmCall, NewsItem};

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
    fn spend_clamp_normalizes_negative_zero() {
        for v in [-0.0, -0.001, 0.0] {
            let spend = clamp_spend(v);
            assert_eq!(format!("{spend:.2}"), "0.00");
        }
        assert_eq!(clamp_spend(12.5), 12.5);
    }

    #[test]
    fn negligible_spend_renders_as_positive_zero() {
        let mut db = Db::open_memory().unwrap();
        db.store_items(&[
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
        ])
        .unwrap();
        db.llm_store_call(&LlmCall {
            id: "c1".to_string(),
            ts: now_naive(),
            task: "extract".to_string(),
            model: "m1".to_string(),
            prompt_version: "1".to_string(),
            prompt_hash: "h".to_string(),
            input_tokens: 1,
            output_tokens: 1,
            cost_usd: -0.001,
            latency_ms: 1,
            cached: false,
            response: None,
        })
        .unwrap();
        let feed = load_feed(&db, &["US:AAPL".to_string()]);
        let line = feed.activity_line.as_deref().unwrap_or_default();
        assert!(line.contains("$0.00 LLM spend"), "got: {line}");
        assert!(!line.contains('-'), "got: {line}");
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
}
