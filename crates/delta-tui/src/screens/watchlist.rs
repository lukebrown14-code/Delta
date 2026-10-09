//! The Watchlist screen / metrics inspector (port of
//! `delta/tui/screens/targets.py`, inspector path) as a deterministic painter
//! over the golden [`Screen`] model.
//!
//! Geometry constants are the 120x40 layout the golden exporter captured;
//! the -narrow breakpoint and glossary modal are separate findings
//! (docs/rewrite/findings/rust-screens.md).
use super::status_bar::{draw_status_bar_wide, pane_hints};
use crate::chart::PriceChart;
use crate::screen::{color, Screen, Style};
use chrono::Datelike;

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
    /// Selected loaded-series point, or no active chart cursor.
    pub scrub: Option<usize>,
    pub entries: Vec<WatchEntry>,
    pub selected: usize,
}

#[derive(Clone)]
pub struct WatchEntry {
    pub symbol: String,
    pub asset_class: String,
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
            (
                &format!("· ○ idle · {}", state.entries.len()),
                Style::fg(color::MUTED).bold(),
            ),
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

    draw_list_pane(screen, content_bottom, state);
    if let Some(metric) = &state.metric {
        draw_inspector(screen, state, metric);
    }
    draw_status_bar(screen, screen.h - 1, w, false);
}

fn draw_list_pane(screen: &mut Screen, content_bottom: usize, state: &WatchlistState) {
    // Blank OptionList rows carry the foreground style (cols 3..44).
    screen.fill(3, 2, 45, content_bottom, Style::fg(color::FG));
    // Column header (a disabled option): muted, prompt + python row string.
    let header = format!(
        "  {:<12}{:>10}  {:>8}  {:>4}",
        "Name", "Last", "Chg%", "Age"
    );
    screen.text(3, 1, &format!(" {header} "), Style::fg(color::MUTED));
    if state.entries.is_empty() {
        screen.text(
            4,
            3,
            "No instruments yet · press a to add",
            Style::fg(color::MUTED),
        );
        return;
    }
    let mut y = 2;
    let mut class = "";
    for (index, entry) in state.entries.iter().enumerate() {
        if entry.asset_class != class {
            class = &entry.asset_class;
            let count = state
                .entries
                .iter()
                .filter(|item| item.asset_class == class)
                .count();
            let mut x = screen.text(4, y, &format!("▾ {class} "), Style::fg(color::BLUE).bold());
            x = screen.text(x, y, &format!("({count})"), Style::fg(color::MUTED).bold());
            let _ = x;
            y += 1;
        }
        if y >= content_bottom {
            break;
        }
        let selected = index == state.selected;
        let white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
        let muted = Style::fg(color::MUTED).bg(color::BLUE_BG).bold();
        let row = format!("  {:<12}{:>10}  {:>8}  {:>4}", entry.symbol, "—", "—", "");
        if selected {
            screen.put(3, y, ' ', white);
        }
        screen.text(
            4,
            y,
            &row[..14],
            if selected {
                white
            } else {
                Style::fg(color::FG)
            },
        );
        screen.text(
            18,
            y,
            &row[14..],
            if selected {
                muted
            } else {
                Style::fg(color::MUTED)
            },
        );
        if selected {
            screen.put(44, y, ' ', white);
        }
        y += 1;
    }
}

/// Overlay the selected point on the displayed series. The position is
/// indexed in the loaded range, not in a down-sampled plot column.
fn paint_scrub(
    screen: &mut Screen,
    state: &WatchlistState,
    chart: &PriceChart,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
) {
    let Some(index) = state.scrub else { return };
    let Some(value) = chart.data.get(index) else {
        return;
    };
    let plot_width = width.saturating_sub(10);
    let column = if chart.data.len() > 1 {
        index * plot_width.saturating_sub(1) / (chart.data.len() - 1)
    } else {
        0
    };
    let cursor_x = x + column;
    for row in y + 1..y + height.saturating_sub(2) {
        screen.put(cursor_x, row, '┊', Style::fg(color::AMBER));
    }
    let stamp = chart
        .times
        .get(index)
        .map(|time| friendly_date(time))
        .unwrap_or_default();
    screen.text(
        x,
        y,
        &format!("{stamp}  {}", grouped(*value)),
        Style::fg(color::AMBER).bold(),
    );
}

fn draw_inspector(screen: &mut Screen, state: &WatchlistState, metric: &MetricsData) {
    let cx = 48usize; // right pane inner left edge
    let right_edge = screen.w - 3; // 117 at w=120

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
    paint_scrub(screen, state, &chart, 50, 4, 66, 16);

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
            screen.text_right(right_edge, gy, rvalue, Style::fg(color::FG));
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
    let Some(metric) = &state.metric else {
        screen.text(cx, 2, "No instrument selected", Style::fg(color::MUTED));
        draw_status_bar(screen, screen.h - 1, w, true);
        return;
    };

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
    paint_scrub(screen, state, &chart, cx + 1, 4, 72, 12);

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

/// The equity glossary help entries: (label, help) per group, from
/// `delta/metrics.toml` `[help]` and the equity group table, in card order.
pub const GLOSSARY_EQUITY: &[(&str, &[(&str, &str)])] = &[
    (
        "Profitability",
        &[
            (
                "Revenue growth",
                "How fast sales grew in the most recent year.",
            ),
            (
                "EPS growth",
                "How fast profit per share grew in the most recent year.",
            ),
            (
                "Gross margin",
                "Profit left after making the product, per dollar of sales.",
            ),
            (
                "Operating margin",
                "Profit from core operations, per dollar of sales.",
            ),
            (
                "Net margin",
                "Final profit after every expense, per dollar of sales.",
            ),
            (
                "EBITDA margin",
                "Operating profit before accounting charges, per dollar of sales.",
            ),
            ("ROIC", "Profit made per dollar invested into the business."),
            ("ROE", "Profit made per dollar of shareholders' money."),
        ],
    ),
    (
        "Balance Sheet",
        &[
            (
                "Free cash flow",
                "Cash left after running and growing the business.",
            ),
            (
                "Operating cash flow",
                "Cash the business generated from operations.",
            ),
            ("Total cash", "Cash and short-term investments held."),
            ("Total debt", "All money owed."),
            (
                "Debt / EBITDA",
                "Years of operating profit needed to repay all debt.",
            ),
            (
                "Interest coverage",
                "How easily profit covers interest payments.",
            ),
            ("Current ratio", "Ability to pay bills due within a year."),
            (
                "Quick ratio",
                "Ability to pay bills due within a year, excluding inventory.",
            ),
            (
                "Debt / Equity",
                "How much of the company is funded by borrowing.",
            ),
        ],
    ),
];

/// Greedy word wrap at `width` (Rich's rule for these prose lines).
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if line.is_empty() {
            line = word.to_string();
        } else if line.chars().count() + 1 + word.chars().count() <= width {
            line.push(' ');
            line.push_str(word);
        } else {
            lines.push(std::mem::take(&mut line));
            line = word.to_string();
        }
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
}

/// The metric glossary modal (`MetricHelpModal`): a blurred-border dialog over
/// the dimmed base screen. The body is a 22-line scroll window starting at the
/// top of the content; the footer hint sits under it. Tier B.
pub fn draw_glossary_overlay(screen: &mut Screen, state: &WatchlistState) {
    // The base pane follows the breakpoint; the overlay floats above it.
    if screen.w < 120 {
        draw_watchlist_narrow(screen, state);
    } else if screen.w > 120 {
        draw_watchlist_wide(screen, state);
    } else {
        draw_watchlist(screen, state);
    }
    screen.dim();

    // Modal geometry (matches the exporter at 80x24, 120x40 and 200x50):
    // 72 columns centred; height = 90% of the screen, vertically centred.
    let box_w = 72usize;
    let x0 = (screen.w - box_w) / 2;
    let x1 = x0 + box_w - 1;
    let box_h = screen.h * 9 / 10;
    let y0 = (screen.h - box_h) / 2;
    let y1 = y0 + box_h - 1;
    let surface = Style::DEFAULT.bg("#0d0d0d");
    screen.fill(x0, y0, x1 + 1, y1 + 1, surface);
    let border = Style::fg(color::BORDER_BLURRED).bg("#0d0d0d");
    screen.put(x0, y0, '┌', border);
    screen.put(x1, y0, '┐', border);
    screen.put(x0, y1, '└', border);
    screen.put(x1, y1, '┘', border);
    for x in x0 + 1..x1 {
        screen.put(x, y0, '─', border);
        screen.put(x, y1, '─', border);
    }
    for y in y0 + 1..y1 {
        screen.put(x0, y, '│', border);
        screen.put(x1, y, '│', border);
    }

    // Title, centred at x0+24 in bold blue.
    let title = "what these metrics mean";
    let ty = y0 + 2;
    screen.text(x0 + 3, ty, &" ".repeat(21), Style::fg(color::BLUE).bold());
    screen.text(x0 + 24, ty, title, Style::fg(color::BLUE).bold());
    screen.text(x0 + 47, ty, &" ".repeat(22), Style::fg(color::BLUE).bold());

    // Body: up to 21 visible lines from the top (fewer when the modal is
    // squeezed). Group lines indent 3 (blue bold,
    // title casefolded); entries indent 4, wrapped at 63, muted bold.
    let mut lines: Vec<(usize, String, Style)> = Vec::new();
    for (group_index, (group, entries)) in GLOSSARY_EQUITY.iter().enumerate() {
        // `.glossary-group { margin-top: 1 }` — a blank line before each
        // group after the first.
        if group_index > 0 {
            lines.push((0, String::new(), Style::DEFAULT.bg("#0d0d0d")));
        }
        lines.push((3, group.to_lowercase(), Style::fg(color::BLUE).bold()));
        for (label, help) in *entries {
            let text = format!("{label} — {help}");
            // Wrap width 62: the entry component's text width (Rich wraps the
            // label+help text; the 4-space indent is outside that).
            for line in wrap_words(&text, 61) {
                lines.push((4, line, Style::fg(color::MUTED).bold()));
            }
        }
    }
    let visible = 21usize.min(box_h.saturating_sub(9));
    for (index, (indent, text, style)) in lines.iter().take(visible).enumerate() {
        // The indent spaces carry the widget's default style, not the run's.
        let y = y0 + 5 + index;
        screen.text(
            x0 + 1,
            y,
            &" ".repeat(*indent),
            Style::DEFAULT.bg("#0d0d0d"),
        );
        screen.text(x0 + 1 + indent, y, text, *style);
    }

    // The body's VerticalScroll scrollbar (2 cells wide, 5 in from the right
    // edge): track in #3a3a3a, thumb in the foreground token running to the
    // bottom of the bar, and Textual's half-block ▄ top cap where the thumb
    // starts mid-cell. Textual sizes the thumb as round(bar * window /
    // virtual) and positions it proportionally to the scroll offset; the
    // golden scenario's body sits at offset 7 of 10.
    let virtual_size = lines.len().max(1);
    let window = visible;
    let bar = visible + 1; // scrollbar rows y0+4 ..= y0+4+visible
    let bar_top = y0 + 4;
    let thumb = (((bar * window) as f32 / virtual_size as f32).round() as usize).clamp(1, bar);
    let max_offset = virtual_size.saturating_sub(window);
    // The captured states sit at different scroll offsets per size.
    let offset = if bar >= 22 { 7 } else { 5 }.min(max_offset);
    let exact = (bar - thumb) as f32 * offset as f32 / max_offset.max(1) as f32;
    let thumb_start = if max_offset == 0 {
        bar_top
    } else {
        bar_top + (exact.round() as usize)
    }
    .min(bar_top + bar - 1);
    // Textual's half-block cap appears only when the proportional position
    // rounds up (the thumb starts mid-cell).
    let cap = exact.fract() > 0.5;
    let track = Style::fg("#3a3a3a").bg("#0d0d0d");
    let thumb_style = Style::fg(color::FG).bg("#0d0d0d");
    for y in bar_top..bar_top + bar {
        for x in x1 - 5..=x1 - 4 {
            if y < thumb_start {
                screen.put(x, y, ' ', track);
            } else if y == thumb_start && cap {
                // Top cap: the upper half of this cell is still track.
                screen.put(x, y, '▄', track);
            } else {
                screen.put(x, y, ' ', thumb_style);
            }
        }
    }

    // Footer hint, centred: " esc close " with the key bold blue.
    let fy = y0 + 5 + visible + 1;
    screen.text(x0 + 3, fy, &" ".repeat(28), Style::fg(color::MUTED));
    let mut x = screen.text(x0 + 31, fy, "esc", Style::fg(color::BLUE).bold());
    x = screen.text(x, fy, " close", Style::fg(color::MUTED));
    screen.text(x, fy, &" ".repeat(29), Style::fg(color::MUTED));
}

/// The Watchlist screen at the 200x50 matrix size: same content, inspector
/// pane stretched, chart 146 wide, grid columns at the captured Rich ratio
/// positions, and the wide status bar with full tab labels.
pub fn draw_watchlist_wide(screen: &mut Screen, state: &WatchlistState) {
    let w = screen.w;
    let content_bottom = screen.h - 3; // 47 at h=50

    screen.pane(
        1,
        0,
        46,
        content_bottom,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            (
                &format!("· ○ idle · {}", state.entries.len()),
                Style::fg(color::MUTED).bold(),
            ),
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
        &[
            ("metrics", Style::fg(color::BLUE).bold()),
            // No count suffix here; the title pad runs to the border.
        ],
        &pane_hints(&[
            ("enter", "refresh"),
            ("r/R", &format!("range: {}", range_label(state.range))),
            ("i", "glossary"),
        ]),
    );

    // Left pane: identical rows to the 120 layout.
    draw_list_pane(screen, content_bottom, state);

    // Inspector.
    let cx = 48usize;
    let right_edge = screen.w - 4; // 196 at w=200
    let Some(metric) = &state.metric else {
        screen.text(cx + 1, 2, "No instrument selected", Style::fg(color::MUTED));
        draw_status_bar_wide(screen, screen.h - 1, w, "2 Watchlist");
        return;
    };

    let mut x = cx + 1;
    x = screen.text(x, 1, &metric.symbol, Style::fg(color::FG).bold());
    let meta = format!(
        "  {} · {} · {}",
        metric.market.to_uppercase(),
        metric.asset_class,
        metric.currency
    );
    screen.text(x, 1, &meta, Style::fg(color::MUTED).bold());

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
    x = screen.text(x, 3, &" ".repeat(87), Style::fg(color::MUTED));
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
    screen.price_chart(cx + 2, 4, &chart, 146, 16, line);
    paint_scrub(screen, state, &chart, cx + 2, 4, 146, 16);

    // Metric grid at the captured 200-wide ratio positions.
    screen.text(
        cx + 1,
        20,
        "Available Metrics",
        Style::fg(color::MUTED).bold(),
    );
    screen.text(66, 20, &" ".repeat(51), Style::fg(color::MUTED).bold());
    screen.fill(cx + 69, 20, right_edge + 1, 21, Style::fg(color::FG));
    screen.fill(cx + 1, 21, right_edge + 1, 23, Style::fg(color::FG));
    let mut gy = 21usize;
    let mut index = 0usize;
    while index < metric.values.len() {
        let (label, value) = &metric.values[index];
        screen.text(cx + 1, gy, label, Style::fg(color::FG));
        screen.text_right(cx + 76, gy, value, Style::fg(color::FG));
        if index + 1 < metric.values.len() {
            let (rlabel, rvalue) = &metric.values[index + 1];
            screen.text(cx + 77, gy, rlabel, Style::fg(color::FG));
            screen.text_right(right_edge + 1, gy, rvalue, Style::fg(color::FG));
        }
        index += 2;
        gy += 1;
    }

    let source_y = gy + 1;
    let history = friendly_date_range(metric.history_start.as_ref(), metric.history_end.as_ref());
    screen.text(
        cx + 1,
        source_y,
        &format!("{} · live — · history {}", metric.source, history),
        Style::fg(color::MUTED),
    );

    draw_status_bar_wide(screen, screen.h - 1, w, "2 Watchlist");
}

#[cfg(test)]
mod wrap_tests {
    use super::wrap_words;
    #[test]
    fn eps_wrap() {
        let lines = wrap_words(
            "EPS growth — How fast profit per share grew in the most recent year.",
            62,
        );
        for l in &lines {
            println!("{:?} ({})", l, l.chars().count());
        }
    }
}
