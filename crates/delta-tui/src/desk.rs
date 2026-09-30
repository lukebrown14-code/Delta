//! The desk: the screen states the app paints, loaded from the real
//! `config.toml` + `data/delta.db` (via `delta-services` / `delta-core`),
//! falling back to the offline golden seed when either is absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use chrono::{Duration, NaiveDateTime, Utc};
use delta_core::config::load_config;
use delta_core::db::Db;
use delta_core::models::{AssetClass, Bar, Instrument};

use crate::braille::BrailleGraph;
use crate::screens::{grouped, HomeState, MetricsData, WatchlistState};

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
    /// Live quotes keyed by instrument id (`regularMarketPrice`).
    pub live: BTreeMap<String, f64>,
    pub source: Source,
    /// Last ingest counts, when a gather has run this session.
    pub last_ingest: Option<BTreeMap<String, usize>>,
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
        match Self::try_real() {
            Some((instruments, db_path)) => Desk {
                instruments,
                selected: 0,
                range_index: 2, // "1m"
                live: BTreeMap::new(),
                source: Source::Real(db_path),
                last_ingest: None,
            },
            None => Self::offline(),
        }
    }

    fn try_real() -> Option<(Vec<InstrumentDesk>, PathBuf)> {
        let config_path = PathBuf::from("config.toml");
        if !config_path.exists() {
            return None;
        }
        let (_, app) = load_config(&config_path).ok()?;
        let db_path = PathBuf::from(&app.db_path);
        let specs = delta_services::config_ops::target_specs(&config_path).ok()?;
        let universe: Vec<Instrument> = specs.into_values().flat_map(|t| t.instruments()).collect();
        if universe.is_empty() {
            return None;
        }
        let db = Db::open(&db_path).ok()?;
        let mut instruments: Vec<InstrumentDesk> = Vec::new();
        for inst in universe {
            let bars = db.bars(&inst.id).ok()?;
            if !bars.is_empty() {
                instruments.push(InstrumentDesk::from_db(inst, bars));
            }
        }
        if instruments.is_empty() {
            return None;
        }
        Some((instruments, db_path))
    }

    /// The offline golden environment (in-memory; nothing touches disk).
    pub fn offline() -> Desk {
        Desk {
            instruments: vec![InstrumentDesk::offline("AAPL")],
            selected: 0,
            range_index: 2, // "1m"
            live: BTreeMap::new(),
            source: Source::Offline,
            last_ingest: None,
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
        self.live.get(&self.current().instrument.id).copied()
    }

    /// The inspector state for the current instrument and range.
    pub fn watch_state(&self) -> WatchlistState {
        let cur = self.current();
        let range = self.range();
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
        let last = series[series.len() - 1];
        let first = series[0];
        let change = if first != 0.0 {
            (last / first - 1.0) * 100.0
        } else {
            0.0
        };
        let hi = series.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let lo = series.iter().cloned().fold(f64::INFINITY, f64::min);
        let mut values = vec![("Current price".to_string(), grouped(last))];
        if live.is_some() {
            values.push(("Source".to_string(), "live quote".to_string()));
        }
        WatchlistState {
            range,
            metric: Some(MetricsData {
                symbol: cur.instrument.symbol.clone(),
                market: cur.instrument.market.clone(),
                asset_class: "equity".to_string(),
                currency: cur.instrument.currency.clone(),
                current: grouped(last),
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
        let last = closes[closes.len() - 1];
        let prev = closes[closes.len() - 2];
        let chg_label = format!("{:+.2}%", (last / prev - 1.0) * 100.0);
        let spark = BrailleGraph::filled(closes.clone()).rows(17, 1)[0].clone();
        let now = Utc::now();
        HomeState {
            clock: now.format("%A %d %B %Y · %H:%M:%S UTC").to_string(),
            symbol: cur.instrument.symbol.clone(),
            last: grouped(last),
            chg_label,
            spark,
            since_stamp: format!("since {}", now.format("%a %H:%M")),
            closes,
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
    }
}

/// Wall-clock now as the naive-UTC the pipeline speaks.
pub fn now_naive() -> NaiveDateTime {
    Utc::now().naive_utc()
}
