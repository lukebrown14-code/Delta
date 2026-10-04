//! Live Watchlist inspector. Painting consumes cached metrics without I/O.
use crate::chart::PriceChart;
use crate::research::{truncate, wrap};
use crate::screen::{color, Screen, Style};
use chrono::NaiveDate;
use delta_core::models::Instrument;
use delta_services::asset_metrics::AssetMetrics;
use unicode_width::UnicodeWidthStr;

pub struct MetricsView<'a> {
    pub instrument: &'a Instrument,
    pub metrics: &'a AssetMetrics,
    pub range: &'a str,
    pub live_price: Option<f64>,
    pub scroll: usize,
}
impl MetricsView<'_> {
    pub fn paint(&self, screen: &mut Screen) {
        if screen.w == 0 || screen.h == 0 {
            return;
        }
        let width = screen.w.saturating_sub(2).max(1);
        let two_columns = width >= 54;
        let groups = if self.metrics.groups.is_empty() {
            vec![(
                "Available Metrics".to_string(),
                self.metrics
                    .values
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>(),
            )]
        } else {
            self.metrics.groups.clone()
        };
        let grid_rows: usize = groups
            .iter()
            .filter(|(_, rows)| !rows.is_empty())
            .map(|(_, rows)| {
                1 + if two_columns {
                    rows.len().div_ceil(2)
                } else {
                    rows.len()
                } + 1
            })
            .sum();
        let chart_height = (screen.h / 3).clamp(6, 16);
        let source = if let Some(error) = &self.metrics.error {
            format!("metrics unavailable: {error} — press enter to retry")
        } else {
            format!(
                "{} · live {} · history {}",
                self.metrics.source,
                if self.live_price.is_some() {
                    "●"
                } else {
                    "—"
                },
                friendly_dates(
                    self.metrics.history_start.as_deref(),
                    self.metrics.history_end.as_deref()
                )
            )
        };
        let source_lines = wrap(&source, width);
        let mut content = Screen::new(screen.w, chart_height + grid_rows + source_lines.len() + 10);
        let fg = Style::fg(color::FG);
        let muted = Style::fg(color::MUTED);
        let name = self
            .instrument
            .name
            .as_deref()
            .unwrap_or(&self.instrument.symbol);
        content.text(1, 0, &truncate(name, width), fg.bold());
        content.text(
            1,
            1,
            &truncate(
                &format!(
                    "{} · {} · {}",
                    self.instrument.symbol,
                    self.instrument.market.to_uppercase(),
                    self.metrics.profile
                ),
                width,
            ),
            muted,
        );
        let current = self
            .live_price
            .or_else(|| self.metrics.series.last().copied());
        let label = if self.metrics.profile == "bond" {
            "Current yield"
        } else {
            "Current price"
        };
        let value = current
            .map(|value| {
                format!(
                    "{}{}",
                    delta_core::format::grouped(value, 2),
                    if self.metrics.profile == "bond" {
                        "%"
                    } else {
                        ""
                    }
                )
            })
            .unwrap_or_else(|| {
                self.metrics
                    .values
                    .get(label)
                    .cloned()
                    .unwrap_or_else(|| "—".into())
            });
        let closed = self
            .metrics
            .history_end
            .as_deref()
            .map(|stamp| format!("closed · last {}", friendly_dates(Some(stamp), Some(stamp))))
            .unwrap_or_else(|| "— today".into());
        content.text(
            1,
            3,
            &truncate(
                &format!("{value} {}   {closed}", self.instrument.currency),
                width,
            ),
            fg.bold(),
        );
        let mut y = 4;
        if let (Some(low), Some(high), Some(&last)) = (
            self.metrics.week_52_low,
            self.metrics.week_52_high,
            self.metrics.series.last(),
        ) {
            if high > low {
                let fraction = (last - low) / (high - low);
                content.text(
                    1,
                    y,
                    &truncate(
                        &format!(
                            "52w {} {:.0}% of high",
                            fraction_bar(fraction),
                            fraction * 100.0
                        ),
                        width,
                    ),
                    Style::fg(color::BLUE),
                );
                y += 1;
            }
        }
        let mut x = 1;
        for (range, label) in [
            ("1d", "1D"),
            ("5d", "5D"),
            ("1m", "1M"),
            ("6m", "6M"),
            ("ytd", "YTD"),
            ("1y", "1Y"),
            ("all", "ALL"),
        ] {
            x = content.text(
                x,
                y,
                label,
                if range == self.range {
                    Style::fg(color::BLUE).bold()
                } else {
                    muted
                },
            );
            x = content.text(x, y, "  ", muted);
        }
        y += 1;
        let change = &self.metrics.change_label;
        let (arrow, tone) = if change.starts_with('+') {
            ("▲", color::GREEN)
        } else if change.starts_with('-') {
            ("▼", color::RED)
        } else {
            ("─", color::MUTED)
        };
        let mut summary = format!("{arrow} {change}");
        if let (Some(high), Some(low)) = (self.metrics.period_high, self.metrics.period_low) {
            summary.push_str(&format!(
                "   hi {}   lo {}",
                delta_core::format::grouped(high, 2),
                delta_core::format::grouped(low, 2)
            ));
        }
        content.text(1, y, &truncate(&summary, width), Style::fg(tone));
        y += 1;
        let (data, times) = chart_window(self.metrics, self.range);
        let chart = PriceChart { data, times };
        let formatter = |value: f64| {
            if self.metrics.profile == "bond" {
                format!("{value:.2}")
            } else {
                delta_core::format::grouped(value, 2)
            }
        };
        content.price_chart_runs(
            1,
            y,
            chart.runs_with_format(width, chart_height, formatter),
            tone,
        );
        y += chart_height + 1;
        for (title, rows) in groups {
            if rows.is_empty() {
                continue;
            }
            content.text(1, y, &truncate(&title, width), muted.bold());
            y += 1;
            if two_columns {
                let left_width = (width - 2) / 2;
                for pair in rows.chunks(2) {
                    metric_row(&mut content, 1, y, left_width, &pair[0]);
                    if let Some(right) = pair.get(1) {
                        metric_row(
                            &mut content,
                            left_width + 3,
                            y,
                            width - left_width - 2,
                            right,
                        );
                    }
                    y += 1;
                }
            } else {
                for row in rows {
                    metric_row(&mut content, 1, y, width, &row);
                    y += 1;
                }
            }
            y += 1;
        }
        for line in source_lines {
            content.text(1, y, &line, muted);
            y += 1;
        }
        let start = self.scroll.min(y.saturating_sub(screen.h));
        for row in 0..screen.h {
            for col in 0..screen.w {
                if let Some(cell) = content.cells.get((row + start) * content.w + col) {
                    screen.cells[row * screen.w + col] = cell.clone();
                }
            }
        }
    }
}
fn metric_row(screen: &mut Screen, x: usize, y: usize, width: usize, row: &(String, String)) {
    let value = truncate(&row.1, width);
    let value_width = UnicodeWidthStr::width(value.as_str());
    let label_width = width.saturating_sub(value_width + 1);
    screen.text(
        x,
        y,
        &truncate(&row.0, label_width),
        Style::fg(color::MUTED),
    );
    screen.text_right(x + width, y, &value, Style::fg(color::FG));
}
fn chart_window(metrics: &AssetMetrics, range: &str) -> (Vec<f64>, Vec<String>) {
    let limit = match range {
        "5d" => Some(5),
        "1m" => Some(30),
        "6m" => Some(130),
        "ytd" => Some(250),
        "1y" => Some(252),
        _ => None,
    };
    let start = limit
        .map(|limit| metrics.series.len().saturating_sub(limit))
        .unwrap_or(0);
    let times = if metrics.series_times.len() == metrics.series.len() {
        metrics.series_times[start..].to_vec()
    } else {
        vec![]
    };
    (metrics.series[start..].to_vec(), times)
}
fn fraction_bar(fraction: f64) -> String {
    let filled = fraction.clamp(0.0, 1.0) * 16.0;
    let mut whole = filled as usize;
    let mut bar = "█".repeat(whole);
    if whole < 16 {
        let eighth = ((filled - whole as f64) * 8.0) as usize;
        if eighth > 0 {
            bar.push(['▏', '▎', '▍', '▌', '▋', '▊', '▉'][eighth.min(7) - 1]);
            whole += 1;
        }
    }
    format!("▕{bar}{}▏", " ".repeat(16 - whole))
}
fn friendly_dates(start: Option<&str>, end: Option<&str>) -> String {
    let (Some(start), Some(end)) = (start, end) else {
        return "no historical dates".into();
    };
    let date = |text: &str| {
        text.get(..10)
            .and_then(|date| NaiveDate::parse_from_str(date, "%Y-%m-%d").ok())
    };
    let (Some(first), Some(last)) = (date(start), date(end)) else {
        return format!("{start} → {end}");
    };
    if first == last {
        first.format("%-d %b %Y").to_string()
    } else if first.format("%Y").to_string() == last.format("%Y").to_string() {
        format!("{} – {}", first.format("%-d %b"), last.format("%-d %b %Y"))
    } else {
        format!(
            "{} – {}",
            first.format("%-d %b %Y"),
            last.format("%-d %b %Y")
        )
    }
}
