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
    draw_watchlist(screen, state);
    screen.dim();

    let (x0, y0, x1, y1) = (24usize, 2usize, 95usize, 37usize);
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

    // Title, centred: 23 left / 24 right pad in bold blue.
    let title = "what these metrics mean";
    screen.text(27, 4, &" ".repeat(21), Style::fg(color::BLUE).bold());
    screen.text(48, 4, title, Style::fg(color::BLUE).bold());
    screen.text(71, 4, &" ".repeat(22), Style::fg(color::BLUE).bold());

    // Body: 22 visible lines from the top. Group lines indent 3 (blue bold,
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
    for (index, (indent, text, style)) in lines.iter().take(21).enumerate() {
        // The indent spaces carry the widget's default style, not the run's.
        screen.text(
            25,
            7 + index,
            &" ".repeat(*indent),
            Style::DEFAULT.bg("#0d0d0d"),
        );
        screen.text(25 + indent, 7 + index, text, *style);
    }

    // Footer hint, centred: " esc close " with the key bold blue.
    screen.text(27, 29, &" ".repeat(28), Style::fg(color::MUTED));
    let mut x = screen.text(55, 29, "esc", Style::fg(color::BLUE).bold());
    x = screen.text(x, 29, " close", Style::fg(color::MUTED));
    screen.text(x, 29, &" ".repeat(29), Style::fg(color::MUTED));
}

/// Home state the painter renders (the seeded golden scenario's values).
pub struct HomeState {
    pub clock: String,
    pub symbol: String,
    pub last: String,
    pub chg_label: String,
    pub spark: String,
    pub since_stamp: String,
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
