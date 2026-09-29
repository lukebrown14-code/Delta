//! A braille price line with a right-hand Y gutter and an X date axis
//! (port of `delta/tui/widgets.py::PriceChart`).
//!
//! The same dot grid as [`crate::braille::BrailleGraph`] plus the axes a price
//! needs — Y ticks in a right gutter, a faint `┄` gridline on each tick row,
//! and up to three date labels on a `┬` rule. A `●` marks the last close and
//! carries its price in the gutter (K4).

use crate::axes::{default_y_format, nice_ticks, x_ticks};
use crate::braille::{braille_char, BrailleGraph, EMPTY};

/// The gutter is `2 + widest tick label` cells wide and hides before it would
/// squeeze the plot under 12 cells.
const MIN_PLOT: usize = 12;

/// What a run of text on one row is styled as.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunKind {
    Line,
    Marker,
    Grid,
    Axis,
    Blank,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub text: String,
    pub kind: RunKind,
}

#[derive(Debug, Clone, Default)]
pub struct PriceChart {
    pub data: Vec<f64>,
    /// ISO stamps aligned with `data`; may be empty (the rule row goes plain).
    pub times: Vec<String>,
}

impl PriceChart {
    pub fn new(data: Vec<f64>) -> Self {
        Self {
            data,
            times: Vec::new(),
        }
    }

    /// How many Y ticks fit the plot: at least 3, at most 5 (K2).
    fn tick_count(plot_rows: usize) -> usize {
        plot_rows.clamp(3, 5)
    }

    /// `-up` / `-down` / `-flat` over the displayed range (K5).
    pub fn direction(&self) -> Direction {
        if self.data.len() < 2 {
            return Direction::Flat;
        }
        let (first, last) = (self.data[0], self.data[self.data.len() - 1]);
        if last > first {
            Direction::Up
        } else if last < first {
            Direction::Down
        } else {
            Direction::Flat
        }
    }

    /// The chart as `height` strings of `width` columns.
    ///
    /// Unlike BrailleGraph (whose width is braille cells), width here is
    /// widget columns: the braille plot plus the gutter when shown.
    pub fn rows(&self, width: usize, height: usize) -> Vec<String> {
        self.runs(width, height)
            .iter()
            .map(|row| row.iter().map(|run| run.text.as_str()).collect())
            .collect()
    }

    /// Layout as per-line `(text, kind)` runs, ready to paint.
    ///
    /// Pure — no app state — so tests assert layout without a terminal. A
    /// gridline breaking around the line means a braille row can need two
    /// differently-styled runs, which a single-colour string cannot express.
    pub fn runs(&self, width: usize, height: usize) -> Vec<Vec<Run>> {
        if width < 1 || height < 1 {
            return Vec::new();
        }
        if height < 3 {
            // No room for the rule and label rows: a bare line, BrailleGraph's
            // own shape.
            if self.data.is_empty() {
                return vec![
                    vec![Run {
                        text: EMPTY.repeat(width),
                        kind: RunKind::Line
                    }];
                    height
                ];
            }
            let low = min_of(&self.data);
            let high = max_of(&self.data);
            let g = BrailleGraph {
                data: self.data.clone(),
                fill: false,
            };
            let span = (high - low).max(0.0);
            let span = if span == 0.0 { 1.0 } else { span };
            return g
                .cell_grid(width, height, low, span)
                .iter()
                .map(|row| {
                    vec![Run {
                        text: row.iter().map(|&c| braille_char(c)).collect(),
                        kind: RunKind::Line,
                    }]
                })
                .collect();
        }

        let plot_rows = height - 2;
        let dot_rows = plot_rows * 4;
        if self.data.is_empty() {
            // Nothing to scale: blank rows over a bare rule keep the pane's
            // height rhythm instead of inventing an axis for nothing.
            let mut runs = vec![
                vec![Run {
                    text: EMPTY.repeat(width),
                    kind: RunKind::Blank
                }];
                plot_rows
            ];
            runs.push(vec![Run {
                text: format!("└{}┘", "─".repeat(width.saturating_sub(2))),
                kind: RunKind::Axis,
            }]);
            runs.push(vec![Run {
                text: " ".repeat(width),
                kind: RunKind::Blank,
            }]);
            return runs;
        }

        let low = min_of(&self.data);
        let high = max_of(&self.data);
        let ticks = nice_ticks(low, high, Self::tick_count(plot_rows));
        // K2: scale to the outer ticks so every tick lands exactly on a row.
        let (plot_low, plot_high) = (ticks[0], ticks[ticks.len() - 1]);
        let span = (plot_high - plot_low).max(0.0);
        let span = if span == 0.0 { 1.0 } else { span };
        let labels: Vec<String> = ticks.iter().map(|&t| default_y_format(t)).collect();
        let gutter = 2 + labels.iter().map(String::len).max().unwrap_or(0);
        let mut plot = width.saturating_sub(gutter);
        let mut gutter = gutter;
        if plot < MIN_PLOT {
            // A gutter that starves the line hides entirely: the shape carries
            // the meaning, the labels are secondary.
            plot = width;
            gutter = 0;
        }
        let g = BrailleGraph {
            data: self.data.clone(),
            fill: false,
        };
        let cells = g.cell_grid(plot, plot_rows, plot_low, span);
        // One label per text row: a tick owns the gutter row its dot lands on.
        let mut tick_rows: std::collections::BTreeMap<usize, &str> =
            std::collections::BTreeMap::new();
        for (tick, label) in ticks.iter().zip(labels.iter()) {
            let scaled = (tick - plot_low) / span * (dot_rows as f64 - 1.0);
            let row = ((dot_rows as f64 - 1.0 - crate::braille::py_round(scaled)) as usize) / 4;
            tick_rows.insert(row.min(plot_rows - 1), label.as_str());
        }
        // The last close marks its gutter row with a bullet and price (K4).
        let last = self.data[self.data.len() - 1];
        let scaled = (last - plot_low) / span * (dot_rows as f64 - 1.0);
        let marker_row = (((dot_rows as f64 - 1.0 - crate::braille::py_round(scaled)) as i64) / 4)
            .clamp(0, plot_rows as i64 - 1) as usize;
        let marker_label = default_y_format(last);

        let mut runs: Vec<Vec<Run>> = Vec::new();
        for (index, row_cells) in cells.iter().enumerate().take(plot_rows) {
            let on_tick = tick_rows.contains_key(&index);
            let mut row_runs: Vec<Run> = Vec::new();
            let mut parts = String::new();
            let mut kind = RunKind::Blank;
            let mut have_kind = false;
            for &cell in row_cells {
                let (text, cell_kind) = if cell != 0 {
                    (braille_char(cell).to_string(), RunKind::Line)
                } else if on_tick {
                    ("┄".to_string(), RunKind::Grid)
                } else {
                    (EMPTY.to_string(), RunKind::Blank)
                };
                if have_kind && cell_kind == kind {
                    parts.push_str(&text);
                    continue;
                }
                if have_kind && !parts.is_empty() {
                    row_runs.push(Run {
                        text: std::mem::take(&mut parts),
                        kind,
                    });
                }
                parts = text;
                kind = cell_kind;
                have_kind = true;
            }
            if have_kind && !parts.is_empty() {
                row_runs.push(Run { text: parts, kind });
            }
            if gutter > 0 {
                if index == marker_row {
                    row_runs.push(Run {
                        text: format!("● {}", rjust(&marker_label, gutter - 2)),
                        kind: RunKind::Marker,
                    });
                } else {
                    let label = tick_rows.get(&index);
                    row_runs.push(match label {
                        // K3: ├ ticks, not ┤
                        Some(label) => Run {
                            text: format!("├ {}", rjust(label, gutter - 2)),
                            kind: RunKind::Axis,
                        },
                        None => Run {
                            text: format!("│{}", " ".repeat(gutter - 1)),
                            kind: RunKind::Grid,
                        },
                    });
                }
            }
            runs.push(row_runs);
        }

        // Rule and label rows. Without the gutter the rule shrinks to fit the
        // widget; the line stays full width.
        let time_refs: Vec<&str> = self.times.iter().map(String::as_str).collect();
        let xaxis = x_ticks(&time_refs, plot);
        let rule_cells = if gutter > 0 {
            plot
        } else {
            width.saturating_sub(2)
        };
        let mut rule: Vec<&str> = vec!["─"; rule_cells];
        for &(column, _) in &xaxis {
            if column < rule_cells {
                rule[column] = "┬";
            }
        }
        // K3: with a gutter the ┘ meets the Y gutter │ exactly under column 0;
        // without one the └ roots an otherwise free-standing rule.
        let rule_row = if gutter > 0 {
            format!("{}┘", rule.concat())
        } else {
            format!("└{}┘", rule.concat())
        };
        runs.push(vec![
            Run {
                text: rule_row.clone(),
                kind: RunKind::Axis,
            },
            Run {
                text: " ".repeat(width.saturating_sub(rule_row.chars().count())),
                kind: RunKind::Blank,
            },
        ]);

        let mut canvas: Vec<char> = vec![' '; plot];
        let mut cursor = 0usize;
        for &(column, ref label) in &xaxis {
            // Centre each label on its tick, clamped inside the plot and past
            // the label before it; x_ticks spaces ticks so this rarely bites.
            let start = (column.saturating_sub(label.len() / 2))
                .min(plot.saturating_sub(label.len()))
                .max(cursor);
            let end = (start + label.len()).min(plot);
            for (i, ch) in label.chars().take(end - start).enumerate() {
                canvas[start + i] = ch;
            }
            cursor = start + label.len();
        }
        let canvas_line: String = canvas.into_iter().collect();
        runs.push(vec![Run {
            text: format!("{}{}", canvas_line, " ".repeat(width.saturating_sub(plot))),
            kind: RunKind::Axis,
        }]);
        runs
    }
}

/// `-up` / `-down` / `-flat` over the displayed range (K5).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Flat,
}

impl Direction {
    /// The token the line colour takes (K5): the direction colour instead of
    /// the neutral primary, so a glance reads the range without the figures.
    pub fn token(self) -> &'static str {
        match self {
            Direction::Up => "text-success",
            Direction::Down => "text-error",
            Direction::Flat => "text-accent",
        }
    }
}

/// Python `str.rjust(width)`.
fn rjust(s: &str, width: usize) -> String {
    if s.len() >= width {
        s.to_string()
    } else {
        format!("{}{s}", " ".repeat(width - s.len()))
    }
}

fn min_of(data: &[f64]) -> f64 {
    data.iter().cloned().fold(f64::INFINITY, f64::min)
}

fn max_of(data: &[f64]) -> f64 {
    data.iter().cloned().fold(f64::NEG_INFINITY, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data() -> Vec<f64> {
        vec![10.0, 11.0, 10.5, 12.0, 11.5, 13.0, 12.5]
    }

    fn times() -> Vec<String> {
        vec![
            "2026-01-02T21:00:00".to_string(),
            "2026-01-10T21:00:00".to_string(),
            "2026-01-20T21:00:00".to_string(),
        ]
    }

    fn texts(row: &[Run], kind: RunKind) -> Vec<&str> {
        row.iter()
            .filter(|r| r.kind == kind)
            .map(|r| r.text.as_str())
            .collect()
    }

    #[test]
    fn direction_matches_python() {
        assert_eq!(PriceChart::new(vec![1.0, 2.0]).direction(), Direction::Up);
        assert_eq!(PriceChart::new(vec![2.0, 1.0]).direction(), Direction::Down);
        assert_eq!(PriceChart::new(vec![2.0, 2.0]).direction(), Direction::Flat);
        assert_eq!(PriceChart::new(vec![2.0]).direction(), Direction::Flat);
    }

    #[test]
    fn chart_runs_match_python_golden() {
        let mut chart = PriceChart::new(data());
        chart.times = times();
        let runs = chart.runs(40, 8);
        assert_eq!(runs.len(), 8);
        // Row 0: top gridline, the line, then the marker in the gutter.
        assert_eq!(texts(&runs[0], RunKind::Grid)[0], "┄".repeat(25));
        assert_eq!(
            texts(&runs[0], RunKind::Line)[0],
            "\u{28c0}\u{2814}\u{2809}\u{2811}\u{2812}\u{2824}\u{2824}\u{28c0}"
        );
        assert_eq!(texts(&runs[0], RunKind::Marker)[0], "● 12.50");
        // Tick rows carry ├ labels in the gutter (K3).
        assert_eq!(texts(&runs[1], RunKind::Axis)[0], "├ 12.00");
        assert_eq!(texts(&runs[2], RunKind::Axis)[0], "├ 11.00");
        assert_eq!(texts(&runs[4], RunKind::Axis)[0], "├ 10.00");
        assert_eq!(texts(&runs[5], RunKind::Axis)[0], "├  9.00");
        // Rule row with three ┬ ticks and the ┘ meeting the gutter.
        assert_eq!(
            texts(&runs[6], RunKind::Axis)[0],
            "┬───────────────┬───────────────┬┘"
        );
        // Label row, centred on its ticks.
        assert_eq!(
            texts(&runs[7], RunKind::Axis)[0],
            "02 Jan       10 Jan        20 Jan       "
        );
    }

    #[test]
    fn empty_chart_keeps_height_rhythm() {
        let chart = PriceChart::new(vec![]);
        let runs = chart.runs(40, 8);
        assert_eq!(runs.len(), 8);
        assert!(runs[0].iter().all(|r| r.kind == RunKind::Blank));
        assert_eq!(
            texts(&runs[6], RunKind::Axis)[0],
            format!("└{}┘", "─".repeat(38))
        );
    }

    #[test]
    fn narrow_chart_hides_gutter() {
        let mut chart = PriceChart::new(data());
        chart.times = times();
        let runs = chart.runs(18, 8);
        // No gutter: no marker/├ runs anywhere.
        assert!(runs
            .iter()
            .all(|row| texts(row, RunKind::Marker).is_empty()));
        assert!(runs.iter().all(|row| texts(row, RunKind::Axis)
            .iter()
            .all(|t| !t.starts_with("├"))));
        // The rule roots with └ instead of meeting a gutter (fresh Python
        // golden for `_runs(18, 8)`).
        assert_eq!(texts(&runs[6], RunKind::Axis)[0], "└┬───────┬───────┘");
        assert_eq!(texts(&runs[7], RunKind::Axis)[0], "02 Jan10 Jan20 Jan");
    }

    #[test]
    fn tiny_height_degrades_to_bare_line() {
        let chart = PriceChart::new(data());
        let runs = chart.runs(10, 1);
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].len(), 1);
        assert_eq!(runs[0][0].kind, RunKind::Line);
    }
}
