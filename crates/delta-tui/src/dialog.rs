//! Modal dialogs (port of `delta/tui/widgets.py::Dialog`, `shell.py::GoPicker`
//! and `screens/help.py::HelpScreen` chrome).
//!
//! The Delta dialog frame: a thin `solid` border in `$border-blurred` on
//! the surface background, `padding: 1 2`, the title centred whole-line in
//! bold `$text-primary`, the hint centred whole-line with the keys bold.
//! Modal screens dim what is under them (`Screen::dim`).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::widgets::{Clear, Paragraph};
use ratatui::Frame;

use crate::components::KeyGrid;
use crate::keymap::{all_items, binding_key};
use crate::markdown;
use crate::screen::{color, Screen, Style};
use crate::theme::Theme;
use crate::Action;

/// The one modal width (`widgets.py::MODAL_WIDTH`).
pub const MODAL_WIDTH: usize = 64;
/// The single documented exception: the help modal's four-column keymap
/// (`widgets.py::MODAL_WIDTH_WIDE`).
pub const MODAL_WIDTH_WIDE: usize = 72;

/// `hint_markup`: `[bold $text-primary]key[/] label` pairs joined by two
/// spaces (`widgets.py::hint_markup`), as runs: key bold blue, the rest
/// muted.
pub fn hint_markup(pairs: &[(&str, &str)]) -> Vec<(String, Style)> {
    let mut runs = Vec::new();
    for (index, (key, label)) in pairs.iter().enumerate() {
        if index > 0 {
            runs.push(("  ".to_string(), Style::fg(color::MUTED)));
        }
        runs.push((key.to_string(), Style::fg(color::BLUE).bold()));
        runs.push((format!(" {label}"), Style::fg(color::MUTED)));
    }
    runs
}

/// Centred whole-line runs: pad to `width` on both sides. The padding
/// carries `pad_style` — the widget's own colour (Textual's content-align
/// extends the widget colour across the line, and the bold key runs sit
/// inside it).
pub fn centred_line(
    text: &[(&str, Style)],
    width: usize,
    pad_style: Style,
) -> Vec<(String, Style)> {
    let len: usize = text.iter().map(|(t, _)| t.chars().count()).sum();
    let pad = width.saturating_sub(len) / 2;
    let mut runs = vec![(" ".repeat(pad), pad_style)];
    for (part, part_style) in text {
        runs.push((part.to_string(), *part_style));
    }
    let trailing = width.saturating_sub(pad + len);
    runs.push((" ".repeat(trailing), pad_style));
    runs
}

/// Paint the dialog frame: the box centred in `screen` at `width` x
/// `height`, thin blurred border on the surface, interior cleared. Returns
/// `(content_x, content_y, content_w, content_h)` for the body (inside the
/// border and the `1 2` padding).
pub fn dialog_frame(
    screen: &mut Screen,
    width: usize,
    height: usize,
) -> (usize, usize, usize, usize) {
    let x0 = screen.w.saturating_sub(width) / 2;
    let y0 = screen.h.saturating_sub(height) / 2;
    let x1 = (x0 + width - 1).min(screen.w - 1);
    let y1 = (y0 + height - 1).min(screen.h - 1);
    // The frame paints its own surface under the border (`background:
    // $surface` on `#dialog-frame`).
    let edge = Style::fg(color::BORDER_BLURRED).bg(color::SURFACE);
    let interior = Style::DEFAULT.bg(color::SURFACE);
    screen.fill(x0 + 1, y0 + 1, x1, y1, interior);
    screen.put(x0, y0, '┌', edge);
    screen.put(x1, y0, '┐', edge);
    screen.put(x0, y1, '└', edge);
    screen.put(x1, y1, '┘', edge);
    for x in x0 + 1..x1 {
        screen.put(x, y0, '─', edge);
        screen.put(x, y1, '─', edge);
    }
    for y in y0 + 1..y1 {
        screen.put(x0, y, '│', edge);
        screen.put(x1, y, '│', edge);
    }
    (x0 + 3, y0 + 2, x1 - x0 - 5, y1 - y0 - 3)
}

/// The Go picker (`shell.py::GoPicker`): a centred keymap of every panel.
pub struct GoPicker;

impl GoPicker {
    /// The dialog's fixed height: borders + pad + title + margin + grid
    /// rows + margin + hint + pad (the exported 12 rows at every size).
    fn height() -> usize {
        2 + 1 + 1 + 1 + 4 + 1 + 1 + 1
    }

    pub fn draw_screen(screen: &mut Screen) {
        screen.dim();
        let (cx, cy, cw, _ch) = dialog_frame(screen, MODAL_WIDTH, Self::height());
        // Title, centred whole-line in bold blue (the title widget's colour).
        let title = centred_line(
            &[("go", Style::fg(color::BLUE).bold())],
            cw,
            Style::fg(color::BLUE).bold(),
        );
        paint_runs(screen, cx, cy, &title);
        // KeyGrid: key chips + descriptions, 8/1fr/8/1fr with gutters.
        let items: Vec<(String, String)> = all_items()
            .into_iter()
            .map(|(key, _name, label)| (key.to_string(), label.to_string()))
            .collect();
        let grid = KeyGrid { items };
        grid.draw_screen(screen, cx, cy + 2, cw);
        // Hint row: `key open  esc exit` (the hint widget is muted).
        let hint_y = cy + 2 + grid.items.len().div_ceil(2) + 1;
        let hint = hint_markup(&[("key", "open"), ("esc", "exit")]);
        let centred = centred_runs(&hint, cw, Style::fg(color::MUTED));
        paint_runs(screen, cx, hint_y, &centred);
    }
}

/// Centred runs with the pad split around the whole run sequence; padding
/// inherits `pad_style` (the hint widget's own colour).
fn centred_runs(runs: &[(String, Style)], width: usize, pad_style: Style) -> Vec<(String, Style)> {
    let len: usize = runs.iter().map(|(t, _)| t.chars().count()).sum();
    let pad = width.saturating_sub(len) / 2;
    let mut out = vec![(" ".repeat(pad), pad_style)];
    out.extend(runs.iter().cloned());
    let trailing = width.saturating_sub(pad + len);
    out.push((" ".repeat(trailing), pad_style));
    out
}

fn paint_runs(screen: &mut Screen, x: usize, y: usize, runs: &[(String, Style)]) {
    let mut cx = x;
    for (text, style) in runs {
        cx = screen.text(cx, y, text, *style);
    }
}

/// The help modal's tabs and hint (`screens/help.py::HelpScreen`).
#[derive(Debug, Clone, Copy, Default)]
pub struct HelpDialog {
    /// `t` tour, `k` keys — which tab is showing.
    pub tab: HelpTab,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HelpTab {
    #[default]
    Tour,
    Keys,
}

/// The tutorial the Python help shows (`screens/help.py::TUTORIAL`).
pub const TUTORIAL: &str = include_str!("help_tutorial.md");

impl HelpDialog {
    /// The frame is 90% of the screen (`HelpScreen > #dialog-frame`).
    fn height(screen: &Screen) -> usize {
        (screen.h * 9 / 10).max(3)
    }

    pub fn draw_screen(&self, screen: &mut Screen) {
        screen.dim();
        let height = Self::height(screen);
        let (cx, cy, cw, ch) = dialog_frame(screen, MODAL_WIDTH_WIDE, height);
        // Title.
        let title = centred_line(
            &[("delta help", Style::fg(color::BLUE).bold())],
            cw,
            Style::fg(color::BLUE).bold(),
        );
        paint_runs(screen, cx, cy, &title);
        // Tabs: the active one chips on `$primary`, the other stays muted.
        let tabs_y = cy + 2;
        let mut tx = cx;
        for (label, tab) in [("Getting started", HelpTab::Tour), ("Keys", HelpTab::Keys)] {
            if tab == self.tab {
                let width = label.chars().count() + 2;
                screen.fill(
                    tx,
                    tabs_y,
                    tx + width,
                    tabs_y + 1,
                    Style::DEFAULT.bg(color::BLUE_BG),
                );
                screen.put(tx, tabs_y, ' ', Style::DEFAULT.bg(color::BLUE_BG));
                tx = screen.text(
                    tx + 1,
                    tabs_y,
                    label,
                    Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
                );
                tx = screen.text(tx, tabs_y, " ", Style::DEFAULT.bg(color::BLUE_BG));
            } else {
                tx = screen.text(tx, tabs_y, label, Style::fg(color::TAB_INACTIVE_FG));
            }
            tx += 1;
        }
        // The tab underline: `╸` cap, the active tab's run in `$primary`,
        // the rest in the inactive grey, across the full content width.
        let underline_y = tabs_y + 1;
        let active_width = match self.tab {
            HelpTab::Tour => "Getting started".len(),
            HelpTab::Keys => "Keys".len(),
        };
        let mut ux = cx + 1;
        screen.put(cx, underline_y, '╸', Style::fg(color::TAB_UNDERLINE));
        let mut remaining = cw.saturating_sub(1);
        let mut in_active = active_width;
        let mut cap_painted = false;
        while remaining > 0 {
            let ch = if in_active > 0 {
                in_active -= 1;
                ('━', Style::fg(color::BLUE_BG))
            } else if !cap_painted {
                cap_painted = true;
                ('╺', Style::fg(color::TAB_UNDERLINE))
            } else {
                ('━', Style::fg(color::TAB_UNDERLINE))
            };
            screen.put(ux, underline_y, ch.0, ch.1);
            ux += 1;
            remaining -= 1;
        }
        // Content: the tour markdown, or the keymap groups. The tab pane
        // keeps one blank row above the hint (the goldens clip there).
        let content_y = underline_y + 1;
        let content_h = ch.saturating_sub((content_y - cy) + 2);
        match self.tab {
            HelpTab::Tour => {
                let blocks = markdown::render(TUTORIAL, cw.saturating_sub(4));
                let mut y = content_y;
                for runs in markdown::stack_lines(&blocks) {
                    if y >= content_y + content_h {
                        break;
                    }
                    paint_runs(screen, cx + 2, y, &markdown::Run::screen_runs(&runs));
                    y += 1;
                }
            }
            HelpTab::Keys => {
                let mut y = content_y;
                for (title, items) in crate::keymap::binding_groups() {
                    if items.is_empty() {
                        continue;
                    }
                    // `.help-group`: bold blue, one blank line before.
                    screen.text(cx + 2, y, title, Style::fg(color::BLUE).bold());
                    y += 1;
                    let grid = KeyGrid { items };
                    y += grid.draw_screen(screen, cx + 2, y, cw.saturating_sub(4));
                    y += 1;
                    if y >= content_y + content_h {
                        break;
                    }
                }
            }
        }
        // Hint row: the last content row inside the bottom padding.
        let hint_y = cy + ch - 1;
        let hint = hint_markup(&[("t", "tour"), ("k", "keys"), ("esc", "close")]);
        let centred = centred_runs(&hint, cw, Style::fg(color::MUTED));
        paint_runs(screen, cx, hint_y, &centred);
    }

    /// Keys: `?`/esc close (the modal stops the key before the app),
    /// `t`/`k` switch tabs.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('?') | KeyCode::Esc => Some(Action::CloseDialog),
            KeyCode::Char('t') => {
                self.tab = HelpTab::Tour;
                Some(Action::Noop)
            }
            KeyCode::Char('k') => {
                self.tab = HelpTab::Keys;
                Some(Action::Noop)
            }
            _ => Some(Action::Noop),
        }
    }
}

/// Keys on the Go picker: any nav item opens that panel, esc closes.
pub fn go_picker_key(key: KeyEvent) -> Option<Action> {
    if key.code == KeyCode::Esc {
        return Some(Action::CloseDialog);
    }
    let pressed = match key.code {
        KeyCode::Char(ch) => ch.to_string(),
        _ => return None,
    };
    for (item_key, name, _label) in all_items() {
        if binding_key(item_key) == pressed {
            return Some(Action::GotoScreen(name.to_string()));
        }
    }
    None
}

/// Draw one dialog-style box on a live frame (the ratatui path used by
/// `ModalStack`; matches [`dialog_frame`] chrome).
pub fn draw_modal(frame: &mut Frame, title: &str, body: Vec<String>, hint: &str, area: Rect) {
    use ratatui::style::{Modifier, Style as RStyle};
    use ratatui::text::{Line, Span};
    use ratatui::widgets::{Block, BorderType, Borders, Wrap};
    let width = 64.min(area.width);
    let height = (body.len() as u16 + 7).min(area.height).max(4);
    let x0 = area.x + (area.width - width) / 2;
    let y0 = area.y + (area.height - height) / 2;
    let box_area = Rect {
        x: x0,
        y: y0,
        width,
        height,
    };
    let mut lines: Vec<Line> = vec![Line::default()];
    lines.push(Line::from(Span::styled(
        format!(
            "{title:^width$}",
            width = (width.saturating_sub(6)) as usize
        ),
        RStyle::default()
            .fg(Theme::TEXT_PRIMARY.color())
            .add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::default());
    for line in body {
        lines.push(Line::from(Span::styled(
            line,
            RStyle::default().fg(Theme::FOREGROUND.color()),
        )));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled(
        format!("{hint:^width$}", width = (width.saturating_sub(6)) as usize),
        RStyle::default().fg(Theme::TEXT_MUTED.color()),
    )));
    frame.render_widget(Clear, box_area);
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .border_type(BorderType::Plain)
                    .border_style(RStyle::default().fg(Theme::BORDER_BLURRED.color()))
                    .borders(Borders::ALL),
            )
            .wrap(Wrap { trim: false }),
        box_area,
    );
}

/// The modal stack: dialogs layer over the pane content, last on top
/// (kept for screens that still answer through it).
#[derive(Debug, Clone, Default)]
pub struct ModalStack {
    pub dialogs: Vec<crate::dialog::Dialog>,
}

/// One modal dialog: title, body lines, and the confirm/cancel answer.
#[derive(Debug, Clone, Default)]
pub struct Dialog {
    pub title: String,
    pub body: Vec<String>,
    /// Shown in the footer as `key hint`; Enter confirms, Esc cancels.
    pub confirm_hint: String,
}

impl Dialog {
    pub fn new(title: impl Into<String>, body: Vec<String>) -> Self {
        Self {
            title: title.into(),
            body,
            confirm_hint: "enter confirm · esc cancel".to_string(),
        }
    }
}

impl ModalStack {
    pub fn is_open(&self) -> bool {
        !self.dialogs.is_empty()
    }

    pub fn push(&mut self, dialog: Dialog) {
        self.dialogs.push(dialog);
    }

    pub fn pop(&mut self) -> Option<Dialog> {
        self.dialogs.pop()
    }

    /// Keys go to the top dialog first; Esc/Enter answer it.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Enter => {
                self.pop();
                Some(Action::CloseDialog)
            }
            _ => None,
        }
    }

    /// Draw every open dialog over `area` (call after the panes).
    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        for dialog in &self.dialogs {
            draw_modal(
                frame,
                &dialog.title,
                dialog.body.clone(),
                &dialog.confirm_hint,
                area,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn stack_keys_answer_top_dialog() {
        let mut stack = ModalStack::default();
        stack.push(Dialog::new("Confirm", vec!["really?".to_string()]));
        assert!(stack.is_open());
        assert_eq!(
            stack.handle_key(KeyEvent::new(
                KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE
            )),
            Some(Action::CloseDialog)
        );
        assert!(!stack.is_open());
        stack.push(Dialog::new("Confirm", vec![]));
        assert_eq!(
            stack.handle_key(KeyEvent::new(
                KeyCode::Enter,
                crossterm::event::KeyModifiers::NONE
            )),
            Some(Action::CloseDialog)
        );
        assert!(!stack.is_open());
    }

    #[test]
    fn dialog_renders_centred_box() {
        let stack = ModalStack {
            dialogs: vec![Dialog::new("Confirm", vec!["really?".to_string()])],
        };
        let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
        terminal.draw(|f| stack.draw(f, f.area())).unwrap();
        let cell = |x: u16, y: u16| terminal.backend().buffer()[(x, y)].symbol().to_string();
        let text: Vec<String> = (0..12)
            .map(|y| (0..40).map(|x| cell(x, y)).collect())
            .collect();
        let box_rows: Vec<&String> = text.iter().filter(|r| r.contains('┌')).collect();
        assert_eq!(box_rows.len(), 1, "exactly one dialog box: {text:?}");
        assert!(text.iter().any(|r| r.contains("really?")));
        assert!(text.iter().any(|r| r.contains("enter confirm")));
    }

    #[test]
    fn go_dialog_paints_the_centred_keymap() {
        let mut screen = Screen::new(120, 40);
        // Content behind the modal: a muted cell proves the dim.
        screen.text(
            2,
            2,
            "muted",
            crate::screen::Style::fg(crate::screen::color::MUTED),
        );
        GoPicker::draw_screen(&mut screen);
        let row = |y: usize| {
            (0..120)
                .map(|x| screen.cells[y * 120 + x].ch)
                .collect::<String>()
        };
        // The box lands at the golden's position for 120x40.
        assert_eq!(row(14).trim().chars().next(), Some('┌'));
        let title = row(16);
        assert!(title.contains(" go "), "{title:?}");
        // Grid rows carry key chips and labels.
        assert!(row(18).contains("1") && row(18).contains("Home"));
        assert!(row(18).contains("5") && row(18).contains("Ask"));
        // The hint row is centred.
        assert!(row(23).contains("key open") && row(23).contains("esc exit"));
        // The backdrop: fg-None cells resolve to the (undimmed) default
        // foreground; the muted text dims to #373737.
        assert_eq!(screen.cells[0].fg, Some("#d4d4d4"));
        assert_eq!(screen.cells[2 * 120 + 2].fg, Some("#373737"));
    }

    #[test]
    fn help_dialog_paints_tabs_tutorial_and_hint() {
        let mut screen = Screen::new(120, 40);
        HelpDialog::default().draw_screen(&mut screen);
        let row = |y: usize| {
            (0..120)
                .map(|x| screen.cells[y * 120 + x].ch)
                .collect::<String>()
        };
        assert_eq!(row(2).trim().chars().next(), Some('┌'));
        assert!(row(4).contains("delta help"));
        assert!(row(6).contains("Getting started") && row(6).contains("Keys"));
        assert!(row(10).contains("Welcome to Delta"));
        assert!(row(12).starts_with(' ') && row(12).contains("Delta reads for you."));
        assert!(row(35).contains("t tour") && row(35).contains("esc close"));
    }

    #[test]
    fn help_dialog_keys_tab_lists_the_binding_groups() {
        let mut screen = Screen::new(120, 40);
        HelpDialog { tab: HelpTab::Keys }.draw_screen(&mut screen);
        let row = |y: usize| {
            (0..120)
                .map(|x| screen.cells[y * 120 + x].ch)
                .collect::<String>()
        };
        assert!(row(8).contains("Anywhere"), "{:?}", row(8));
        let all: String = (8..34).map(row).collect();
        assert!(all.contains("Ask") && all.contains("Watchlist") && all.contains("Theses"));
    }

    #[test]
    fn help_frame_is_90_percent_and_centred() {
        let mut screen = Screen::new(80, 24);
        HelpDialog::default().draw_screen(&mut screen);
        let row = |y: usize| {
            (0..80)
                .map(|x| screen.cells[y * 80 + x].ch)
                .collect::<String>()
        };
        assert_eq!(row(1).trim().chars().next(), Some('┌'));
        assert!(row(9).contains("Welcome to Delta"));
    }

    #[test]
    fn go_picker_keys_resolve_to_screens() {
        assert_eq!(
            go_picker_key(KeyEvent::new(
                KeyCode::Char('4'),
                crossterm::event::KeyModifiers::NONE
            )),
            Some(Action::GotoScreen("theses".to_string()))
        );
        assert_eq!(
            go_picker_key(KeyEvent::new(
                KeyCode::Char('c'),
                crossterm::event::KeyModifiers::NONE
            )),
            Some(Action::GotoScreen("config".to_string()))
        );
        assert_eq!(
            go_picker_key(KeyEvent::new(
                KeyCode::Esc,
                crossterm::event::KeyModifiers::NONE
            )),
            Some(Action::CloseDialog)
        );
    }
}

#[cfg(test)]
mod debug_tmp2 {
    use super::*;
    #[test]
    fn debug_help_rows80() {
        let mut screen = Screen::new(80, 24);
        HelpDialog::default().draw_screen(&mut screen);
        for y in [11usize, 12, 13, 15, 18] {
            let row: String = (0..80).map(|x| screen.cells[y * 80 + x].ch).collect();
            println!("got  {y} {:?}", row.trim_end());
            let bolds: Vec<usize> = (5..74).filter(|x| screen.cells[y * 80 + x].bold).collect();
            if !bolds.is_empty() {
                println!("     bold cols {bolds:?}");
            }
        }
    }
}
