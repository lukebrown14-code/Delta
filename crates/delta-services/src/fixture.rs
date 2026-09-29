//! Offline fixture feed: one seed that reproduces the golden-exporter
//! environment (80 `_price` bars, the one-target config, the state file) so
//! Rust screens and tests run without Python or the network.

use std::path::Path;

use chrono::NaiveDateTime;
use delta_core::db::{Db, StoreItem};
use delta_core::models::{AssetClass, Bar, Instrument};

/// `_price(i)` from `tests/export_golden.py` — the shared bar seed.
pub fn price(i: usize) -> f64 {
    let v = 200.0 + i as f64 * 0.4 + 6.0 * (i as f64 / 4.0).sin();
    (v * 100.0).round() / 100.0
}

/// The AAPL instrument the golden scenarios watch.
pub fn aapl() -> Instrument {
    Instrument {
        id: "US:AAPL".to_string(),
        market: "us".to_string(),
        symbol: "AAPL".to_string(),
        name: None,
        currency: "USD".to_string(),
        sector: None,
        asset_class: AssetClass::Equity,
        watchlists: Vec::new(),
        tags: Default::default(),
        industry: None,
        meta: Default::default(),
    }
}

/// The universe as the exporter builds it: every watched target's instruments.
pub fn golden_universe() -> Vec<Instrument> {
    vec![aapl()]
}

/// Seed `BAR_COUNT` daily closes ending `last_day` (`seed_bars` parity).
pub fn seed_bars(db: &mut Db, instrument_id: &str, count: usize, last_day: NaiveDateTime) -> usize {
    let start = last_day.date() - chrono::Duration::days(count as i64 - 1);
    let items: Vec<StoreItem> = (0..count)
        .map(|i| {
            let day = start + chrono::Duration::days(i as i64);
            let close = price(i);
            StoreItem::Bar(Bar {
                instrument_id: instrument_id.to_string(),
                ts: day.and_hms_opt(21, 0, 0).unwrap_or_default(),
                open: ((close - 1.0) * 100.0).round() / 100.0,
                high: ((close + 1.0) * 100.0).round() / 100.0,
                low: ((close - 1.0) * 100.0).round() / 100.0,
                close,
                volume: 1000.0,
                source: "yahoo".to_string(),
            })
        })
        .collect();
    let n = items.len();
    let counts = db.store_items(&items).expect("seed bars");
    counts.get("bar").copied().unwrap_or(0).min(n)
}

/// The `[targets]` table the golden scenarios use.
pub const GOLDEN_CONFIG: &str =
    "[targets.apple]\nkind = \"company\"\nmarket = \"us\"\ntickers = [\"AAPL\"]\n";

/// Write the golden config + an empty state file next to `db_path`, so the
/// offline environment matches the exporter's tmp dir.
pub fn seed_config(dir: &Path, config: &str) -> std::path::PathBuf {
    std::fs::create_dir_all(dir).ok();
    let path = dir.join("config.toml");
    std::fs::write(&path, config).expect("write config");
    path
}

/// One call that stands up the offline desk: seeded DB + config file.
/// Returns the config path.
pub fn seed_offline_desk(
    db: &mut Db,
    dir: &Path,
    bar_count: usize,
    last_day: NaiveDateTime,
) -> std::path::PathBuf {
    for inst in golden_universe() {
        seed_bars(db, &inst.id, bar_count, last_day);
    }
    seed_config(dir, GOLDEN_CONFIG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seed_stores_the_full_bar_history() {
        let mut db = Db::open_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let last =
            NaiveDateTime::parse_from_str("2026-09-20 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let config_path = seed_offline_desk(&mut db, dir.path(), 80, last);
        assert_eq!(db.table_count("bar").unwrap(), 80);
        assert!(config_path.exists());
        let bars = db.bars("US:AAPL").unwrap();
        assert_eq!(bars.len(), 80);
        assert!((bars[79].close - price(79)).abs() < 1e-9);
        assert_eq!(bars[79].close, 236.30);
    }
}
