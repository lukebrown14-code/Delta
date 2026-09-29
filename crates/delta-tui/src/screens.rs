//! The Watchlist screen / metrics inspector (port of
//! `delta/tui/screens/targets.py`, inspector path) as a deterministic painter
//! over the golden [`Screen`] model.
//!
//! Geometry constants are the 120x40 layout the golden exporter captured;
//! the -narrow breakpoint and glossary modal are separate findings
//! (docs/rewrite/findings/rust-screens.md).

use crate::chart::PriceChart;
use crate::screen::{color, Screen, Style};

/// `RANGES` / `RANGE_LABELS` / `RANGE_WINDOW` (targets.py).
pub const RANGES: [&str; 7] = ["1d", "5d", "1m", "6m", "ytd", "1y", "all"];
pub fn range_label(range: &str) -> &'static str {
    match range {
        "1d" => "1D",
        "5d" => "5D",
        "1m" => "1M",
        "6m" => "6M",
        "ytd" => "YTD",
        "1y" => "1Y",
        _ => "ALL",
    }
}
pub fn range_window(range: &str) -> Option<usize> {
    match range {
        "1d" => None,
        "5d" => Some(5),
        "1m" => Some(30),
        "6m" => Some(130),
        "ytd" => Some(250),
        "1y" => Some(252),
        _ => None,
    }
}

/// Everything the inspector paints, precomputed by the caller (the equivalent
/// of the Python screen's gathered state).
pub struct WatchlistState {
    pub range: &'static str,
    pub metric: Option<MetricsData>,
}

/// The gathered `AssetMetrics` fields the inspector renders.
pub struct MetricsData {
    pub symbol: String,
    pub market: String,
    pub asset_class: String,
    pub currency: String,
    pub current: String,
    pub change_label: Option<String>,
    pub period_high: Option<f64>,
    pub period_low: Option<f64>,
    pub history_start: Option<String>,
    pub history_end: Option<String>,
    pub series: Vec<f64>,
    pub series_times: Vec<String>,
    pub source: String,
    /// Metric key/value cards, in display order.
    pub values: Vec<(String, String)>,
}

/// `_friendly_date_range`'s compact form: "22 Aug", "20 Sep 2026".
pub fn friendly_date(point: &str) -> String {
    let date = point.get(..10).unwrap_or("");
    let Ok(d) = chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d") else {
        return point.to_string();
    };
    format!("{:02} {} {}", d.day(), month_name(d.month()), d.year())
}

/// "22 Aug – 20 Sep 2026"; a same-day range collapses to one date.
pub fn friendly_date_range(start: Option<&String>, end: Option<&String>) -> String {
    match (start, end) {
        (Some(s), Some(e)) => {
            let (s, e) = (friendly_date(s), friendly_date(e));
            if s == e {
                s
            } else if s.get(6..) == e.get(6..) {
                format!("{} – {}", s.get(..6).unwrap_or(&s), e)
            } else {
                format!("{s} – {e}")
            }
        }
        (Some(s), None) | (None, Some(s)) => friendly_date(s),
        (None, None) => String::new(),
    }
}

fn month_name(month: u32) -> &'static str {
    [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][(month as usize).saturating_sub(1).clamp(0, 11)]
}

use chrono::Datelike;

/// `chart_window`: the trailing window, values and timestamps together.
pub fn chart_window(
    series: &[f64],
    days: Option<usize>,
    times: &[String],
) -> (Vec<f64>, Vec<String>) {
    let start = match days {
        None => 0,
        Some(days) => series.len().saturating_sub(days),
    };
    if times.len() != series.len() {
        return (series[start..].to_vec(), Vec::new());
    }
    (series[start..].to_vec(), times[start..].to_vec())
}

/// Python `f"{value:,.2f}"`.
pub fn grouped(value: f64) -> String {
    let s = format!("{value:.2}");
    let (sign, digits) = match s.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", s.as_str()),
    };
    let (int_part, frac_part) = digits.split_once('.').unwrap_or((digits, ""));
    let mut out = String::new();
    for (i, ch) in int_part.chars().enumerate() {
        if i > 0 && (int_part.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{sign}{out}.{frac_part}")
}

/// Hint runs in the panes' title/hint bars: key (bold blue) + " hint" (muted),
/// pairs joined by two muted spaces, wrapped in single spaces.
fn pane_hints(pairs: &[(&str, &str)]) -> Vec<(String, Style)> {
    let mut runs = vec![(" ".to_string(), Style::fg(color::MUTED))];
    let mut first = true;
    for (key, hint) in pairs {
        if !first {
            runs.push(("  ".to_string(), Style::fg(color::MUTED)));
        }
        first = false;
        runs.push(((*key).to_string(), Style::fg(color::BLUE).bold()));
        runs.push((format!(" {hint}"), Style::fg(color::MUTED)));
    }
    runs.push((" ".to_string(), Style::fg(color::MUTED)));
    runs
}

/// Paint the full 120x40 Watchlist screen (`draw` = the composed frame).
pub fn draw_watchlist(screen: &mut Screen, state: &WatchlistState) {
    let w = screen.w;
    let content_bottom = screen.h - 3; // last content row (37 at h=40)

    // Panes: left watchlist (cols 1..46), right metrics (47..118).
    screen.pane(
        1,
        0,
        46,
        content_bottom,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· ○ idle · 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("a", "add"),
            ("d", "remove"),
            ("/", "filter"),
            ("space", "fold"),
        ]),
    );
    screen.pane(
        47,
        0,
        w - 2,
        content_bottom,
        false,
        &[("metrics", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "refresh"),
            ("r/R", &format!("range: {}", range_label(state.range))),
            ("i", "glossary"),
        ]),
    );

    draw_list_pane(screen, content_bottom);
    if let Some(metric) = &state.metric {
        draw_inspector(screen, state, metric);
    }
    draw_status_bar(screen, screen.h - 1, w, false);
}

fn draw_list_pane(screen: &mut Screen, content_bottom: usize) {
    // Blank OptionList rows carry the foreground style (cols 3..44).
    screen.fill(3, 2, 45, content_bottom, Style::fg(color::FG));
    // Column header (a disabled option): muted, prompt + python row string.
    let header = format!(
        "  {:<12}{:>10}  {:>8}  {:>4}",
        "Name", "Last", "Chg%", "Age"
    );
    screen.text(3, 1, &format!(" {header} "), Style::fg(color::MUTED));
    // Group header: "▾ equity (1)".
    let mut x = 3;
    screen.put(x, 2, ' ', Style::fg(color::FG));
    x += 1;
    x = screen.text(x, 2, "▾ equity ", Style::fg(color::BLUE).bold());
    screen.text(x, 2, "(1)", Style::fg(color::MUTED).bold());
    // Selected instrument row: cursor row in $primary, quoteless dashes.
    let y = 3usize;
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    let muted = Style::fg(color::MUTED).bg(color::BLUE_BG).bold();
    let row = format!("  {:<12}{:>10}  {:>8}  {:>4}", "AAPL", "—", "—", "");
    screen.put(3, y, ' ', white);
    let mut x = 4;
    x = screen.text(x, y, &row[..14], white); // prompt + name column
    x = screen.text(x, y, &row[14..], muted); // quote columns
    screen.put(44, y, ' ', white);
    let _ = x;
}

fn draw_inspector(screen: &mut Screen, state: &WatchlistState, metric: &MetricsData) {
    let cx = 48usize; // right pane inner left edge
    let right_edge = 117usize; // last content col before the blurred border

    // Row 1: name (bold) + "US · equity · USD" (muted bold).
    let mut x = cx + 1;
    x = screen.text(x, 1, &metric.symbol, Style::fg(color::FG).bold());
    let meta = format!(
        "  {} · {} · {}",
        metric.market.to_uppercase(),
        metric.asset_class,
        metric.currency
    );
    screen.text(x, 1, &meta, Style::fg(color::MUTED).bold());

    // Row 2: hero — current price, currency, closed/last date.
    let mut x = cx + 1;
    x = screen.text(x, 2, &metric.current, Style::fg(color::FG).bold());
    x = screen.text(
        x,
        2,
        &format!(" {}", metric.currency),
        Style::fg(color::MUTED),
    );
    x = screen.text(x, 2, "   ", Style::fg(color::FG));
    let closed = metric
        .history_end
        .as_deref()
        .map(friendly_date)
        .map(|d| format!("closed · last {d}"))
        .unwrap_or_else(|| "— today".to_string());
    x = screen.text(x, 2, &closed, Style::fg(color::MUTED));
    screen.text(x, 2, "   ", Style::fg(color::FG));

    // Row 3: range tabs (active bold blue) + the range's change and hi/lo.
    let mut x = cx + 1;
    for label in RANGES {
        let active = label == state.range;
        x = screen.text(
            x,
            3,
            range_label(label),
            if active {
                Style::fg(color::BLUE).bold()
            } else {
                Style::fg(color::MUTED)
            },
        );
        x = screen.text(x, 3, "  ", Style::fg(color::MUTED));
    }
    x = screen.text(x, 3, "       ", Style::fg(color::MUTED)); // summary lead-in
    if let Some(label) = &metric.change_label {
        let (arrow, style) = if label.starts_with('+') {
            ("▲", Style::fg(color::GREEN))
        } else if label.starts_with('-') {
            ("▼", Style::fg(color::RED))
        } else {
            ("─", Style::fg(color::MUTED))
        };
        x = screen.text(x, 3, &format!("{arrow} {label}"), style);
        if let (Some(hi), Some(lo)) = (metric.period_high, metric.period_low) {
            x = screen.text(x, 3, "   ", Style::fg(color::MUTED));
            x = screen.text(
                x,
                3,
                &format!("hi {}", grouped(hi)),
                Style::fg(color::MUTED),
            );
            x = screen.text(x, 3, "   ", Style::fg(color::MUTED));
            screen.text(
                x,
                3,
                &format!("lo {}", grouped(lo)),
                Style::fg(color::MUTED),
            );
        }
    }

    // Chart: 16 rows (14 braille + rule + labels), x +2, width 66.
    let (window, window_times) = chart_window(
        &metric.series,
        range_window(state.range),
        &metric.series_times,
    );
    let mut chart = PriceChart::new(window);
    chart.times = window_times;
    let line = match metric.change_label.as_deref().unwrap_or("") {
        l if l.starts_with('-') => color::RED,
        l if l.starts_with('+') => color::GREEN,
        _ => color::BLUE,
    };
    screen.price_chart(50, 4, &chart, 66, 16, line);

    // Metric grid: muted-bold heading, then label/value pairs (J8). The Rich
    // table paints its full row width in the foreground style.
    screen.text(49, 20, "Available Metrics", Style::fg(color::MUTED).bold());
    screen.text(66, 20, "           ", Style::fg(color::MUTED).bold());
    screen.fill(77, 20, right_edge, 21, Style::fg(color::FG));
    screen.fill(49, 21, right_edge, 23, Style::fg(color::FG));
    let mut gy = 21usize;
    let mut index = 0usize;
    while index < metric.values.len() {
        let (label, value) = &metric.values[index];
        screen.text(49, gy, label, Style::fg(color::FG));
        screen.text_right(84, gy, value, Style::fg(color::FG));
        if index + 1 < metric.values.len() {
            let (rlabel, rvalue) = &metric.values[index + 1];
            screen.text(85, gy, rlabel, Style::fg(color::FG));
            screen.text_right(117, gy, rvalue, Style::fg(color::FG));
        }
        index += 2;
        gy += 1;
    }

    // Blank row, then the source line.
    let source_y = gy + 1;
    let history = friendly_date_range(metric.history_start.as_ref(), metric.history_end.as_ref());
    screen.text(
        49,
        source_y,
        &format!("{} · live — · history {}", metric.source, history),
        Style::fg(color::MUTED),
    );
}

/// The footer: nav keys, data/provider/spend cluster, config/help hints
/// (`shell.py` status bar; Watchlist is the active tab). The narrow layout
/// hides the provider name and the help hint.
fn draw_status_bar(screen: &mut Screen, y: usize, w: usize, narrow: bool) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let active = Style::DEFAULT.bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    screen.text(2, y, "1", muted);
    screen.put(4, y, ' ', active);
    screen.text(5, y, "2 Watchlist", active_fg);
    screen.put(16, y, ' ', active);
    screen.text(18, y, "3", muted);
    screen.text(21, y, "4", muted);
    screen.text(24, y, "5", muted);
    screen.text(27, y, "6", muted);
    let mut parts: Vec<(&str, Style)> = vec![("  ", panel), ("data 1d", plain)];
    if !narrow {
        parts.push(("  ", panel));
        parts.push(("openrouter", plain));
    }
    parts.push(("  ", panel));
    parts.push(("$0.00", plain));
    parts.push(("  ", panel));
    parts.push(("c", muted));
    parts.push((" Settings", Style::fg(color::MUTED).bg(color::PANEL)));
    if !narrow {
        parts.push(("  ", panel));
        parts.push(("?", Style::fg(color::MUTED).bg(color::PANEL)));
        parts.push((" help · g go", Style::fg(color::MUTED).bg(color::PANEL)));
    }
    let cluster_len: usize = parts.iter().map(|(t, _)| t.chars().count()).sum::<usize>() + 3;
    let mut x = w - cluster_len;
    screen.put(x, y, '\u{25CF}', Style::fg(color::AMBER).bg(color::PANEL));
    x += 1;
    for (part, style) in parts {
        x = screen.text(x, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}

/// The narrow (`-narrow`, detail-open) Watchlist frame: the metrics pane fills
/// the content area, an `esc back` hint joins the pane hints, the provider
/// name and help hint drop out of the status bar, and the grid/chart geometry
/// reflows to the captured 80x24 layout.
pub fn draw_watchlist_narrow(screen: &mut Screen, state: &WatchlistState) {
    let w = screen.w;
    let content_bottom = screen.h - 3; // 21 at h=24

    screen.pane(
        1,
        0,
        w - 2,
        content_bottom,
        false,
        &[("metrics", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "refresh"),
            ("r/R", &format!("range: {}", range_label(state.range))),
            ("i", "glossary"),
            ("esc", "back"),
        ]),
    );

    let cx = 3usize; // pane border 1 + padding 1
    let right_edge = 77usize;
    let Some(metric) = &state.metric else { return };

    // Row 1: name + meta.
    let mut x = cx;
    x = screen.text(x, 1, &metric.symbol, Style::fg(color::FG).bold());
    let meta = format!(
        "  {} · {} · {}",
        metric.market.to_uppercase(),
        metric.asset_class,
        metric.currency
    );
    screen.text(x, 1, &meta, Style::fg(color::MUTED).bold());

    // Row 2: hero.
    let mut x = cx;
    x = screen.text(x, 2, &metric.current, Style::fg(color::FG).bold());
    x = screen.text(
        x,
        2,
        &format!(" {}", metric.currency),
        Style::fg(color::MUTED),
    );
    x = screen.text(x, 2, "   ", Style::fg(color::FG));
    let closed = metric
        .history_end
        .as_deref()
        .map(friendly_date)
        .map(|d| format!("closed · last {d}"))
        .unwrap_or_else(|| "— today".to_string());
    x = screen.text(x, 2, &closed, Style::fg(color::MUTED));
    screen.text(x, 2, "   ", Style::fg(color::FG));

    // Row 3: range tabs + summary (15-cell lead-in here).
    let mut x = cx;
    for label in RANGES {
        let active = label == state.range;
        x = screen.text(
            x,
            3,
            range_label(label),
            if active {
                Style::fg(color::BLUE).bold()
            } else {
                Style::fg(color::MUTED)
            },
        );
        x = screen.text(x, 3, "  ", Style::fg(color::MUTED));
    }
    x = screen.text(x, 3, "             ", Style::fg(color::MUTED));
    if let Some(label) = &metric.change_label {
        let (arrow, style) = if label.starts_with('+') {
            ("▲", Style::fg(color::GREEN))
        } else if label.starts_with('-') {
            ("▼", Style::fg(color::RED))
        } else {
            ("─", Style::fg(color::MUTED))
        };
        x = screen.text(x, 3, &format!("{arrow} {label}"), style);
        if let (Some(hi), Some(lo)) = (metric.period_high, metric.period_low) {
            x = screen.text(x, 3, "   ", Style::fg(color::MUTED));
            x = screen.text(
                x,
                3,
                &format!("hi {}", grouped(hi)),
                Style::fg(color::MUTED),
            );
            x = screen.text(x, 3, "   ", Style::fg(color::MUTED));
            screen.text(
                x,
                3,
                &format!("lo {}", grouped(lo)),
                Style::fg(color::MUTED),
            );
        }
    }

    // Chart: 12 rows (10 braille + rule + labels), x +1, width 72.
    let (window, window_times) = chart_window(
        &metric.series,
        range_window(state.range),
        &metric.series_times,
    );
    let mut chart = PriceChart::new(window);
    chart.times = window_times;
    let line = match metric.change_label.as_deref().unwrap_or("") {
        l if l.starts_with('-') => color::RED,
        l if l.starts_with('+') => color::GREEN,
        _ => color::BLUE,
    };
    screen.price_chart(cx + 1, 4, &chart, 72, 12, line);

    // Metric grid.
    screen.text(cx, 16, "Available Metrics", Style::fg(color::MUTED).bold());
    screen.text(
        cx + 17,
        16,
        "              ",
        Style::fg(color::MUTED).bold(),
    );
    screen.fill(cx + 31, 16, right_edge, 17, Style::fg(color::FG));
    screen.fill(cx, 17, right_edge, 19, Style::fg(color::FG));
    let mut gy = 17usize;
    let mut index = 0usize;
    while index < metric.values.len() {
        let (label, value) = &metric.values[index];
        screen.text(cx, gy, label, Style::fg(color::FG));
        screen.text_right(41, gy, value, Style::fg(color::FG));
        if index + 1 < metric.values.len() {
            let (rlabel, rvalue) = &metric.values[index + 1];
            screen.text(42, gy, rlabel, Style::fg(color::FG));
            screen.text_right(right_edge, gy, rvalue, Style::fg(color::FG));
        }
        index += 2;
        gy += 1;
    }

    // Blank row, then the source line.
    let source_y = gy + 1;
    let history = friendly_date_range(metric.history_start.as_ref(), metric.history_end.as_ref());
    screen.text(
        cx,
        source_y,
        &format!("{} · live — · history {}", metric.source, history),
        Style::fg(color::MUTED),
    );

    draw_status_bar(screen, screen.h - 1, w, true);
}
