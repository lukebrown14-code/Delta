//! Cell-grid screen model for golden parity (R3 oracle).
//!
//! The harness compares this model cell-for-cell against
//! `fixtures/golden_screens/*.json` (Tier A: character, fg, bg, attrs). Styles
//! come only from [`crate::theme::Theme`] tokens, as resolved by the exporter.

use crate::chart::{PriceChart, RunKind};
use crate::theme::Palette;
use ratatui::{buffer::Buffer, layout::Rect, style::Modifier};
use std::collections::HashMap;

/// Resolved theme hexes (see `theme.rs`); short aliases for the painter.
/// Painters write these dark hexes; `Screen` remaps them per palette at
/// write time, so a painter never branches on the theme.
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
    // Shell-modal chrome the painters share (resolved by the exporter).
    pub const SURFACE: &str = "#0d0d0d";
    pub const BACKGROUND: &str = "#000000";
    pub const TAB_INACTIVE_FG: &str = "#707070"; // Tabs inactive label
    pub const TAB_UNDERLINE: &str = "#484848"; // Tabs inactive underline
    pub const SCRIM_BG: &str = "#060606"; // command palette scrim
    pub const SCRIM_FG: &str = "#030303"; // palette top border
    pub const HIT_DESC: &str = "#8d8d8d"; // palette hit description
    pub const HIT_DESC_SELECTED: &str = "#9399a6"; // …on the highlighted hit
    pub const HIT_SELECTED_BG: &str = "#16284e"; // $block-cursor-blurred over the scrim
    pub const SURFACE_SCROLLBAR: &str = "#3a3a3a"; // $scrollbar
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: Option<&'static str>,
    pub bg: Option<&'static str>,
    pub bold: bool,
    pub reverse: bool,
}

impl Style {
    pub const DEFAULT: Style = Style {
        fg: None,
        bg: None,
        bold: false,
        reverse: false,
    };
    pub const fn fg(color: &'static str) -> Style {
        Style {
            fg: Some(color),
            bg: None,
            bold: false,
            reverse: false,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub fg: Option<&'static str>,
    pub bg: Option<&'static str>,
    pub bold: bool,
}

/// The 60% backdrop blend of every token hex either palette can place on a
/// screen (`int(v * 0.4)` per channel, precomputed so `dim` stays
/// allocation-free). Unknown hexes pass through undimmed — never hit by the
/// golden screens.
fn dim_hex(hex: &'static str) -> &'static str {
    match hex {
        // delta-dark tokens.
        color::BLUE => "#24385f",
        color::FG => "#545454",
        color::MUTED => "#373737",
        color::DISABLED => "#242424",
        color::GREEN => "#0d4e25",
        color::RED => "#632d2d",
        color::AMBER => "#623f04",
        color::WHITE => "#666666",
        color::BLUE_BG => "#0f1e3c",
        color::PANEL => "#0a0a0a",
        color::BLACK => "#000000",
        color::BORDER_BLURRED => "#141414",
        color::SURFACE => "#050505",
        color::TAB_INACTIVE_FG => "#2c2c2c",
        color::TAB_UNDERLINE => "#1c1c1c",
        color::SCRIM_BG => "#020202",
        color::SCRIM_FG => "#010101",
        color::HIT_DESC => "#383838",
        color::HIT_DESC_SELECTED => "#3a3d42",
        color::HIT_SELECTED_BG => "#08101f",
        "#232323" => "#0e0e0e",
        "#13254b" => "#070e1e",
        // delta-light tokens (palette.remap outputs).
        "#f4f4ef" => "#61615f",
        "#e9e9e1" => "#5d5d5a",
        "#dddbcf" => "#585752",
        "#22252a" => "#0d0e10",
        "#6b6e74" => "#2a2c2e",
        "#9a9da3" => "#3d3e41",
        "#095261" => "#032026",
        "#295e1c" => "#10250b",
        "#7e251c" => "#320e0b",
        "#5e4500" => "#251b00",
        "#dfeef0" => "#595f60",
        "#0f7d93" => "#06323a",
        "#3f8f2b" => "#193911",
        "#8e6a00" => "#382a00",
        "#bf392b" => "#4c1611",
        "#0f6d80" => "#062b33",
        "#8a5a00" => "#372400",
        "#7d3fa8" => "#321943",
        "#0a6b52" => "#042a20",
        "#5a6474" => "#24282e",
        other => other,
    }
}

/// A painted screen: `w x h` cells. Carries the active [`Palette`]: cells
/// store the *resolved* hex of that palette (painters write dark tokens and
/// `put` remaps), so the golden diff compares against the right exporter
/// output whatever the theme.
#[derive(Debug, Clone)]
pub struct Screen {
    pub w: usize,
    pub h: usize,
    pub cells: Vec<Cell>,
    pub palette: Palette,
}

/// The drawing operations shared by the oracle surface and a live Ratatui
/// buffer. Coordinates are relative to the supplied area.
pub trait PaintCanvas {
    fn width(&self) -> usize;
    fn height(&self) -> usize;
    fn palette(&self) -> Palette;
    fn put(&mut self, x: usize, y: usize, ch: char, style: Style);
    fn read(&self, index: usize) -> Cell;
    fn write(&mut self, index: usize, cell: Cell);

    fn edit(&mut self, index: usize, edit: impl FnOnce(&mut Cell)) {
        let mut cell = self.read(index);
        edit(&mut cell);
        self.write(index, cell);
    }

    fn text(&mut self, x: usize, y: usize, text: &str, style: Style) -> usize {
        let mut x = x;
        for ch in text.chars() {
            self.put(x, y, ch, style);
            x += 1;
        }
        x
    }

    fn text_right(&mut self, end: usize, y: usize, text: &str, style: Style) {
        self.text(end.saturating_sub(text.chars().count()), y, text, style);
    }

    fn fill(&mut self, x0: usize, y0: usize, x1: usize, y1: usize, style: Style) {
        for y in y0..y1 {
            for x in x0..x1 {
                self.put(x, y, ' ', style);
            }
        }
    }

    fn resolve_modal_base(&mut self) {
        let foreground = self.palette().foreground();
        for index in 0..self.width() * self.height() {
            self.edit(index, |cell| {
                if cell.fg.is_none() {
                    cell.fg = Some(foreground);
                }
            });
        }
    }

    fn dim(&mut self) {
        let foreground = self.palette().foreground();
        for index in 0..self.width() * self.height() {
            self.edit(index, |cell| {
                cell.fg = Some(cell.fg.map_or(foreground, dim_hex));
                cell.bg = cell.bg.map(dim_hex);
            });
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn pane(
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
        let (tl, tr, bl, br, horizontal, vertical) = if focused {
            ('┏', '┓', '┗', '┛', '━', '┃')
        } else {
            ('┌', '┐', '└', '┘', '─', '│')
        };
        for (x, y, ch) in [(x0, y0, tl), (x1, y0, tr), (x0, y1, bl), (x1, y1, br)] {
            self.put(x, y, ch, border);
        }
        for x in x0 + 1..x1 {
            self.put(x, y0, horizontal, border);
            self.put(x, y1, horizontal, border);
        }
        for y in y0 + 1..y1 {
            self.put(x0, y, vertical, border);
            self.put(x1, y, vertical, border);
        }
        let mut x = x0 + 2;
        self.put(x, y0, ' ', Style::fg(color::BLUE).bold());
        x += 1;
        for (label, style) in title_parts {
            x = self.text(x, y0, label, *style);
        }
        self.put(x, y0, ' ', Style::fg(color::BLUE).bold());
        let mut x = x0 + 1;
        self.put(x, y1, horizontal, border);
        x += 1;
        for (label, style) in hint_parts {
            x = self.text(x, y1, label, *style);
        }
    }

    fn price_chart(
        &mut self,
        x: usize,
        y: usize,
        chart: &PriceChart,
        w: usize,
        h: usize,
        line: &'static str,
    ) {
        for (row_index, row) in chart.runs(w, h).into_iter().enumerate() {
            let mut cx = x;
            for run in row {
                let style = match run.kind {
                    RunKind::Line | RunKind::Marker => Style::fg(line),
                    RunKind::Grid => Style::fg(color::DISABLED),
                    RunKind::Axis => Style::fg(color::MUTED),
                    RunKind::Blank => Style::fg(color::FG),
                };
                cx = self.text(cx, y + row_index, &run.text, style);
            }
        }
    }
}

impl PaintCanvas for Screen {
    fn width(&self) -> usize {
        self.w
    }
    fn height(&self) -> usize {
        self.h
    }
    fn palette(&self) -> Palette {
        self.palette
    }
    fn put(&mut self, x: usize, y: usize, ch: char, style: Style) {
        Screen::put(self, x, y, ch, style);
    }
    fn read(&self, index: usize) -> Cell {
        self.cells[index].clone()
    }
    fn write(&mut self, index: usize, cell: Cell) {
        self.cells[index] = cell;
    }
}

/// Paints directly into the frame's Buffer. The owned [`Screen`] remains the
/// golden oracle; production components can use this surface as they migrate.
pub struct FrameCanvas<'a> {
    buffer: &'a mut Buffer,
    area: Rect,
    palette: Palette,
    colors: HashMap<ratatui::style::Color, &'static str>,
}

impl<'a> FrameCanvas<'a> {
    pub fn new(frame: &'a mut ratatui::Frame, area: Rect, palette: Palette) -> Self {
        let buffer = frame.buffer_mut();
        for y in area.y..area.bottom() {
            for x in area.x..area.right() {
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    *cell = ratatui::buffer::Cell::default();
                    cell.set_bg(hex_color(palette.remap(color::BLACK)));
                }
            }
        }
        Self {
            buffer,
            area,
            palette,
            colors: HashMap::from([(
                hex_color(palette.remap(color::BLACK)),
                palette.remap(color::BLACK),
            )]),
        }
    }

    fn position(&self, index: usize) -> (u16, u16) {
        (
            self.area.x + (index % self.width()) as u16,
            self.area.y + (index / self.width()) as u16,
        )
    }
}

impl PaintCanvas for FrameCanvas<'_> {
    fn width(&self) -> usize {
        self.area.width as usize
    }
    fn height(&self) -> usize {
        self.area.height as usize
    }
    fn palette(&self) -> Palette {
        self.palette
    }
    fn put(&mut self, x: usize, y: usize, ch: char, style: Style) {
        if x >= self.width() || y >= self.height() {
            return;
        }
        let fg = style.fg.map(|fg| {
            let token = if self.palette == Palette::Light && fg == color::BLUE {
                if style.bold || ch == '▸' {
                    "#095261"
                } else {
                    "#0f7d93"
                }
            } else {
                self.palette.remap(fg)
            };
            self.colors.insert(hex_color(token), token);
            hex_color(token)
        });
        let bg = style.bg.map(|bg| {
            let token = self.palette.remap(bg);
            self.colors.insert(hex_color(token), token);
            hex_color(token)
        });
        if let Some(cell) = self
            .buffer
            .cell_mut((self.area.x + x as u16, self.area.y + y as u16))
        {
            cell.set_char(ch);
            cell.fg = fg.unwrap_or(ratatui::style::Color::Reset);
            if let Some(bg) = bg {
                cell.set_bg(bg);
            }
            cell.modifier.set(Modifier::BOLD, style.bold);
        }
    }

    fn read(&self, index: usize) -> Cell {
        let cell = &self.buffer[self.position(index)];
        Cell {
            ch: cell.symbol().chars().next().unwrap_or(' '),
            fg: self.colors.get(&cell.fg).copied(),
            bg: self.colors.get(&cell.bg).copied(),
            bold: cell.modifier.contains(Modifier::BOLD),
        }
    }

    fn write(&mut self, index: usize, cell: Cell) {
        let position = self.position(index);
        let fg = cell.fg.map(hex_color);
        let bg = cell.bg.map(hex_color);
        if let Some(token) = cell.fg {
            self.colors.insert(hex_color(token), token);
        }
        if let Some(token) = cell.bg {
            self.colors.insert(hex_color(token), token);
        }
        if let Some(target) = self.buffer.cell_mut(position) {
            target.set_char(cell.ch);
            target.fg = fg.unwrap_or(ratatui::style::Color::Reset);
            target.bg = bg.unwrap_or(ratatui::style::Color::Reset);
            target.modifier.set(Modifier::BOLD, cell.bold);
        }
    }
}

impl Screen {
    /// A dark-theme screen (the goldens' default).
    pub fn new(w: usize, h: usize) -> Self {
        Self::themed(Palette::Dark, w, h)
    }

    /// A screen in `palette`.
    pub fn themed(palette: Palette, w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            cells: vec![
                Cell {
                    ch: ' ',
                    fg: None,
                    bg: Some(palette.remap(color::BLACK)),
                    bold: false,
                };
                w * h
            ],
            palette,
        }
    }

    pub fn put(&mut self, x: usize, y: usize, ch: char, style: Style) {
        if x < self.w && y < self.h {
            let fg = style.fg.map(|fg| self.remap_token(fg, style.bold, ch));
            let bg = style.bg.map(|bg| self.palette.remap(bg));
            let cell = &mut self.cells[y * self.w + x];
            cell.ch = ch;
            cell.fg = fg;
            cell.bg = bg.or(cell.bg);
            cell.bold = style.bold;
        }
    }

    /// Palette remap for one fg token. The `#5b8def` dark token splits in
    /// the light theme: bold runs read on the derived `text-primary`
    /// (`#095261`), while the pane border glyphs land on `$border`
    /// (`#0f7d93`) — and the agenda's `▸` glyph keeps `text-primary`
    /// though it is not bold. Verified against the exporter at all three
    /// matrix sizes (see `findings/rust-widgets.md`).
    fn remap_token(&self, fg: &'static str, bold: bool, ch: char) -> &'static str {
        if self.palette == Palette::Light && fg == color::BLUE {
            if bold || ch == '▸' {
                return "#095261";
            }
            return "#0f7d93";
        }
        self.palette.remap(fg)
    }

    /// Paint `text` left to right from (x, y); returns the next x.
    pub fn text(&mut self, x: usize, y: usize, text: &str, style: Style) -> usize {
        let mut x = x;
        for ch in text.chars() {
            self.put(x, y, ch, style);
            x += 1;
        }
        x
    }

    /// Paint `text` right-aligned so it ends at `end` (exclusive).
    pub fn text_right(&mut self, end: usize, y: usize, text: &str, style: Style) {
        let start = end.saturating_sub(text.chars().count());
        self.text(start, y, text, style);
    }

    /// The modal-screen base layer: Textual's modal screens paint a
    /// full-screen background that resolves the default foreground on
    /// every cell behind them (the exporter records `#d4d4d4` where the
    /// plain app screen leaves fg None). Modals call this before dimming.
    pub fn resolve_modal_base(&mut self) {
        let fg = self.palette.foreground();
        for cell in &mut self.cells {
            if cell.fg.is_none() {
                cell.fg = Some(fg);
            }
        }
    }

    /// Dim the whole screen: Textual's modal backdrop blends every resolved
    /// colour toward black at 60% — per channel, `int(v * 0.4)` (verified
    /// against the exporter: `#5b8def` -> `#24385f`, `#d4d4d4` -> `#545454`,
    /// `#22c55e` -> `#0d4e25`).
    pub fn dim(&mut self) {
        let foreground = self.palette.foreground();
        for cell in &mut self.cells {
            // A cell with no explicit fg inherits the modal screen's default
            // (foreground) *after* dimming, so it is not dimmed itself;
            // every explicitly painted colour blends toward black at 60%
            // (`#8a8a8a` -> `#373737`, `#ffffff` -> `#666666`, even the
            // plain foreground -> `#545454`).
            cell.fg = match cell.fg {
                None => Some(foreground),
                Some(fg) => Some(dim_hex(fg)),
            };
            cell.bg = cell.bg.map(dim_hex);
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
        for (row_index, row) in chart.runs(w, h).into_iter().enumerate() {
            let mut cx = x;
            for run in row {
                let style = match run.kind {
                    RunKind::Line | RunKind::Marker => Style::fg(line),
                    RunKind::Grid => Style::fg(color::DISABLED),
                    RunKind::Axis => Style::fg(color::MUTED),
                    RunKind::Blank => Style::fg(color::FG),
                };
                cx = self.text(cx, y + row_index, &run.text, style);
            }
        }
    }
}

/// Blit the golden `Screen` cell grid into a ratatui buffer (the live-frame
/// bridge: the golden painters target the cell model, the terminal takes
/// the buffer). Colour/attr parity matches what the exporter records:
/// fg, bg and bold.
pub fn blit(frame: &mut ratatui::Frame, screen: &Screen, area: ratatui::layout::Rect) {
    use ratatui::style::{Modifier, Style as RStyle};
    for y in 0..screen.h.min(area.height as usize) {
        for x in 0..screen.w.min(area.width as usize) {
            let cell = &screen.cells[y * screen.w + x];
            let mut style = RStyle::default();
            if let Some(fg) = cell.fg {
                style = style.fg(hex_color(fg));
            }
            if let Some(bg) = cell.bg {
                style = style.bg(hex_color(bg));
            }
            if cell.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            frame
                .buffer_mut()
                .cell_mut((area.x + x as u16, area.y + y as u16))
                .expect("cell in bounds")
                .set_char(cell.ch)
                .set_style(style);
        }
    }
}

/// `#rrggbb` (the exporter's resolved tokens) to a ratatui RGB colour.
pub fn hex_color(hex: &str) -> ratatui::style::Color {
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    ratatui::style::Color::Rgb(byte(1), byte(3), byte(5))
}

#[cfg(test)]
mod tests {
    use super::{blit, color, FrameCanvas, PaintCanvas, Screen, Style};
    use crate::theme::Palette;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    #[test]
    fn blit_respects_the_frame_area_origin() {
        let mut terminal = Terminal::new(TestBackend::new(6, 4)).unwrap();
        let mut screen = Screen::new(2, 1);
        screen.text(0, 0, "Hi", Style::DEFAULT);
        terminal
            .draw(|frame| blit(frame, &screen, Rect::new(2, 1, 2, 1)))
            .unwrap();
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(2, 1)].symbol(), "H");
        assert_eq!(buffer[(3, 1)].symbol(), "i");
        assert_eq!(buffer[(0, 0)].symbol(), " ");
    }

    #[test]
    fn frame_canvas_matches_owned_screen_for_overwrites_and_palette() {
        let area = Rect::new(2, 1, 4, 2);
        for palette in [Palette::Dark, Palette::Light] {
            let mut expected = Terminal::new(TestBackend::new(8, 4)).unwrap();
            let mut screen = Screen::themed(palette, 4, 2);
            screen.fill(0, 0, 4, 2, Style::fg(color::FG).bg(color::PANEL));
            screen.text(0, 0, "Hi", Style::fg(color::BLUE).bold());
            screen.put(1, 0, '!', Style::fg(color::MUTED));
            expected.draw(|frame| blit(frame, &screen, area)).unwrap();

            let mut actual = Terminal::new(TestBackend::new(8, 4)).unwrap();
            actual
                .draw(|frame| {
                    let mut canvas = FrameCanvas::new(frame, area, palette);
                    canvas.fill(0, 0, 4, 2, Style::fg(color::FG).bg(color::PANEL));
                    canvas.text(0, 0, "Hi", Style::fg(color::BLUE).bold());
                    canvas.put(1, 0, '!', Style::fg(color::MUTED));
                    assert_eq!(canvas.read(0).ch, 'H');
                    assert!(!canvas.read(1).bold);
                })
                .unwrap();
            assert_eq!(actual.backend().buffer(), expected.backend().buffer());
        }
    }

    #[test]
    fn frame_canvas_matches_owned_screen_for_pane_and_modal_dimming() {
        let area = Rect::new(1, 1, 14, 6);
        let title = [("title", Style::fg(color::BLUE).bold())];
        let hints = [("q quit".to_string(), Style::fg(color::MUTED))];
        let mut expected = Terminal::new(TestBackend::new(17, 9)).unwrap();
        let mut screen = Screen::new(14, 6);
        screen.pane(0, 0, 13, 5, true, &title, &hints);
        screen.resolve_modal_base();
        screen.dim();
        expected.draw(|frame| blit(frame, &screen, area)).unwrap();

        let mut actual = Terminal::new(TestBackend::new(17, 9)).unwrap();
        actual
            .draw(|frame| {
                let mut canvas = FrameCanvas::new(frame, area, Palette::Dark);
                canvas.pane(0, 0, 13, 5, true, &title, &hints);
                canvas.resolve_modal_base();
                canvas.dim();
            })
            .unwrap();
        assert_eq!(actual.backend().buffer(), expected.backend().buffer());
    }
}
