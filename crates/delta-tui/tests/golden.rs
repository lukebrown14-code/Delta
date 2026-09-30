//! R3 golden-screen oracle: render the Watchlist inspector scenario and diff
//! cell-for-cell against `fixtures/golden_screens/*.json` (Tier A: character,
//! fg, bg, attrs — zero mismatches; see docs/RUST_REWRITE_PLAN.md Rule 1).

use std::path::PathBuf;

use delta_tui::screen::Screen;
use delta_tui::screens::{
    draw_ask, draw_ask_narrow, draw_ask_wide, draw_decisions, draw_decisions_narrow,
    draw_decisions_wide, draw_glossary_overlay, draw_home, draw_home_narrow, draw_home_wide,
    draw_research, draw_research_narrow, draw_research_wide, draw_settings, draw_settings_narrow,
    draw_settings_wide, draw_theses, draw_theses_narrow, draw_theses_wide, draw_watchlist,
    draw_watchlist_narrow, draw_watchlist_wide, grouped, HomeState, MetricsData, WatchlistState,
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

/// Render `draw` at each size and require zero Tier A mismatches against the
/// exporter's golden for that size (the full R4 3-size matrix rule).
fn assert_sized_tier_a(name: &str, sizes: &[(usize, usize)], draw: impl Fn(&mut Screen)) {
    for &(w, h) in sizes {
        let mut screen = Screen::new(w, h);
        draw(&mut screen);
        let diffs = golden_diff_sized(&screen, &format!("{name}-{w}x{h}"));
        for d in &diffs {
            println!("{d}");
        }
        assert!(
            diffs.is_empty(),
            "{name} at {w}x{h}: {} Tier A mismatches\n{}",
            diffs.len(),
            diffs.join("\n")
        );
    }
}

/// Tier B rule for the glossary overlay: the text layer must be exact at
/// every size; colour differences are logged as findings, not failures.
fn assert_glossary_sized_tier_b(w: usize, h: usize) {
    let mut screen = Screen::new(w, h);
    draw_glossary_overlay(&mut screen, &state("1m"));
    let raw = std::fs::read_to_string(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(
        format!("../../fixtures/golden_screens/glossary-{w}x{h}.json"),
    ))
    .unwrap();
    let value: serde_json::Value = serde_json::from_str(&raw).unwrap();
    let mut char_diffs = Vec::new();
    let mut color_diffs = Vec::new();
    for (y, row) in value["rows"].as_array().unwrap().iter().enumerate() {
        for (x, cell) in row.as_array().unwrap().iter().enumerate() {
            let mine = &screen.cells[y * screen.w + x];
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
            if (mine.fg, mine.bg, mine.bold) != (want_fg, want_bg, want_bold) {
                color_diffs.push(format!(
                    "row {y} col {x}: want {want_fg:?}/{want_bg:?}/b={want_bold} got {:?}/{:?}/b={:?}",
                    mine.fg, mine.bg, mine.bold
                ));
            }
        }
    }
    if !color_diffs.is_empty() {
        println!(
            "glossary at {w}x{h}: {} colour findings (Tier B, logged):\n{}",
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
        char_diffs.is_empty(),
        "glossary at {w}x{h}: {} text mismatches\n{}",
        char_diffs.len(),
        char_diffs.join("\n")
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
fn glossary_state_tier_a() {
    // The VerticalScroll scrollbar (cols 90-91) is ported, so the glossary
    // now gates at Tier A like every other screen.
    let mut screen = Screen::new(120, 40);
    draw_glossary_overlay(&mut screen, &state("1m"));
    let diffs = golden_diff_sized(&screen, "glossary-120x40");
    for d in &diffs {
        println!("{d}");
    }
    assert!(diffs.is_empty(), "glossary: {} mismatches", diffs.len());
}

fn home_state() -> HomeState {
    let (_series, _times) = series();
    let closes: Vec<f64> = (40..80)
        .map(|i| {
            let v = 200.0 + i as f64 * 0.4 + 6.0 * (i as f64 / 4.0).sin();
            (v * 100.0).round() / 100.0
        })
        .collect();
    let spark = delta_tui::braille::BrailleGraph::filled(closes.clone()).rows(17, 1)[0].clone();
    let last = closes[closes.len() - 1];
    let prev = closes[closes.len() - 2];
    let chg_label = format!("{:+.2}%", (last / prev - 1.0) * 100.0);
    HomeState {
        clock: "Monday 21 September 2026 · 09:30:00 UTC".to_string(),
        symbol: "AAPL".to_string(),
        last: grouped(last),
        chg_label,
        spark,
        since_stamp: "since Mon 09:30".to_string(),
        closes,
    }
}

#[test]
fn home_state_tier_a() {
    let mut screen = Screen::new(120, 40);
    draw_home(&mut screen, &home_state());
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/golden_screens/home-120x40.json"),
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
        "home: {} Tier A mismatches\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

#[test]
fn research_state_tier_a() {
    let mut screen = Screen::new(120, 40);
    draw_research(&mut screen);
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/golden_screens/research-120x40.json"),
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
                if diffs.len() >= 30 {
                    break;
                }
            }
        }
        if diffs.len() >= 30 {
            break;
        }
    }
    assert!(
        diffs.is_empty(),
        "research: {} mismatches\n{}",
        diffs.len(),
        diffs.join("\n")
    );
}

fn golden_diff_sized(screen: &Screen, file: &str) -> Vec<String> {
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../fixtures/golden_screens/{file}.json")),
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
                    break;
                }
            }
        }
        if diffs.len() >= 40 {
            break;
        }
    }
    diffs
}

fn golden_diff(screen: &Screen, name: &str) -> Vec<String> {
    let raw = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../fixtures/golden_screens/{name}-120x40.json")),
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
                if diffs.len() >= 30 {
                    break;
                }
            }
        }
        if diffs.len() >= 30 {
            break;
        }
    }
    diffs
}

#[test]
fn theses_state_tier_a() {
    let mut screen = Screen::new(120, 40);
    draw_theses(&mut screen);
    let diffs = golden_diff(&screen, "theses");
    for d in &diffs {
        println!("{d}");
    }
    assert!(diffs.is_empty(), "theses: {} mismatches", diffs.len());
}

#[test]
fn ask_state_tier_a() {
    let mut screen = Screen::new(120, 40);
    draw_ask(&mut screen);
    let diffs = golden_diff(&screen, "ask");
    for d in &diffs {
        println!("{d}");
    }
    assert!(diffs.is_empty(), "ask: {} mismatches", diffs.len());
}

#[test]
fn decisions_state_tier_a() {
    let mut screen = Screen::new(120, 40);
    draw_decisions(&mut screen);
    let diffs = golden_diff(&screen, "decisions");
    for d in &diffs {
        println!("{d}");
    }
    assert!(diffs.is_empty(), "decisions: {} mismatches", diffs.len());
}

#[test]
fn settings_state_tier_a() {
    let mut screen = Screen::new(120, 40);
    draw_settings(&mut screen);
    let diffs = golden_diff(&screen, "settings");
    for d in &diffs {
        println!("{d}");
    }
    assert!(diffs.is_empty(), "settings: {} mismatches", diffs.len());
}

#[test]
fn default_200x50_matrix() {
    let mut screen = Screen::new(200, 50);
    draw_watchlist_wide(&mut screen, &state("1m"));
    let diffs = golden_diff_sized(&screen, "default-200x50");
    for d in &diffs {
        println!("{d}");
    }
    assert!(diffs.is_empty(), "{} mismatches", diffs.len());
}

#[test]
fn range_cycled_200x50_matrix() {
    let mut screen = Screen::new(200, 50);
    draw_watchlist_wide(&mut screen, &state("6m"));
    let diffs = golden_diff_sized(&screen, "range-cycled-200x50");
    for d in &diffs {
        println!("{d}");
    }
    assert!(diffs.is_empty(), "{} mismatches", diffs.len());
}

// ---- R4: the full 3-size golden matrix (80x24, 120x40, 200x50) ----

/// At 80x24 the watchlist inspector is the narrow breakpoint layout.
#[test]
fn watchlist_80x24_matrix() {
    for (name, range) in [("default", "1m"), ("range-cycled", "6m")] {
        let mut screen = Screen::new(80, 24);
        draw_watchlist_narrow(&mut screen, &state(range));
        let diffs = golden_diff_sized(&screen, &format!("{name}-80x24"));
        for d in &diffs {
            println!("{d}");
        }
        assert!(
            diffs.is_empty(),
            "{name} at 80x24: {} mismatches\n{}",
            diffs.len(),
            diffs.join("\n")
        );
    }
}

#[test]
fn home_matrix_tier_a() {
    assert_sized_tier_a("home", &[(80, 24)], |s| draw_home_narrow(s, &home_state()));
    assert_sized_tier_a("home", &[(200, 50)], |s| draw_home_wide(s, &home_state()));
}

#[test]
fn research_matrix_tier_a() {
    assert_sized_tier_a("research", &[(80, 24)], draw_research_narrow);
    assert_sized_tier_a("research", &[(200, 50)], draw_research_wide);
}

#[test]
fn theses_matrix_tier_a() {
    assert_sized_tier_a("theses", &[(80, 24)], draw_theses_narrow);
    assert_sized_tier_a("theses", &[(200, 50)], draw_theses_wide);
}

#[test]
fn ask_matrix_tier_a() {
    assert_sized_tier_a("ask", &[(80, 24)], draw_ask_narrow);
    assert_sized_tier_a("ask", &[(200, 50)], draw_ask_wide);
}

#[test]
fn decisions_matrix_tier_a() {
    assert_sized_tier_a("decisions", &[(80, 24)], draw_decisions_narrow);
    assert_sized_tier_a("decisions", &[(200, 50)], draw_decisions_wide);
}

#[test]
fn settings_matrix_tier_a() {
    assert_sized_tier_a("settings", &[(80, 24)], draw_settings_narrow);
    assert_sized_tier_a("settings", &[(200, 50)], draw_settings_wide);
}

#[test]
fn glossary_matrix_tier_b() {
    // 120x40 is gated Tier A by glossary_state_tier_a; the other two sizes
    // are Tier B: text exact, colour findings logged for triage.
    assert_glossary_sized_tier_b(80, 24);
    assert_glossary_sized_tier_b(200, 50);
}
// (dbg helper removed)
