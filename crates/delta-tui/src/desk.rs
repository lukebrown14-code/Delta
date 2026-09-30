//! The offline desk: builds the screen states from a seeded in-memory DB so
//! the binary runs without the network or Python (the golden-exporter
//! environment; see `delta-services::fixture` for the shared seed).
//!
//! Real config/DB loading through `delta-services` replaces this once the
//! services stream is wired into the binary.

use chrono::{Duration, NaiveDateTime, Utc};
use delta_core::db::Db;
use delta_core::models::Bar;

use crate::braille::BrailleGraph;
use crate::screens::{grouped, HomeState, MetricsData, WatchlistState};

/// `RANGES` index the desk starts on.
const DEFAULT_RANGE: usize = 2; // "1m"

/// Golden-exporter constants: 80 seeded bars.
pub const BAR_COUNT: usize = 80;

pub fn price(i: usize) -> f64 {
    let v = 200.0 + i as f64 * 0.4 + 6.0 * (i as f64 / 4.0).sin();
    (v * 100.0).round() / 100.0
}

/// One desk: the bar history plus the current watchlist range.
pub struct Desk {
    bars: Vec<(f64, String)>,
    pub range_index: usize,
}

impl Default for Desk {
    fn default() -> Self {
        Self::offline()
    }
}

impl Desk {
    /// Seed `BAR_COUNT` daily closes ending yesterday, offline.
    pub fn offline() -> Desk {
        let mut db = Db::open_memory().expect("open offline db");
        let last_day = (Utc::now().naive_utc() - Duration::days(1)).date();
        let start = last_day - Duration::days(BAR_COUNT as i64 - 1);
        let bars: Vec<Bar> = (0..BAR_COUNT)
            .map(|i| {
                let day = start + Duration::days(i as i64);
                let close = price(i);
                Bar {
                    instrument_id: "US:AAPL".to_string(),
                    ts: NaiveDateTime::new(day, chrono::NaiveTime::from_hms_opt(0, 0, 0).unwrap()),
                    open: ((close - 1.0) * 100.0).round() / 100.0,
                    high: ((close + 1.0) * 100.0).round() / 100.0,
                    low: ((close - 1.0) * 100.0).round() / 100.0,
                    close,
                    volume: 1000.0,
                    source: "yahoo".to_string(),
                }
            })
            .collect();
        // Keep the painter's series in close+time form.
        let _ = &mut db;
        let series = bars
            .into_iter()
            .map(|b| {
                (
                    b.close,
                    format!("{}T00:00:00+00:00", b.ts.format("%Y-%m-%d")),
                )
            })
            .collect();
        Desk {
            bars: series,
            range_index: DEFAULT_RANGE,
        }
    }

    /// The watchlist `RANGES` entry currently selected.
    pub fn range(&self) -> &'static str {
        crate::screens::RANGES[self.range_index]
    }

    pub fn cycle_range(&mut self, delta: isize) {
        let n = crate::screens::RANGES.len() as isize;
        self.range_index = ((self.range_index as isize + delta).rem_euclid(n)) as usize;
    }

    /// The inspector state for the current range (series = the range window).
    pub fn watch_state(&self) -> WatchlistState {
        let range = self.range();
        let window = crate::screens::range_window(range);
        let start = window.map_or(0, |n| self.bars.len().saturating_sub(n));
        let picked: Vec<(f64, String)> = self.bars[start..].to_vec();
        let series: Vec<f64> = picked.iter().map(|(c, _)| *c).collect();
        let times: Vec<String> = picked.iter().map(|(_, t)| t.clone()).collect();
        let last = *series.last().unwrap_or(&0.0);
        let first = *series.first().unwrap_or(&0.0);
        let change = if first != 0.0 {
            (last / first - 1.0) * 100.0
        } else {
            0.0
        };
        let hi = series.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = series.iter().cloned().fold(f64::INFINITY, f64::min);
        WatchlistState {
            range,
            metric: Some(MetricsData {
                symbol: "AAPL".to_string(),
                market: "us".to_string(),
                asset_class: "equity".to_string(),
                currency: "USD".to_string(),
                current: grouped(last),
                change_label: Some(format!("{change:+.1}%")),
                period_high: Some(hi),
                period_low: Some(lo),
                history_start: times.first().cloned(),
                history_end: times.last().cloned(),
                series,
                series_times: times,
                source: "Yahoo Finance".to_string(),
                values: vec![("Current price".to_string(), grouped(last))],
            }),
        }
    }

    /// The home screen state (clock, headline quote, sparkline).
    pub fn home_state(&self) -> HomeState {
        let closes: Vec<f64> = self.bars.iter().map(|(c, _)| *c).collect();
        let last = closes[closes.len() - 1];
        let prev = closes[closes.len() - 2];
        let chg_label = format!("{:+.2}%", (last / prev - 1.0) * 100.0);
        let spark = BrailleGraph::filled(closes.clone()).rows(17, 1)[0].clone();
        let now = Utc::now();
        HomeState {
            clock: now.format("%A %d %B %Y · %H:%M:%S UTC").to_string(),
            symbol: "AAPL".to_string(),
            last: grouped(last),
            chg_label,
            spark,
            since_stamp: format!("since {}", now.format("%a %H:%M")),
            closes,
        }
    }
}
