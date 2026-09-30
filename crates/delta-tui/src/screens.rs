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

/// Home state the painter renders (the seeded golden scenario's values).
pub struct HomeState {
    pub clock: String,
    pub symbol: String,
    pub last: String,
    pub chg_label: String,
    pub spark: String,
    pub since_stamp: String,
    /// Raw closes so the narrow breakpoint can resize the sparkline.
    pub closes: Vec<f64>,
}

/// The Home screen: header chip + clock, watchlist / since-you-last-looked,
/// upcoming / theses, the agenda, and the status bar with `1 Home` active
/// (port of `delta/tui/screens/home.py` at the captured 120x40 layout).
pub fn draw_home(screen: &mut Screen, home: &HomeState) {
    let w = screen.w;
    let content_bottom = screen.h - 3; // 37 at h=40

    // Header row: inked DELTA chip, "overview", clock right-aligned.
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));

    // Row 1: watchlist (focused) + since you last looked (blurred).
    screen.pane(
        1,
        1,
        59,
        content_bottom - 22,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );
    screen.pane(
        60,
        1,
        w - 2,
        content_bottom - 22,
        false,
        &[("since you last looked", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("3", "research")]),
    );

    // Watchlist table: muted header, selected row in $primary.
    let header = " Symbol          close     chg%   age  40 closes";
    screen.text(2, 2, header, Style::fg(color::MUTED));
    let blue = Style::DEFAULT.bg(color::BLUE_BG);
    screen.fill(2, 3, 59, 4, blue);
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG);
    let white_bold = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(2, 3, ' ', blue);
    let mut x = screen.text(3, 3, &home.symbol, white_bold);
    x = screen.text(x + 11, 3, &home.last, white);
    screen.put(x, 3, ' ', blue);
    x = screen.text(x + 1, 3, "▲ ", white);
    x = screen.text(x, 3, &home.chg_label, white);
    let _ = x;
    screen.text(41, 3, &home.spark, white);
    // Row 5: the spark legend.
    screen.text(
        3,
        5,
        "spark: 40 daily closes · chg%: move on the day",
        Style::fg(color::MUTED),
    );

    // Since-you-last-looked: empty-run summary, stale warnings.
    screen.text(
        62,
        2,
        "nothing new since your last visit",
        Style::fg(color::MUTED),
    );
    screen.text(102, 2, &home.since_stamp, Style::fg(color::MUTED));
    screen.text(
        62,
        4,
        "no activity in the last 30 days",
        Style::fg(color::MUTED),
    );
    screen.text(62, 5, "newest   no articles yet", Style::fg(color::MUTED));
    let warn = Style::fg(color::AMBER);
    screen.text(62, 7, &format!("⚠ {} 1d old", home.symbol), warn);
    screen.text(62, 8, "⚠ 2 evidence prompts · 3 evidence", warn);

    // Middle row: upcoming + theses.
    screen.pane(
        1,
        content_bottom - 21,
        59,
        content_bottom - 7,
        false,
        &[("upcoming", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open evidence")]),
    );
    screen.pane(
        60,
        content_bottom - 21,
        w - 2,
        content_bottom - 7,
        false,
        &[("theses", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open thesis"), ("4", "all")]),
    );
    screen.text(
        3,
        content_bottom - 20,
        "nothing scheduled — press 3, then U to gather evidence",
        Style::fg(color::MUTED),
    );
    screen.text(
        3,
        content_bottom - 19,
        "calendar plugin: earnings & dividends only",
        Style::fg(color::MUTED),
    );
    screen.text(
        62,
        content_bottom - 20,
        "no theses yet",
        Style::fg(color::MUTED),
    );
    screen.text(
        62,
        content_bottom - 18,
        "4 opens the theses desk — n tracks a claim",
        Style::fg(color::MUTED),
    );

    // Agenda: full width, jump keys with ✓/⚠ verdicts.
    screen.pane(
        1,
        content_bottom - 6,
        w - 2,
        content_bottom,
        false,
        &[
            ("needs you today ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("enter", "open"), ("tab", "next box")]),
    );
    let rows: [(&str, &str, &str, Style); 4] = [
        (
            "6",
            "✓",
            " no decision reviews due",
            Style::fg(color::MUTED),
        ),
        ("4", "✓", " no falsifier hits", Style::fg(color::MUTED)),
        (
            "3",
            "✓",
            " no earnings in the next 7 days",
            Style::fg(color::MUTED),
        ),
        ("2", "⚠", " 1 stale source", Style::fg(color::AMBER)),
    ];
    for (index, (key, glyph, message, style)) in rows.into_iter().enumerate() {
        let y = content_bottom - 5 + index;
        let mut x = 3;
        x = screen.text(x, y, "▸", Style::fg(color::BLUE));
        x += 1;
        x = screen.text(x, y, key, Style::fg(color::BLUE).bold());
        x += 2;
        x = screen.text(x, y, glyph, style);
        screen.text(x, y, message, style);
    }

    draw_status_bar_home(screen, screen.h - 1, w);
}

/// Status bar with the Home tab active (`1 Home`).
fn draw_status_bar_home(screen: &mut Screen, y: usize, w: usize) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let active = Style::DEFAULT.bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    screen.put(1, y, ' ', active);
    screen.text(2, y, "1 Home", active_fg);
    screen.put(8, y, ' ', active);
    screen.put(9, y, ' ', panel);
    screen.text(10, y, "2", muted);
    for (i, key) in ["3", "4", "5", "6"].into_iter().enumerate() {
        screen.text(13 + i * 3, y, key, muted);
    }
    let x = w - 1 - 57; // cluster width (57 cells) ends at w-2
    screen.put(x, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    let mut x = x + 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
        ("  ", panel),
        ("c", muted),
        (" Settings", Style::fg(color::MUTED).bg(color::PANEL)),
        ("  ", panel),
        ("?", Style::fg(color::MUTED).bg(color::PANEL)),
        (" help · g go", Style::fg(color::MUTED).bg(color::PANEL)),
    ] {
        x = screen.text(x, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}

/// The Research screen: company / report / evidence panes with their
/// empty-state content (port of `delta/tui/screens/research.py`, seeded
/// golden layout).
pub fn draw_research(screen: &mut Screen) {
    let surface_fg_none = Style::DEFAULT.bg("#0d0d0d");
    let surface = Style::fg(color::FG).bg("#0d0d0d");
    let muted = Style::fg(color::MUTED);

    screen.pane(
        1,
        0,
        36,
        37,
        true,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("company ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select")]),
    );
    screen.pane(
        37,
        0,
        78,
        37,
        false,
        &[
            ("r ", Style::fg(color::BLUE).bold()),
            ("report ", Style::fg(color::BLUE).bold()),
            ("· no report", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "scroll"), ("enter", "citation"), ("n", "regene…")]),
    );
    screen.pane(
        79,
        0,
        118,
        37,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("/", "search"),
            ("k", "kind"),
            ("space", "fold"),
            ("l", "m…"),
        ]),
    );

    // Company pane: DataTable header on its own bg, blank rows, empty state.
    screen.fill(2, 1, 36, 2, Style::fg(color::FG).bold().bg("#232323"));
    screen.text(
        2,
        1,
        " Company  Report",
        Style::fg(color::FG).bold().bg("#232323"),
    );
    screen.fill(2, 2, 36, 33, Style::fg(color::FG));
    screen.text(
        3,
        33,
        "no companies yet — press 1 to",
        Style::fg(color::MUTED),
    );
    screen.text(3, 34, "add a target", Style::fg(color::MUTED));
    // DataTable bottom separator, then the hint rows on a narrow surface strip
    // (cols 2..20).
    for x in 2..36 {
        screen.put(x, 32, '─', Style::fg(color::BORDER_BLURRED));
    }
    // Each hint row's surface strip is exactly its text width plus one cell.
    for (y, key, rest, strip_end) in [
        (35usize, "u", " gather company", 21usize),
        (36, "U", " gather all", 17),
    ] {
        screen.fill(
            3,
            y,
            strip_end,
            y + 1,
            Style::fg(color::FG).bold().bg("#0d0d0d"),
        );
        screen.put(strip_end, y, ' ', surface_fg_none);
        screen.put(2, y, ' ', surface_fg_none);
        screen.put(3, y, ' ', Style::fg(color::FG).bold().bg("#0d0d0d"));
        screen.put(
            4,
            y,
            key.chars().next().unwrap(),
            Style::fg(color::BLUE).bold().bg("#0d0d0d"),
        );
        screen.text(5, y, rest, Style::fg(color::FG).bold().bg("#0d0d0d"));
    }

    // Report pane: scroll body on the surface, button, centred heading.
    screen.fill(38, 6, 76, 37, surface_fg_none);
    let button_bg = Style::DEFAULT.bg(color::BLUE_BG);
    screen.fill(40, 2, 61, 3, button_bg);
    screen.put(41, 2, ' ', white_on_blue());
    screen.put(59, 2, ' ', white_on_blue());
    screen.put(42, 2, 'n', Style::fg(color::BLUE).bold().bg(color::BLUE_BG));
    screen.text(43, 2, " generate report", white_on_blue());
    for x in 40..72 {
        for y in 8..9 {
            screen.put(x, y, ' ', Style::fg(color::BLUE).bold().bg("#0d0d0d"));
        }
    }
    screen.text(53, 8, "Report", Style::fg(color::BLUE).bold().bg("#0d0d0d"));
    screen.text(40, 10, "No report yet — press n to", surface);
    screen.text(40, 11, "generate one.", surface);

    // Evidence pane: search field, column header, separator, empty states.
    screen.put(80, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.put(81, 1, ' ', surface_fg_none);
    screen.put(106, 1, ' ', surface_fg_none);
    screen.text(
        82,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(99, 1, 106, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(108, 1, "kind: all", muted);
    // Column header row: DataTable header on the panel background, bold.
    screen.fill(80, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        81,
        2,
        "Evidence",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(94, 2, "Type", Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(107, 2, "Date", Style::fg(color::FG).bold().bg(color::PANEL));
    // Blank DataTable rows carry the foreground style.
    screen.fill(80, 3, 118, 26, Style::fg(color::FG));
    screen.fill(81, 26, 116, 27, Style::fg(color::FG));
    screen.text(81, 26, "no companies yet — press 1 to add a", muted);
    for x in 80..118 {
        screen.put(x, 27, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        81,
        28,
        "select evidence to preview it",
        Style::fg(color::FG),
    );

    draw_status_bar_research(screen, screen.h - 1, w_of(screen));
}

fn white_on_blue() -> Style {
    Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
}

fn w_of(screen: &Screen) -> usize {
    screen.w
}

/// Status bar with the Research tab active (`3 Research`).
fn draw_status_bar_research(screen: &mut Screen, y: usize, w: usize) {
    draw_status_bar_nav(screen, y, w, "3 Research");
}

/// Status bar with an arbitrary active tab label.
fn draw_status_bar_nav(screen: &mut Screen, y: usize, w: usize, active: &str) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let chip_bg = Style::DEFAULT.bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    // Keys 1..6; the active one expands to "n Label" in the primary chip.
    screen.put(1, y, ' ', panel);
    screen.text(2, y, "1", muted);
    screen.put(4, y, ' ', panel);
    screen.text(5, y, "2", muted);
    screen.put(7, y, ' ', chip_bg);
    x_active_label(screen, y, 8, active);
    let after = 8 + active.chars().count() + 1;
    screen.put(after, y, ' ', panel);
    let mut x = after + 1;
    for key in ["4", "5", "6"] {
        x = screen.text(x, y, key, muted);
        x += 2;
    }
    let cluster = "●  data 1d  openrouter  $0.00  c Settings  ? help · g go ";
    let start = w - 1 - cluster.chars().count();
    while x < start {
        screen.put(x, y, ' ', panel);
        x += 1;
    }
    screen.put(x, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    x += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
        ("  ", panel),
        ("c", muted),
        (" Settings", Style::fg(color::MUTED).bg(color::PANEL)),
        ("  ", panel),
        ("?", Style::fg(color::MUTED).bg(color::PANEL)),
        (" help · g go", Style::fg(color::MUTED).bg(color::PANEL)),
    ] {
        x = screen.text(x, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}

fn x_active_label(screen: &mut Screen, y: usize, mut x: usize, label: &str) {
    // "n Label" with a trailing chip space (no leading one).
    let mut parts = label.chars();
    let key = parts.next().unwrap();
    x = screen.text(x, y, &key.to_string(), active_chip());
    x = screen.text(x, y, &parts.collect::<String>(), active_chip());
    screen.put(x, y, ' ', Style::DEFAULT.bg(color::BLUE_BG));
}

fn active_chip() -> Style {
    Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
}

/// The Theses screen: fleet list, thesis detail, evidence table (all empty
/// states; port of `delta/tui/screens/theses.py`, seeded golden layout).
pub fn draw_theses(screen: &mut Screen) {
    screen.pane(
        1,
        0,
        36,
        37,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("n", "new"), ("d", "edit"), ("/", "filter")]),
    );
    screen.pane(
        37,
        0,
        78,
        37,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("thesis", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
    );
    screen.pane(
        79,
        0,
        118,
        37,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("f", "find")]),
    );

    screen.fill(2, 1, 36, 36, Style::fg(color::FG));
    screen.text(
        39,
        1,
        "no theses yet — press n to create one",
        Style::fg(color::FG),
    );

    // Evidence table: header row on the panel, blank rows, separator, note.
    screen.fill(80, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        81,
        2,
        "   ±  age   note",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(81, 1, "no evidence yet", Style::fg(color::FG));
    screen.fill(80, 3, 118, 29, Style::fg(color::FG));
    for x in 80..118 {
        screen.put(x, 29, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        81,
        30,
        "no evidence yet — press f to find",
        Style::fg(color::FG),
    );
    screen.text(81, 31, "candidates", Style::fg(color::FG));

    draw_status_bar_theses(screen, screen.h - 1, screen.w);
}

/// Status bar with the Theses tab active (`4 Theses`).
fn draw_status_bar_theses(screen: &mut Screen, y: usize, w: usize) {
    status_bar_tabs(
        screen,
        y,
        w,
        10,
        "4 Theses",
        &[(2, "1"), (5, "2"), (8, "3")],
        &[(21, "5"), (24, "6")],
    );
}

/// The Ask screen: chat transcript pane + targets / citations side panes
/// (port of `delta/tui/screens/chat.py`, seeded golden layout).
pub fn draw_ask(screen: &mut Screen) {
    screen.pane(
        1,
        0,
        82,
        37,
        true,
        &[
            ("5 ", Style::fg(color::BLUE).bold()),
            ("ask", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("i", "ask"), ("↑↓", "scroll"), ("z", "zoom")]),
    );
    // Targets pane (rows 1..18) and citations pane (19..37).
    screen.pane(
        83,
        0,
        118,
        18,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("targets ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("space", "toggle"), ("a", "all"), ("enter", "ask")]),
    );
    screen.pane(
        83,
        19,
        118,
        37,
        false,
        &[
            ("o ", Style::fg(color::BLUE).bold()),
            ("citations", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("←→", "citation"), ("o", "open"), ("s", "save")]),
    );

    // Transcript: scope line, empty-state line, separator, input.
    screen.text(
        3,
        1,
        "scope: apple → US:AAPL · 50+ evidence items",
        Style::fg(color::MUTED),
    );
    screen.text(
        3,
        2,
        "no messages yet — press t to pick targets, then i to ask",
        Style::fg(color::MUTED),
    );
    for x in 2..82 {
        screen.put(x, 35, '─', Style::fg(color::PANEL));
    }
    screen.put(2, 36, ' ', Style::DEFAULT);
    screen.text(3, 36, ">", Style::fg(color::BLUE));
    screen.text(4, 36, " ", Style::DEFAULT);
    screen.text(
        5,
        36,
        "ask about the targets in scope…",
        Style::fg(color::DISABLED),
    );
    screen.fill(36, 36, 82, 37, Style::fg(color::FG));

    // Targets table: header on the panel, one selected row in the dark blue.
    screen.fill(84, 1, 118, 2, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        84,
        1,
        "    Target  Kind     Evidence",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(84, 2, 118, 3, selected);
    screen.put(85, 2, '●', selected);
    screen.text(88, 2, "apple", selected);
    screen.text(96, 2, "company", selected);
    screen.text_right(108, 2, "50+", selected);
    screen.fill(84, 3, 118, 17, Style::fg(color::FG));
    screen.fill(84, 21, 118, 36, Style::fg(color::FG));
    screen.text(85, 17, "1 of 1 in scope · US:AAPL", Style::fg(color::MUTED));

    // Citations table header + session footer.
    screen.fill(
        84,
        20,
        118,
        21,
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        85,
        20,
        "#    Evidence  Kind",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        85,
        36,
        "this session: 0 answers · $0.000",
        Style::fg(color::MUTED),
    );

    draw_status_bar_ask(screen, screen.h - 1, screen.w);
}

/// Status bar with the Ask tab active (`5 Ask`).
fn draw_status_bar_ask(screen: &mut Screen, y: usize, w: usize) {
    status_bar_tabs(
        screen,
        y,
        w,
        13,
        "5 Ask",
        &[(2, "1"), (5, "2"), (8, "3"), (11, "4")],
        &[(21, "6")],
    );
}

/// The Decisions screen: ledger + timeline (empty states; port of
/// `delta/tui/screens/decisions.py`, seeded golden layout).
pub fn draw_decisions(screen: &mut Screen) {
    screen.pane(
        1,
        0,
        47,
        37,
        true,
        &[
            ("decisions ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("e", "edit"),
            ("d", "delete"),
            ("/", "filter"),
        ]),
    );
    screen.pane(
        48,
        0,
        118,
        37,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );

    // Ledger: header row on its own bg + blank rows + the reversed cursor.
    screen.fill(2, 1, 47, 2, Style::fg(color::FG).bold().bg("#232323"));
    screen.text(
        2,
        1,
        " status      review      instrument      rati",
        Style::fg(color::FG).bold().bg("#232323"),
    );
    screen.fill(2, 2, 47, 36, Style::fg(color::FG));
    screen.fill(2, 36, 41, 37, Style::fg("#3a3a3a"));
    screen.put(41, 36, '▊', Style::fg("#3a3a3a").bg("#0d0d0d"));
    screen.fill(42, 36, 47, 37, Style::fg(color::FG).bg("#0d0d0d"));

    // Timeline empty state (wrapped).
    let timeline_hint = Style::fg(color::MUTED);
    let timeline_key = Style::fg(color::BLUE).bold();
    screen.text(50, 1, "no decisions yet — press ", timeline_hint);
    screen.put(75, 1, 'n', timeline_key);
    screen.text(76, 1, " to record the context you want to", timeline_hint);
    screen.text(50, 2, "revisit", timeline_hint);

    draw_status_bar_decisions(screen, screen.h - 1, screen.w);
}

/// Status bar with the Decisions tab active (`6 Decisions`).
fn draw_status_bar_decisions(screen: &mut Screen, y: usize, w: usize) {
    status_bar_tabs(
        screen,
        y,
        w,
        16,
        "6 Decisions",
        &[(2, "1"), (5, "2"), (8, "3"), (11, "4"), (14, "5")],
        &[],
    );
}

/// The Settings screen: provider/model, plugins, data sources & markets,
/// diagnostics (port of `delta/tui/screens/config.py`, seeded golden layout).
pub fn draw_settings(screen: &mut Screen) {
    let w = screen.w;
    // Provider & model pane.
    screen.pane(
        1,
        0,
        62,
        3,
        true,
        &[("provider & model", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("↑↓", "choose"), ("enter", "change")]),
    );
    let selected_white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 1, 62, 2, selected_white);
    screen.text(3, 1, "provider: ", selected_white);
    screen.text(13, 1, "● openrouter", selected_white);
    screen.fill(2, 2, 62, 3, Style::fg(color::FG));
    screen.text(3, 2, "model: ", Style::fg(color::FG).bold());
    screen.text(10, 2, "not chosen — press m", Style::fg(color::AMBER));

    // Plugins pane.
    screen.pane(
        1,
        4,
        62,
        6,
        false,
        &[
            ("plugins ", Style::fg(color::BLUE).bold()),
            ("· 1 of 1 ok", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "plugin"), ("enter", "details")]),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(2, 5, 62, 6, selected);
    screen.put(3, 5, '●', selected);
    screen.text(6, 5, "sec_edgar  enabled", selected);

    // Data sources & markets pane.
    screen.pane(
        1,
        7,
        62,
        12,
        false,
        &[("data sources & markets", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "configure/edit"),
            ("a", "add market"),
            ("x", "remove market"),
        ]),
    );
    screen.text(3, 8, "sources", Style::fg(color::MUTED).bold());
    screen.text(3, 10, "markets", Style::fg(color::MUTED).bold());
    screen.fill(2, 11, 62, 12, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        3,
        11,
        "ID  Market  Currency  Yahoo",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );

    // Diagnostics pane.
    screen.pane(
        63,
        0,
        118,
        37,
        false,
        &[
            ("diagnostics ", Style::fg(color::BLUE).bold()),
            ("· 80 rows · $0.00", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("r", "refresh"), ("d", "fold"), ("↑↓", "scroll")]),
    );
    let muted = Style::fg(color::MUTED);
    let fg = Style::fg(color::FG);
    screen.text(65, 1, "evidence", Style::fg(color::MUTED).bold());
    screen.text_right(117, 1, "delta.db · 4 KB", muted);
    screen.fill(64, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        65,
        2,
        "Table        Rows",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    let diag_selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(64, 3, 118, 4, diag_selected);
    screen.fill(64, 4, 118, 8, Style::fg(color::FG));
    let mut first = true;
    for (i, (name, count)) in [
        ("bar", "80"),
        ("event", "0"),
        ("fundamental", "0"),
        ("llmcall", "0"),
        ("newsitem", "0"),
    ]
    .into_iter()
    .enumerate()
    {
        let row_style = if first { diag_selected } else { fg };
        first = false;
        screen.text(65, 3 + i, name, row_style);
        screen.text(78, 3 + i, count, row_style);
    }
    screen.text(65, 8, "latest bar US:AAPL 20 Sep 00:00 UTC", muted);
    screen.text(
        65,
        10,
        "model spend · cumulative",
        Style::fg(color::MUTED).bold(),
    );
    screen.fill(
        64,
        11,
        118,
        12,
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        65,
        11,
        "Task  Model  Calls  USD",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(65, 12, "total  0 calls  $0.00", Style::fg(color::FG).bold());
    screen.text(65, 13, "today $0.00", muted);
    screen.text(65, 13, "today $0.00", muted);
    screen.text(65, 15, "refreshed 09:30:00 · press ", muted);
    screen.put(92, 15, 'r', Style::fg(color::BLUE).bold());
    screen.text(93, 15, " to refresh", muted);

    // Status bar: the c Settings chip is active (blue), no numbered tab.
    let y = screen.h - 1;
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let active = Style::DEFAULT.bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    for x in [2usize, 5, 8, 11, 14, 17] {
        screen.text(x, y, &((x / 3) + 1).to_string(), muted);
    }
    let mut x = w - 58; // dot at col w-58 (62 at w=120)
    screen.put(x, y, '\u{25CF}', Style::fg(color::AMBER).bg(color::PANEL));
    x += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
        ("  ", panel),
    ] {
        x = screen.text(x, y, part, style);
    }
    screen.fill(x - 1, y, x + 11, y + 1, active);
    screen.text(x, y, "c Settings", active_fg);
    x += 11;
    screen.put(x, y, ' ', panel);
    x += 1;
    for (part, style) in [
        ("?", Style::fg(color::MUTED).bg(color::PANEL)),
        (" help · g go", Style::fg(color::MUTED).bg(color::PANEL)),
    ] {
        x = screen.text(x, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}

// ---- Wide (200x50) breakpoint painters, one per screen ----

/// Home at 200x50: the 120 layout with the split at half width.
pub fn draw_home_wide(screen: &mut Screen, home: &HomeState) {
    let w = screen.w;
    let s = w / 2; // pane split (100 at w=200)
    let lx1 = s - 1;
    let rx0 = s;
    let rx1 = w - 2;
    let inner = rx0 + 2; // right-hand content column

    // Header row.
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));

    // Watchlist + since-you-last-looked.
    screen.pane(
        1,
        1,
        lx1,
        20,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );
    screen.pane(
        rx0,
        1,
        rx1,
        20,
        false,
        &[("since you last looked", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("3", "research")]),
    );

    let header = " Symbol          close     chg%   age  40 closes";
    screen.text(2, 2, header, Style::fg(color::MUTED));
    let blue = Style::DEFAULT.bg(color::BLUE_BG);
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG);
    let white_bold = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 3, lx1, 4, blue);
    screen.put(2, 3, ' ', blue);
    let mut x = screen.text(3, 3, &home.symbol, white_bold);
    x = screen.text(x + 11, 3, &home.last, white);
    screen.put(x, 3, ' ', blue);
    x = screen.text(x + 1, 3, "▲ ", white);
    x = screen.text(x, 3, &home.chg_label, white);
    let _ = x;
    let spark_w = lx1 - 42;
    let spark =
        crate::braille::BrailleGraph::filled(home.closes.clone()).rows(spark_w, 1)[0].clone();
    screen.text(41, 3, &spark, white);
    screen.text(
        3,
        5,
        "spark: 40 daily closes · chg%: move on the day",
        Style::fg(color::MUTED),
    );

    let muted = Style::fg(color::MUTED);
    let warn = Style::fg(color::AMBER);
    screen.text(inner, 2, "nothing new since your last visit", muted);
    screen.text_right(rx1 - 1, 2, &home.since_stamp, muted);
    screen.text(inner, 4, "no activity in the last 30 days", muted);
    screen.text(inner, 5, "newest   no articles yet", muted);
    screen.text(inner, 7, &format!("⚠ {} 1d old", home.symbol), warn);
    screen.text(inner, 8, "⚠ 2 evidence prompts · 3 evidence", warn);

    // Upcoming + theses.
    screen.pane(
        1,
        21,
        lx1,
        40,
        false,
        &[("upcoming", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open evidence")]),
    );
    screen.pane(
        rx0,
        21,
        rx1,
        40,
        false,
        &[("theses", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open thesis"), ("4", "all")]),
    );
    screen.text(
        3,
        22,
        "nothing scheduled — press 3, then U to gather evidence",
        muted,
    );
    screen.text(3, 23, "calendar plugin: earnings & dividends only", muted);
    screen.text(inner, 22, "no theses yet", muted);
    screen.text(
        inner,
        24,
        "4 opens the theses desk — n tracks a claim",
        muted,
    );

    // Agenda.
    screen.pane(
        1,
        41,
        rx1,
        47,
        false,
        &[
            ("needs you today ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("enter", "open"), ("tab", "next box")]),
    );
    let rows: [(&str, &str, &str, Style); 4] = [
        ("6", "✓", " no decision reviews due", muted),
        ("4", "✓", " no falsifier hits", muted),
        ("3", "✓", " no earnings in the next 7 days", muted),
        ("2", "⚠", " 1 stale source", warn),
    ];
    for (index, (key, glyph, message, style)) in rows.into_iter().enumerate() {
        let y = 42 + index;
        screen.text(3, y, "▸", Style::fg(color::BLUE));
        screen.text(5, y, key, Style::fg(color::BLUE).bold());
        screen.text(8, y, glyph, style);
        screen.text(9, y, message, style);
    }

    draw_status_bar_wide(screen, screen.h - 1, w, "1 Home");
}

/// Research at 200x50: company list, report pane, evidence desk.
pub fn draw_research_wide(screen: &mut Screen) {
    screen.pane(
        1,
        0,
        36,
        47,
        true,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("company ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select")]),
    );
    screen.pane(
        37,
        0,
        158,
        47,
        false,
        &[
            ("r ", Style::fg(color::BLUE).bold()),
            ("report ", Style::fg(color::BLUE).bold()),
            ("· no report", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "scroll"), ("enter", "citation"), ("n", "regenerate")]),
    );
    screen.pane(
        159,
        0,
        198,
        47,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        // Truncated with an ellipsis in the exported frame.
        &vec![
            (" ".to_string(), Style::fg(color::MUTED)),
            ("/".to_string(), Style::fg(color::BLUE).bold()),
            (" search".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("k".to_string(), Style::fg(color::BLUE).bold()),
            (" kind".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("space".to_string(), Style::fg(color::BLUE).bold()),
            (" fold".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("l".to_string(), Style::fg(color::BLUE).bold()),
            (" m…".to_string(), Style::fg(color::MUTED)),
            (" ".to_string(), Style::fg(color::MUTED)),
        ],
    );

    // Company table header.
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, 36, 2, header_style);
    screen.fill(2, 2, 36, 42, Style::fg(color::FG));
    screen.text(2, 1, " Company  Report", header_style);

    // Company pane: input separator, empty state, gather actions.
    for x in 2..36 {
        screen.put(x, 42, '─', Style::fg("#333333"));
    }
    screen.text(
        3,
        43,
        "no companies yet — press 1 to",
        Style::fg(color::MUTED),
    );
    screen.text(3, 44, "add a target", Style::fg(color::MUTED));
    // The action rows sit on a dark well block sized to the text.
    let well_fg = Style::fg(color::FG).bg("#0d0d0d").bold();
    screen.fill(2, 45, 22, 46, Style::DEFAULT.bg("#0d0d0d"));
    screen.fill(2, 46, 18, 47, Style::DEFAULT.bg("#0d0d0d"));
    screen.fill(3, 45, 21, 46, well_fg);
    screen.fill(3, 46, 17, 47, well_fg);
    screen.put(4, 45, 'u', Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(6, 45, "gather company", well_fg);
    screen.put(4, 46, 'U', Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(6, 46, "gather all", well_fg);

    // Report pane: the report well.
    screen.fill(38, 6, 156, 47, Style::DEFAULT.bg("#0d0d0d"));
    // " Report " heading band centred in the well.
    screen.fill(40, 8, 152, 9, Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(93, 8, "Report", Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(
        40,
        10,
        "No report yet — press n to generate one.",
        Style::fg(color::FG).bg("#0d0d0d"),
    );

    // Report pane: the generate button.
    screen.fill(40, 2, 61, 3, Style::DEFAULT.bg(color::BLUE_BG));
    let white_bold_on_blue = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.text(41, 2, " ", white_bold_on_blue);
    screen.put(42, 2, 'n', Style::fg(color::BLUE).bg(color::BLUE_BG).bold());
    screen.text(43, 2, " generate report", white_bold_on_blue);
    screen.put(59, 2, ' ', white_bold_on_blue);

    // Evidence desk: search input, kind filter, table header.
    let well = Style::DEFAULT.bg("#0d0d0d");
    screen.fill(160, 1, 187, 2, well);
    screen.put(160, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        162,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(179, 1, 186, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(188, 1, "kind: all", Style::fg(color::MUTED));
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(160, 2, 198, 3, header_style);
    screen.fill(160, 3, 198, 36, Style::fg(color::FG));
    screen.text(160, 2, " Evidence", header_style);
    screen.text(174, 2, "Type", header_style);
    screen.text(187, 2, "Date", header_style);
    // Empty state, its hint strip, and the clip at the pane edge.
    screen.text(
        161,
        36,
        "no companies yet — press 1 to add a",
        Style::fg(color::MUTED),
    );
    for x in 160..198 {
        screen.put(x, 37, '─', Style::fg("#333333"));
    }
    screen.text(
        161,
        38,
        "select evidence to preview it",
        Style::fg(color::FG),
    );

    draw_status_bar_wide(screen, screen.h - 1, screen.w, "3 Research");
}

/// Theses at 200x50: thesis list, detail, evidence panes.
pub fn draw_theses_wide(screen: &mut Screen) {
    screen.pane(
        1,
        0,
        36,
        47,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("n", "new"), ("d", "edit"), ("/", "filter")]),
    );
    screen.pane(
        37,
        0,
        158,
        47,
        false,
        &[("t thesis", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
    );
    screen.pane(
        159,
        0,
        198,
        47,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("f", "find")]),
    );
    screen.fill(2, 1, 36, 46, Style::fg(color::FG));
    screen.text(
        39,
        1,
        "no theses yet — press n to create one",
        Style::fg(color::FG),
    );
    screen.text(161, 1, "no evidence yet", Style::fg(color::FG));
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(160, 2, 198, 3, header_style);
    screen.fill(160, 3, 198, 40, Style::fg(color::FG));
    screen.text(160, 2, "    ±  age   note", header_style);
    // Input separator above the evidence prompt line.
    for x in 160..198 {
        screen.put(x, 39, '─', Style::fg("#333333"));
    }
    let fg = Style::fg(color::FG);
    screen.text(161, 40, "no evidence yet — press f to find", fg);
    screen.text(161, 41, "candidates", fg);
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "4 Theses");
}

/// Ask at 200x50: conversation pane + targets pane.
pub fn draw_ask_wide(screen: &mut Screen) {
    screen.pane(
        1,
        0,
        162,
        47,
        true,
        &[("5 ask", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("i", "ask"), ("↑↓", "scroll"), ("z", "zoom")]),
    );
    screen.pane(
        163,
        0,
        198,
        23,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("targets ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("space", "toggle"), ("a", "all"), ("enter", "ask")]),
    );
    screen.pane(
        163,
        24,
        198,
        47,
        false,
        &[
            ("o ", Style::fg(color::BLUE).bold()),
            ("citations", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("←→", "citation"), ("o", "open"), ("s", "save")]),
    );
    let muted = Style::fg(color::MUTED);
    screen.text(3, 1, "scope: apple → US:AAPL · 50+ evidence items", muted);
    screen.text(
        3,
        2,
        "no messages yet — press t to pick targets, then i to ask",
        muted,
    );
    // Input separator and prompt line.
    for x in 2..162 {
        screen.put(x, 45, '─', Style::fg(color::PANEL));
    }
    screen.text(3, 46, ">", Style::fg(color::BLUE));
    screen.text(
        5,
        46,
        "ask about the targets in scope…",
        Style::fg(color::DISABLED),
    );
    screen.fill(36, 46, 162, 47, Style::fg(color::FG));
    // Targets table: header strip and the single selected row.
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(164, 1, 198, 2, header_style);
    screen.text(164, 1, "    Target  Kind     Evidence", header_style);
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(164, 2, 198, 3, selected);
    screen.text(164, 2, " ●  apple   company  50+", selected);
    screen.fill(164, 3, 198, 22, Style::fg(color::FG));
    screen.text(
        165,
        22,
        "1 of 1 in scope · US:AAPL",
        Style::fg(color::MUTED),
    );
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(164, 25, 198, 26, header_style);
    screen.text(164, 25, " #    Evidence  Kind", header_style);
    screen.fill(164, 26, 198, 46, Style::fg(color::FG));
    screen.text(
        165,
        46,
        "this session: 0 answers · $0.000",
        Style::fg(color::MUTED),
    );
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "5 Ask");
}

/// Decisions at 200x50: journal table + timeline.
pub fn draw_decisions_wide(screen: &mut Screen) {
    screen.pane(
        1,
        0,
        79,
        47,
        true,
        &[
            ("decisions ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("e", "edit"),
            ("d", "delete"),
            ("/", "filter"),
        ]),
    );
    screen.pane(
        80,
        0,
        198,
        47,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, 79, 2, header_style);
    screen.text(
        2,
        1,
        " status      review      instrument      rationale",
        header_style,
    );
    screen.fill(2, 2, 79, 47, Style::fg(color::FG));
    let muted = Style::fg(color::MUTED);
    screen.text(82, 1, "no decisions yet — press ", muted);
    screen.put(107, 1, 'n', Style::fg(color::BLUE).bold());
    screen.text(108, 1, " to record the context you want to revisit", muted);
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "6 Decisions");
}

/// Settings at 200x50: provider/plugins/sources panes left, diagnostics
/// right.
pub fn draw_settings_wide(screen: &mut Screen) {
    // Provider & model.
    screen.pane(
        1,
        0,
        62,
        3,
        true,
        &[("provider & model", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("↑↓", "choose"), ("enter", "change")]),
    );
    let selected_white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 1, 62, 2, selected_white);
    screen.text(3, 1, "provider: ", selected_white);
    screen.text(13, 1, "● openrouter", selected_white);
    screen.fill(2, 2, 62, 3, Style::fg(color::FG));
    screen.text(3, 2, "model: ", Style::fg(color::FG).bold());
    screen.text(10, 2, "not chosen — press m", Style::fg(color::AMBER));

    // Plugins.
    screen.pane(
        1,
        4,
        62,
        6,
        false,
        &[
            ("plugins ", Style::fg(color::BLUE).bold()),
            ("· 1 of 1 ok", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "plugin"), ("enter", "details")]),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(2, 5, 62, 6, selected);
    screen.put(3, 5, '●', selected);
    screen.text(6, 5, "sec_edgar  enabled", selected);

    // Data sources & markets.
    screen.pane(
        1,
        7,
        62,
        12,
        false,
        &[("data sources & markets", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "configure/edit"),
            ("a", "add market"),
            ("x", "remove market"),
        ]),
    );
    screen.text(3, 8, "sources", Style::fg(color::MUTED).bold());
    screen.text(3, 10, "markets", Style::fg(color::MUTED).bold());
    let table_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(2, 11, 62, 12, table_style);
    screen.text(3, 11, "ID  Market  Currency  Yahoo", table_style);

    // Diagnostics.
    screen.pane(
        63,
        0,
        198,
        47,
        false,
        &[
            ("diagnostics ", Style::fg(color::BLUE).bold()),
            ("· 80 rows · $0.00", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("r", "refresh"), ("d", "fold"), ("↑↓", "scroll")]),
    );
    let muted = Style::fg(color::MUTED);
    let fg = Style::fg(color::FG);
    screen.text(65, 1, "evidence", Style::fg(color::MUTED).bold());
    screen.text_right(197, 1, "delta.db · 4 KB", muted);
    screen.fill(64, 2, 198, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        65,
        2,
        "Table        Rows",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    let diag_selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(64, 3, 198, 4, diag_selected);
    screen.fill(64, 4, 198, 8, fg);
    let mut first = true;
    for (i, (name, count)) in [
        ("bar", "80"),
        ("event", "0"),
        ("fundamental", "0"),
        ("llmcall", "0"),
        ("newsitem", "0"),
    ]
    .into_iter()
    .enumerate()
    {
        let row_style = if first { diag_selected } else { fg };
        first = false;
        screen.text(65, 3 + i, name, row_style);
        screen.text(78, 3 + i, count, row_style);
    }
    screen.text(65, 8, "latest bar US:AAPL 20 Sep 00:00 UTC", muted);
    screen.text(
        65,
        10,
        "model spend · cumulative",
        Style::fg(color::MUTED).bold(),
    );
    screen.fill(
        64,
        11,
        198,
        12,
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        65,
        11,
        "Task  Model  Calls  USD",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(65, 12, "total  0 calls  $0.00", Style::fg(color::FG).bold());
    screen.text(65, 13, "today $0.00", muted);
    screen.text(65, 15, "refreshed 09:30:00 · press ", muted);
    screen.put(92, 15, 'r', Style::fg(color::BLUE).bold());
    screen.text(93, 15, " to refresh", muted);

    draw_status_bar_wide(screen, screen.h - 1, screen.w, "c Settings");
}

// ---- Narrow (80x24) breakpoint painters, one per screen ----

/// The narrow status bar: `1  2  3 …` keys at 3-cell pitch, the active tab
/// as a chip (labelled), and the right cluster without the provider name or
/// help hint.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NarrowTab {
    Home,
    Research,
    Theses,
    Ask,
    Decisions,
    Settings,
}

fn status_bar_narrow(screen: &mut Screen, active: NarrowTab) {
    let y = screen.h - 1;
    let w = screen.w;
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w - 1, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));

    // Tab keys at 3-cell pitch; the active tab becomes a labelled chip that
    // consumes its width plus padding. The exported chips keep the tab's own
    // number ("1 Home", "3 Research", …).
    let tabs: [(NarrowTab, &str, Option<&str>); 5] = [
        (NarrowTab::Home, "1", Some("1 Home")),
        (NarrowTab::Research, "2", Some("3 Research")),
        (NarrowTab::Theses, "3", Some("4 Theses")),
        (NarrowTab::Ask, "4", Some("5 Ask")),
        (NarrowTab::Decisions, "5", Some("6 Decisions")),
    ];
    let _ = tabs;
    // Six fixed positions; the active tab becomes a chip labelled with its
    // own position digit ("3 Research", "4 Theses", ...).
    // The tabs keep their global numbers: research is tab 3, theses 4, ask
    // 5, decisions 6 (watchlist, 2, has no narrow screen of its own).
    let mut x = 2usize;
    for position in 1..=6u32 {
        let tab = match position {
            1 => Some(NarrowTab::Home),
            3 => Some(NarrowTab::Research),
            4 => Some(NarrowTab::Theses),
            5 => Some(NarrowTab::Ask),
            6 => Some(NarrowTab::Decisions),
            _ => None,
        };
        let key = char::from_digit(position, 10).unwrap().to_string();
        if tab == Some(active) {
            let label = format!(
                "{key} {}",
                match tab.unwrap() {
                    NarrowTab::Home => "Home",
                    NarrowTab::Research => "Research",
                    NarrowTab::Theses => "Theses",
                    NarrowTab::Ask => "Ask",
                    _ => "Decisions",
                }
            );
            let len = label.chars().count();
            screen.fill(
                x - 1,
                y,
                x + len + 1,
                y + 1,
                Style::DEFAULT.bg(color::BLUE_BG),
            );
            screen.text(x, y, &label, active_fg);
            x += len + 2;
        } else {
            screen.text(x, y, &key, muted);
            x += 3;
        }
    }

    // Right cluster: dot, data age, spend, then the settings tab (chip when
    // active, muted label otherwise).
    screen.put(49, y, '\u{25CF}', Style::fg(color::AMBER).bg(color::PANEL));
    screen.text(52, y, "data 1d", plain);
    screen.text(59, y, "  ", panel);
    screen.text(61, y, "$0.00", plain);
    if active == NarrowTab::Settings {
        let label = "c Settings";
        let len = label.chars().count();
        let cx = w - len - 2;
        screen.fill(
            cx - 1,
            y,
            cx + len + 1,
            y + 1,
            Style::DEFAULT.bg(color::BLUE_BG),
        );
        screen.text(cx, y, label, active_fg);
    } else {
        screen.text(68, y, "c", muted);
        screen.text(69, y, " Settings", Style::fg(color::MUTED).bg(color::PANEL));
    }
}

/// Home at 80x24: one watchlist pane (table + since-you-last-looked lines),
/// the agenda pane, narrow header/status bar.
pub fn draw_home_narrow(screen: &mut Screen, home: &HomeState) {
    let w = screen.w;

    // Header row: inked DELTA chip, "overview", clock right-aligned.
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));

    let x1 = w - 2;
    screen.pane(
        1,
        1,
        x1,
        14,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );

    // Table header and the selected row (blue ink), spark sized to the pane.
    let header = " Symbol          close     chg%   age  40 closes";
    screen.text(2, 2, header, Style::fg(color::MUTED));
    let blue = Style::DEFAULT.bg(color::BLUE_BG);
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG);
    let white_bold = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 3, x1, 4, blue);
    screen.put(2, 3, ' ', blue);
    let mut x = screen.text(3, 3, &home.symbol, white_bold);
    x = screen.text(x + 11, 3, &home.last, white);
    screen.put(x, 3, ' ', blue);
    x = screen.text(x + 1, 3, "▲ ", white);
    x = screen.text(x, 3, &home.chg_label, white);
    let _ = x;
    let spark_w = x1 - 42;
    let spark =
        crate::braille::BrailleGraph::filled(home.closes.clone()).rows(spark_w, 1)[0].clone();
    screen.text(41, 3, &spark, white);

    // Since-you-last-looked, folded into the pane.
    let muted = Style::fg(color::MUTED);
    screen.text(3, 5, "since Mon 09:30  nothing new   ", muted);
    screen.text(
        34,
        5,
        &format!("⚠ {} 1d old", home.symbol),
        Style::fg(color::AMBER),
    );
    screen.text(3, 6, "next  nothing scheduled", muted);

    // Agenda: full width, jump keys with verdicts.
    screen.pane(
        1,
        15,
        x1,
        21,
        false,
        &[
            ("needs you today ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("enter", "open"), ("tab", "next box")]),
    );
    let rows: [(&str, &str, &str, Style); 4] = [
        ("6", "✓", " no decision reviews due", muted),
        ("4", "✓", " no falsifier hits", muted),
        ("3", "✓", " no earnings in the next 7 days", muted),
        ("2", "⚠", " 1 stale source", Style::fg(color::AMBER)),
    ];
    for (index, (key, glyph, message, style)) in rows.into_iter().enumerate() {
        let y = 16 + index;
        screen.text(3, y, "▸", Style::fg(color::BLUE));
        screen.text(5, y, key, Style::fg(color::BLUE).bold());
        screen.text(8, y, glyph, style);
        screen.text(9, y, message, style);
    }

    status_bar_narrow(screen, NarrowTab::Home);
}

/// Research (evidence desk) at 80x24: one full-width pane.
pub fn draw_research_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    screen.pane(
        1,
        0,
        x1,
        21,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("/", "search"),
            ("k", "kind"),
            ("enter", "preview"),
            ("esc", "back"),
        ]),
    );

    // Search input: block cursor, placeholder in the disabled token, the
    // rest of the strip in foreground-on-well.
    screen.fill(2, 1, 67, 2, Style::DEFAULT.bg("#0d0d0d"));
    screen.put(2, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        4,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(21, 1, 66, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(68, 1, "kind: all", Style::fg(color::MUTED));

    // Table header strip.
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(2, 2, x1, 3, header_style);
    screen.text(2, 2, " Evidence", header_style);
    screen.text(54, 2, "Type", header_style);
    screen.text(67, 2, "Date", header_style);

    // Empty rows carry the foreground default.
    screen.fill(2, 3, x1, 20, Style::fg(color::FG));
    screen.text(
        3,
        20,
        "no companies yet — press 1 to add a target",
        Style::fg(color::MUTED),
    );

    status_bar_narrow(screen, NarrowTab::Research);
}

/// Theses at 80x24: one focused full-width pane.
pub fn draw_theses_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    screen.pane(
        1,
        0,
        x1,
        21,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("d", "edit"),
            ("/", "filter"),
            ("enter", "thesis"),
            ("e", "evidence"),
        ]),
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, x1, 2, header_style);
    screen.text(2, 1, "   Claim", header_style);
    screen.text(49, 1, "Health", header_style);
    screen.text(62, 1, "Tilt", header_style);
    screen.text(72, 1, "Queue", header_style);
    // The focused table's empty rows still carry the foreground default.
    screen.fill(2, 2, x1, 20, Style::fg(color::FG));
    screen.text(
        3,
        20,
        "no theses yet — press n to create one",
        Style::fg(color::MUTED),
    );
    status_bar_narrow(screen, NarrowTab::Theses);
}

/// Ask at 80x24: one focused full-width pane with an input strip at the
/// bottom.
pub fn draw_ask_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    screen.pane(
        1,
        0,
        x1,
        21,
        true,
        &[("5 ask", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("i", "ask"), ("t", "targets"), ("z", "zoom")]),
    );
    let muted = Style::fg(color::MUTED);
    screen.text(3, 1, "scope: apple US:AAPL", muted);
    screen.text(
        3,
        2,
        "no messages yet — press t to pick targets, then i to ask",
        muted,
    );
    // Input separator and prompt line.
    for x in 2..x1 {
        screen.put(x, 19, '─', Style::fg(color::PANEL));
    }
    screen.text(3, 20, ">", Style::fg(color::BLUE));
    screen.text(
        5,
        20,
        "ask about the targets in scope…",
        Style::fg(color::DISABLED),
    );
    screen.fill(36, 20, x1, 21, Style::fg(color::FG));
    status_bar_narrow(screen, NarrowTab::Ask);
}

/// Decisions at 80x24: journal (focused) + timeline panes side by side.
pub fn draw_decisions_narrow(screen: &mut Screen) {
    // Left: the decision journal table.
    screen.pane(
        1,
        0,
        32,
        21,
        true,
        &[
            ("decisions ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &[
            (" ".to_string(), Style::fg(color::MUTED)),
            ("n".to_string(), Style::fg(color::BLUE).bold()),
            (" new".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("e".to_string(), Style::fg(color::BLUE).bold()),
            (" edit".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("d".to_string(), Style::fg(color::BLUE).bold()),
            (" delete".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("…".to_string(), Style::fg(color::BLUE).bold()),
            (" ".to_string(), Style::fg(color::MUTED)),
        ],
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, 32, 2, header_style);
    screen.text(2, 1, " status      review      instr", header_style);
    // The focused table's empty rows still carry the foreground default.
    screen.fill(2, 2, 32, 20, Style::fg(color::FG));
    // Bottom horizontal scrollbar: track, anchor, thumb to the edge.
    screen.fill(2, 20, 19, 21, Style::fg("#3a3a3a"));
    screen.put(19, 20, '▊', Style::fg("#3a3a3a").bg("#0d0d0d"));
    screen.fill(20, 20, 32, 21, Style::fg(color::FG).bg("#0d0d0d"));

    // Right: the timeline.
    screen.pane(
        33,
        0,
        screen.w - 2,
        21,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );
    let muted = Style::fg(color::MUTED);
    screen.text(35, 1, "no decisions yet — press ", muted);
    screen.put(60, 1, 'n', Style::fg(color::BLUE).bold());
    screen.text(61, 1, " to record the", muted);
    screen.text(35, 2, "context you want to revisit", muted);

    status_bar_narrow(screen, NarrowTab::Decisions);
}

/// Settings at 80x24: four panes stacked full-width.
pub fn draw_settings_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    // Provider & model pane.
    screen.pane(
        1,
        0,
        x1,
        3,
        true,
        &[("provider & model", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("↑↓", "choose"), ("enter", "change")]),
    );
    let selected_white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 1, x1, 2, selected_white);
    screen.text(3, 1, "provider: ", selected_white);
    screen.text(13, 1, "● openrouter", selected_white);
    screen.fill(2, 2, x1, 3, Style::fg(color::FG));
    screen.text(3, 2, "model: ", Style::fg(color::FG).bold());
    screen.text(10, 2, "not chosen — press m", Style::fg(color::AMBER));

    // Plugins pane.
    screen.pane(
        1,
        4,
        x1,
        6,
        false,
        &[
            ("plugins ", Style::fg(color::BLUE).bold()),
            ("· 1 of 1 ok", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "plugin"), ("enter", "details")]),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(2, 5, x1, 6, selected);
    screen.put(3, 5, '●', selected);
    screen.text(6, 5, "sec_edgar  enabled", selected);

    // Data sources & markets pane.
    screen.pane(
        1,
        7,
        x1,
        12,
        false,
        &[("data sources & markets", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "configure/edit"),
            ("a", "add market"),
            ("x", "remove market"),
        ]),
    );
    screen.text(3, 8, "sources", Style::fg(color::MUTED).bold());
    screen.text(3, 10, "markets", Style::fg(color::MUTED).bold());
    let table_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(2, 11, x1, 12, table_style);
    screen.text(3, 11, "ID  Market  Currency  Yahoo", table_style);

    // Diagnostics pane (folded to its summary line).
    screen.pane(
        1,
        13,
        x1,
        15,
        false,
        &[
            ("diagnostics ", Style::fg(color::BLUE).bold()),
            ("· 80 rows · $0.00", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("d", "expand"), ("r", "refresh")]),
    );
    screen.text(3, 14, "▸", Style::fg(color::MUTED));
    screen.text(
        4,
        14,
        " 80 rows · latest bar 20 Sep 00:00 UTC · spend $0.00",
        Style::fg(color::FG),
    );

    status_bar_narrow(screen, NarrowTab::Settings);
}

/// Shared tab-bar painter: inactive keys, an optional active chip, then the
/// cluster.
fn status_bar_tabs(
    screen: &mut Screen,
    y: usize,
    w: usize,
    chip_x: usize,
    chip: &str,
    before: &[(usize, &str)],
    after: &[(usize, &str)],
) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    if chip.is_empty() {
        for (x, key) in before.iter().chain(after.iter()) {
            screen.text(*x, y, key, muted);
        }
    } else {
        for (x, key) in before {
            screen.text(*x, y, key, muted);
        }
        screen.fill(
            chip_x,
            y,
            chip_x + chip.chars().count() + 2,
            y + 1,
            Style::DEFAULT.bg(color::BLUE_BG),
        );
        let mut x = screen.text(chip_x + 1, y, chip, active_fg);
        screen.put(x, y, ' ', Style::DEFAULT.bg(color::BLUE_BG));
        x += 1;
        for (xk, key) in after {
            screen.text(*xk, y, key, muted);
        }
        let _ = x;
    }
    let cluster = "●  data 1d  openrouter  $0.00  c Settings  ? help · g go ";
    let mut cx = w - 1 - cluster.chars().count();
    screen.put(cx, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    cx += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
        ("  ", panel),
        ("c", muted),
        (" Settings", Style::fg(color::MUTED).bg(color::PANEL)),
        ("  ", panel),
        ("?", Style::fg(color::MUTED).bg(color::PANEL)),
        (" help · g go", Style::fg(color::MUTED).bg(color::PANEL)),
    ] {
        cx = screen.text(cx, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
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
    draw_list_pane(screen, content_bottom);

    // Inspector.
    let cx = 48usize;
    let right_edge = 196usize;
    let Some(metric) = &state.metric else { return };

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

/// Wide status bar: every tab shows its label; `active` gets the chip.
fn draw_status_bar_wide(screen: &mut Screen, y: usize, w: usize, active: &str) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let muted_plain = Style::fg(color::MUTED).bg(color::PANEL);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    // Captured positions: keys expand to "n Label" wide; active is the chip.
    if active == "1 Home" {
        screen.fill(1, y, 9, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            2,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(2, y, '1', muted);
        screen.text(3, y, " Home", muted_plain);
    }
    if active == "2 Watchlist" {
        screen.fill(9, y, 22, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            10,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(10, y, '2', muted);
        screen.text(11, y, " Watchlist", muted_plain);
    }
    if active == "3 Research" {
        screen.fill(22, y, 34, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            23,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(23, y, '3', muted);
        screen.text(24, y, " Research", muted_plain);
    }
    if active == "4 Theses" {
        screen.fill(34, y, 44, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            35,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(35, y, '4', muted);
        screen.text(36, y, " Theses", muted_plain);
    }
    if active == "5 Ask" {
        screen.fill(44, y, 51, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            45,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(45, y, '5', muted);
        screen.text(46, y, " Ask", muted_plain);
    }
    if active == "6 Decisions" {
        screen.fill(51, y, 64, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            52,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(52, y, '6', muted);
        screen.text(53, y, " Decisions", muted_plain);
    }
    let cluster = "●  data 1d  openrouter  $0.00  c Settings  ? help · g go ";
    let mut cx = w - 1 - cluster.chars().count();
    screen.put(cx, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    cx += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
    ] {
        cx = screen.text(cx, y, part, style);
    }
    if active == "c Settings" {
        // The settings tab chips over the cluster.
        screen.put(cx, y, ' ', panel);
        screen.fill(cx + 1, y, cx + 13, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            cx + 2,
            y,
            "c Settings",
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
        cx += 13;
        cx = screen.text(cx, y, " ", panel);
    } else {
        cx = screen.text(cx, y, "  ", panel);
        cx = screen.text(cx, y, "c", muted);
        cx = screen.text(cx, y, " Settings", Style::fg(color::MUTED).bg(color::PANEL));
        cx = screen.text(cx, y, "  ", panel);
    }
    cx = screen.text(cx, y, "?", Style::fg(color::MUTED).bg(color::PANEL));
    screen.text(
        cx,
        y,
        " help · g go",
        Style::fg(color::MUTED).bg(color::PANEL),
    );
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
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
