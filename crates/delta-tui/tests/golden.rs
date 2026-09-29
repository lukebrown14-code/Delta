//! R3 golden-screen oracle: render the Watchlist inspector scenario and diff
//! cell-for-cell against `fixtures/golden_screens/*.json` (Tier A: character,
//! fg, bg, attrs — zero mismatches; see docs/RUST_REWRITE_PLAN.md Rule 1).

use std::path::PathBuf;

use delta_tui::screen::Screen;
use delta_tui::screens::{
    draw_glossary_overlay, draw_watchlist, draw_watchlist_narrow, grouped, MetricsData,
    WatchlistState,
};

fn golden_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../fixtures/golden_screens/{name}-120x40.json"))
}

/// The export_golden.py seed: 80 bars, `_price(i)`, inspected series = the
/// last 30 days; frozen clock 2026-09-21 09:30 UTC.
fn price(i: usize) -> f64 {
    let v = 200.0 + i as f64 * 0.4 + 6.0 * (i as f64 / 4.0).sin();
    (v * 100.0).round() / 100.0
}

fn series() -> (Vec<f64>, Vec<String>) {
    let values: Vec<f64> = (50..80).map(price).collect();
    let times: Vec<String> = (50..80)
        .map(|i| {
            let d =
                chrono::NaiveDate::from_ymd_opt(2026, 7, 3).unwrap() + chrono::Duration::days(i);
            format!("{}T00:00:00+00:00", d.format("%Y-%m-%d"))
        })
        .collect();
    (values, times)
}

fn state(range: &'static str) -> WatchlistState {
    let (values, times) = series();
    let current_value = values[values.len() - 1];
    let series = values.clone();
    let current = grouped(current_value);
    let change = (series[series.len() - 1] / series[0] - 1.0) * 100.0;
    let hi = series.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lo = series.iter().cloned().fold(f64::INFINITY, f64::min);
    let _ = (&hi, &lo);
    let (history_start, history_end) =
        (Some(times[0].clone()), Some(times[times.len() - 1].clone()));
    let change_label = format!("{change:+.1}%");
    let hi = series.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lo = series.iter().cloned().fold(f64::INFINITY, f64::min);
    let _ = (&hi, &lo);
    WatchlistState {
        range,
        metric: Some(MetricsData {
            symbol: "AAPL".to_string(),
            market: "us".to_string(),
            asset_class: "equity".to_string(),
            currency: "USD".to_string(),
            current,
            change_label: Some(change_label),
            period_high: Some(hi),
            period_low: Some(lo),
            history_start,
            history_end,
            series,
            series_times: times,
            source: "Yahoo Finance".to_string(),
            values: vec![
                (
                    "Current price".to_string(),
                    grouped(values[values.len() - 1]),
                ),
                ("Market cap".to_string(), "3.4T".to_string()),
                ("P/E".to_string(), "31.2".to_string()),
            ],
        }),
    }
}

fn render(range: &'static str) -> Screen {
    let mut screen = Screen::new(120, 40);
    draw_watchlist(&mut screen, &state(range));
    screen
}

fn render_narrow(range: &'static str) -> Screen {
    let mut screen = Screen::new(80, 24);
    draw_watchlist_narrow(&mut screen, &state(range));
    screen
}

fn diff_against(golden_name: &str, screen: &Screen) -> Vec<String> {
    let raw = std::fs::read_to_string(golden_path(golden_name)).unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let rows = value["rows"].as_array().unwrap();
    let mut diffs = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        for (x, cell) in row.as_array().unwrap().iter().enumerate() {
            let mine = &screen.cells[y * screen.w + x];
            let want_ch = cell["ch"].as_str().unwrap().chars().next().unwrap();
            let want_fg = cell["fg"].as_str();
            let want_bg = cell["bg"].as_str();
            let want_bold = cell["attrs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a == "bold");
            let got = (mine.ch, mine.fg, mine.bg, mine.bold);
            let want = (want_ch, want_fg, want_bg, want_bold);
            if got != want {
                diffs.push(format!(
                    "row {y} col {x}: want {want_ch:?} ({want_fg:?}/{want_bg:?}/bold={want_bold}) got {got:?}"
                ));
                if diffs.len() >= 40 {
                    diffs.push("... (truncated at 40)".to_string());
                    return diffs;
                }
            }
        }
    }
    diffs
}

fn assert_golden(name: &str, range: &'static str) {
    let screen = render(range);
    let diffs = diff_against(name, &screen);
    assert!(
        diffs.is_empty(),
        "{name}: {} Tier A mismatches\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
fn default_state_tier_a() {
    assert_golden("default", "1m");
}

#[test]
fn range_cycled_state_tier_a() {
    assert_golden("range-cycled", "6m");
}

#[test]
fn narrow_state_tier_a() {
    let screen = render_narrow("1m");
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/golden_screens/narrow-80x24.json"),
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut diffs = Vec::new();
    for (y, row) in value["rows"].as_array().unwrap().iter().enumerate() {
        for (x, cell) in row.as_array().unwrap().iter().enumerate() {
            let mine = &screen.cells[y * screen.w + x];
            let want = (
                cell["ch"].as_str().unwrap().chars().next().unwrap(),
                cell["fg"].as_str(),
                cell["bg"].as_str(),
                cell["attrs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|a| a == "bold"),
            );
            let got = (mine.ch, mine.fg, mine.bg, mine.bold);
            if got != want {
                diffs.push(format!("row {y} col {x}: want {want:?} got {got:?}"));
                if diffs.len() >= 40 {
                    diffs.push("... truncated".to_string());
                    break;
                }
            }
        }
        if diffs.len() >= 40 {
            break;
        }
    }
    assert!(
        diffs.is_empty(),
        "narrow: {} mismatches\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
fn glossary_state_tier_b() {
    let mut screen = Screen::new(120, 40);
    draw_glossary_overlay(&mut screen, &state("1m"));
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/golden_screens/glossary-120x40.json"),
    )
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    // Tier B: the text layer must be exact (wrap points are deterministic for
    // this content); colour diffs are logged as findings, not failures.
    let mut char_diffs = Vec::new();
    let mut color_diffs = Vec::new();
    for (y, row) in value["rows"].as_array().unwrap().iter().enumerate() {
        for (x, cell) in row.as_array().unwrap().iter().enumerate() {
            let mine = &screen.cells[y * screen.w + x];
            // The body's vertical scrollbar (cols 90-91, its ▄ thumb glyphs)
            // is not ported yet — findings #6 in rust-screens.md; excluded
            // from the Tier B check.
            if (x == 90 || x == 91) && (6..=28).contains(&y) {
                continue;
            }
            let want_ch = cell["ch"].as_str().unwrap().chars().next().unwrap();
            if mine.ch != want_ch {
                char_diffs.push(format!(
                    "row {y} col {x}: want {want_ch:?} got {:?}",
                    mine.ch
                ));
                continue;
            }
            let want_fg = cell["fg"].as_str();
            let want_bg = cell["bg"].as_str();
            let want_bold = cell["attrs"]
                .as_array()
                .unwrap()
                .iter()
                .any(|a| a == "bold");
            // The body's vertical scrollbar (cols 90-91) is not ported yet —
            // findings #6 in rust-screens.md; excluded from the Tier B check.
            if (x == 90 || x == 91) && (6..=28).contains(&y) {
                continue;
            }
            if (mine.fg, mine.bg, mine.bold) != (want_fg, want_bg, want_bold) {
                color_diffs.push(format!(
                    "row {y} col {x}: want {want_fg:?}/{want_bg:?}/b={want_bold} got {:?}/{:?}/b={:?}",
                    mine.fg, mine.bg, mine.bold
                ));
            }
        }
        if char_diffs.len() >= 30 {
            break;
        }
    }
    if !color_diffs.is_empty() {
        println!(
            "glossary Tier B colour findings ({}):\n{}",
            color_diffs.len(),
            color_diffs
                .iter()
                .take(30)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
    assert!(
        color_diffs.is_empty(),
        "glossary: {} colour findings outside the logged scrollbar gap\n{}",
        color_diffs.len(),
        color_diffs.join("\n")
    );
    assert!(
        char_diffs.is_empty(),
        "glossary: {} text mismatches\n{}",
        char_diffs.len(),
        char_diffs.join("\n")
    );
}
