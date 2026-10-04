//! Cell-grid screen model for golden parity (R3 oracle).
//!
//! The harness compares this model cell-for-cell against
//! `fixtures/golden_screens/*.json` (Tier A: character, fg, bg, attrs). Styles
//! come only from [`crate::theme::Theme`] tokens, as resolved by the exporter.

use crate::chart::{PriceChart, RunKind};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Resolved theme hexes (see `theme.rs`); short aliases for the painter.
pub mod color {
    pub const BLUE: &str = "#5b8def"; // text-primary
    pub const FG: &str = "#d4d4d4"; // foreground
    pub const MUTED: &str = "#8a8a8a"; // text-muted
    pub const DISABLED: &str = "#5c5c5c"; // text-disabled
    pub const GREEN: &str = "#22c55e"; // text-success
    pub const RED: &str = "#f87171"; // text-error
    pub const AMBER: &str = "#f59e0b"; // text-warning
    pub const WHITE: &str = "#ffffff";
    pub const BLUE_BG: &str = "#264b96"; // primary (ink)
    pub const PANEL: &str = "#1a1a1a";
    pub const BLACK: &str = "#000000";
    pub const BORDER_BLURRED: &str = "#333333";
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<&'static str>,
    pub bg: Option<&'static str>,
    pub bold: bool,
    pub reverse: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Style {
    pub const DEFAULT: Style = Style {
        fg: None,
        bg: None,
        bold: false,
        reverse: false,
        italic: false,
        underline: false,
    };
    pub const fn fg(color: &'static str) -> Style {
        Style {
            fg: Some(color),
            bg: None,
            bold: false,
            reverse: false,
            italic: false,
            underline: false,
        }
    }
    pub const fn bold(mut self) -> Style {
        self.bold = true;
        self
    }
    pub const fn bg(mut self, color: &'static str) -> Style {
        self.bg = Some(color);
        self
    }
    pub const fn attrs_reverse(mut self) -> Style {
        self.reverse = true;
        self
    }
    pub const fn italic(mut self) -> Style {
        self.italic = true;
        self
    }
    pub const fn underline(mut self) -> Style {
        self.underline = true;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub symbol: String,
    pub fg: Option<&'static str>,
    pub bg: Option<&'static str>,
    pub bold: bool,
    pub reverse: bool,
    pub italic: bool,
    pub underline: bool,
}

/// A painted screen: `w x h` cells.
#[derive(Debug, Clone)]
pub struct Screen {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Cell>,
}

impl Screen {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            cells: vec![
                Cell {
                    ch: ' ',
                    symbol: " ".into(),
                    fg: None,
                    bg: Some(color::BLACK),
                    bold: false,
                    reverse: false,
                    italic: false,
                    underline: false,
                };
                w * h
            ],
        }
    }

    pub fn put(&mut self, x: usize, y: usize, ch: char, style: Style) {
        if x < self.w && y < self.h {
            let cell = &mut self.cells[y * self.w + x];
            cell.ch = ch;
            cell.symbol = ch.to_string();
            cell.fg = style.fg;
            cell.bg = style.bg.or(cell.bg);
            cell.bold = style.bold;
            cell.reverse = style.reverse;
            cell.italic = style.italic;
            cell.underline = style.underline;
        }
    }

    /// Paint `text` left to right from (x, y); returns the next x.
    pub fn text(&mut self, x: usize, y: usize, text: &str, style: Style) -> usize {
        let mut x = x;
        let mut previous: Option<usize> = None;
        for ch in text.chars() {
            let width = ch.width().unwrap_or(0);
            if width == 0 {
                if let Some(index) = previous {
                    self.cells[index].symbol.push(ch);
                }
                continue;
            }
            if x + width > self.w {
                break;
            }
            self.put(x, y, ch, style);
            if y < self.h {
                previous = Some(y * self.w + x);
            }
            for offset in 1..width {
                self.put(x + offset, y, ' ', style);
            }
            x += width;
        }
        x
    }

    /// Paint `text` right-aligned so it ends at `end` (exclusive).
    pub fn text_right(&mut self, end: usize, y: usize, text: &str, style: Style) {
        let start = end.saturating_sub(text.width());
        self.text(start, y, text, style);
    }

    /// Copy a child view into a pane without changing its sibling regions.
    pub fn blit_at(&mut self, child: &Screen, x: usize, y: usize) {
        for row in 0..child.h.min(self.h.saturating_sub(y)) {
            for col in 0..child.w.min(self.w.saturating_sub(x)) {
                self.cells[(y + row) * self.w + x + col] = child.cells[row * child.w + col].clone();
            }
        }
    }

    /// Dim the whole screen: Textual's modal backdrop blends every resolved
    /// colour toward black at 60% — per channel, `int(v * 0.4)` (verified
    /// against the exporter: `#5b8def` -> `#24385f`, `#d4d4d4` -> `#545454`,
    /// `#22c55e` -> `#0d4e25`).
    pub fn dim(&mut self) {
        fn dim_hex(hex: &str) -> String {
            let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
            let r = (byte(1) as f32 * 0.4) as u8;
            let g = (byte(3) as f32 * 0.4) as u8;
            let b = (byte(5) as f32 * 0.4) as u8;
            format!("#{r:02x}{g:02x}{b:02x}")
        }
        // The common dimmed values as 'static strings; anything else keeps its
        // undimmed hex (never hit by the golden screens).
        fn dim(c: Option<&'static str>) -> Option<&'static str> {
            match c {
                Some(color::BLUE) => Some("#24385f"),
                Some(color::FG) => Some("#545454"),
                Some(color::MUTED) => Some("#373737"),
                Some(color::DISABLED) => Some("#242424"),
                Some(color::GREEN) => Some("#0d4e25"),
                Some(color::RED) => Some("#632d2d"),
                Some(color::AMBER) => Some("#623f04"),
                Some(color::WHITE) => Some("#666666"),
                Some(color::BLUE_BG) => Some("#0f1e3c"),
                Some(color::PANEL) => Some("#0a0a0a"),
                Some(color::BLACK) => Some("#000000"),
                Some(color::BORDER_BLURRED) => Some("#141414"),
                other => other,
            }
        }
        let _ = dim_hex;
        for cell in &mut self.cells {
            // A cell with no explicit fg inherits the modal screen's default
            // (foreground) *after* dimming, so it is not dimmed itself.
            cell.fg = match cell.fg {
                None => Some(color::FG),
                Some(fg) => dim(Some(fg)),
            };
            cell.bg = dim(cell.bg);
        }
    }

    pub fn fill(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, style: Style) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.put(x, y, ' ', style);
            }
        }
    }

    /// A Textual `Pane`: bordered box with a centred-ish title and a hint bar.
    ///
    /// Focused panes use the heavy border in `$border`; unfocused ones the
    /// thin border in `$border-blurred`. Title and hints carry their runs.
    #[allow(clippy::too_many_arguments)]
    pub fn pane(
        &mut self,
        x0: usize,
        y0: usize,
        x1: usize,
        y1: usize,
        focused: bool,
        title_parts: &[(&str, Style)],
        hint_parts: &[(String, Style)],
    ) {
        let edge = if focused {
            color::BLUE
        } else {
            color::BORDER_BLURRED
        };
        let border = Style::fg(edge);
        let (corner_tl, corner_tr, corner_bl, corner_br, top, bottom, side) = if focused {
            ('┏', '┓', '┗', '┛', '━', '━', '┃')
        } else {
            ('┌', '┐', '└', '┘', '─', '─', '│')
        };
        self.put(x0, y0, corner_tl, border);
        self.put(x1, y0, corner_tr, border);
        self.put(x0, y1, corner_bl, border);
        self.put(x1, y1, corner_br, border);
        for x in x0 + 1..x1 {
            self.put(x, y0, top, border);
            self.put(x, y1, bottom, border);
        }
        for y in y0 + 1..y1 {
            self.put(x0, y, side, border);
            self.put(x1, y, side, border);
        }
        // Title: " {parts joined} " starting two cells after the corner
        // (the corner is followed by one plain fill glyph).
        let mut x = x0 + 2;
        self.put(x, y0, ' ', Style::fg(color::BLUE).bold());
        x += 1;
        for (text, style) in title_parts {
            x = self.text(x, y0, text, *style);
        }
        self.put(x, y0, ' ', Style::fg(color::BLUE).bold());
        // Hints sit on the bottom edge.
        let mut x = x0 + 1;
        self.put(x, y1, bottom, border);
        x += 1;
        for (text, style) in hint_parts {
            x = self.text(x, y1, text, *style);
        }
    }

    /// Paint [`PriceChart`] runs with the inspector's colour mapping:
    /// line/marker take the direction colour, grid `$text-disabled`,
    /// axis `$text-muted`, and blank cells the foreground (matching the
    /// exporter's resolved default style).
    pub fn price_chart(
        &mut self,
        x: usize,
        y: usize,
        chart: &PriceChart,
        w: usize,
        h: usize,
        line: &'static str,
    ) {
        self.price_chart_runs(x, y, chart.runs(w, h), line);
    }

    pub fn price_chart_runs(
        &mut self,
        x: usize,
        y: usize,
        runs: Vec<Vec<crate::chart::Run>>,
        line_color: &'static str,
    ) {
        for (row_index, row) in runs.into_iter().enumerate() {
            let mut cx = x;
            for run in row {
                let style = match run.kind {
                    RunKind::Line | RunKind::Marker => Style::fg(line_color),
                    RunKind::Grid => Style::fg(color::DISABLED),
                    RunKind::Axis => Style::fg(color::MUTED),
                    RunKind::Blank => Style::fg(color::FG),
                };
                cx = self.text(cx, y + row_index, &run.text, style);
            }
        }
    }
}
