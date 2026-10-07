//! Text inputs drawn like Textual's `Input`/`TextArea`
//! (`docs/rewrite/tasks/r3-tui-foundations.md` item 3).
//!
//! `tui-textarea` carries the editing state (cursor, history, multi-line);
//! this module adds the Delta chrome — Textual's tall border
//! (`$border-blurred` blurred, `$border` focused), surface background,
//! `0 2` padding, `$text-disabled` placeholder and the block cursor — for
//! both the golden [`crate::screen::Screen`] model and live ratatui frames.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::Style as RStyle;
use ratatui::widgets::{Block, BorderType, Borders};
use tui_textarea::TextArea as Inner;

use crate::screen::{color, Screen, Style};
use crate::theme::Theme;
use crate::{Action, Component};

const PADDING: usize = 2; // Textual Input `padding: 0 2`

/// One-line input, the port shape of Textual's `Input`.
pub struct DeltaInput {
    inner: Inner<'static>,
    placeholder: String,
}

impl DeltaInput {
    pub fn new(placeholder: &str) -> Self {
        let mut inner = Inner::new(vec![String::new()]);
        inner.set_cursor_line_style(RStyle::default());
        inner.set_placeholder_text(placeholder.to_string());
        inner.set_placeholder_style(RStyle::default().fg(Theme::TEXT_DISABLED.color()));
        Self {
            inner,
            placeholder: placeholder.to_string(),
        }
    }

    /// The committed text (single line: everything on line 0).
    pub fn value(&self) -> &str {
        self.inner.lines().first().map(String::as_str).unwrap_or("")
    }

    pub fn set_value(&mut self, value: &str) {
        self.inner = Inner::new(vec![value.to_string()]);
        self.inner.set_cursor_line_style(RStyle::default());
        self.inner.set_placeholder_text(self.placeholder.clone());
        self.inner
            .set_placeholder_style(RStyle::default().fg(Theme::TEXT_DISABLED.color()));
        self.jump_end();
    }

    /// Move the caret after the last character (`End`).
    fn jump_end(&mut self) {
        self.inner.input(tui_textarea::Input {
            key: tui_textarea::Key::End,
            ctrl: false,
            alt: false,
            shift: false,
        });
    }

    pub fn clear(&mut self) {
        self.set_value("");
    }

    /// Keys, single-line: Enter submits (the caller's `Action`), arrows
    /// move the cursor within the line, printable keys insert.
    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<Action> {
        let input = to_textarea_input(key);
        match key.code {
            KeyCode::Enter | KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => None,
            KeyCode::Up => None, // forms browse fields with up/down
            _ => {
                self.inner.input(input);
                Some(Action::Noop)
            }
        }
    }

    /// Golden-model paint at `(x, y)`, `w` wide, 3 rows tall
    /// (`border: tall` + one content row). Textual's Input renders the
    /// value left-aligned inside `padding: 0 2`, placeholder in
    /// `$text-disabled`, and a block cursor over the character at the
    /// caret (styled by `input-cursor-*`: value colours inverted).
    pub fn draw_screen(&self, screen: &mut Screen, x: usize, y: usize, w: usize, focused: bool) {
        let border = if focused {
            color::BLUE
        } else {
            color::BORDER_BLURRED
        };
        let edge = Style::fg(border);
        let bg = Style::DEFAULT.bg(color::SURFACE);
        // Tall border: ╷ ╵ ╭─╮ │ │ ╰─╯ with the horizontal in the middle of
        // the top/bottom rows.
        screen.put(x, y, '╭', edge);
        screen.put(x + w - 1, y, '╮', edge);
        screen.put(x, y + 2, '╰', edge);
        screen.put(x + w - 1, y + 2, '╯', edge);
        for cx in x + 1..x + w - 1 {
            screen.put(cx, y, '─', edge);
            screen.put(cx, y + 2, '─', edge);
        }
        for cy in y + 1..y + 3 {
            screen.put(x, cy, '│', edge);
            screen.put(x + w - 1, cy, '│', edge);
        }
        screen.fill(x + 1, y + 1, x + w - 1, y + 2, bg);
        let value = self.value();
        let content_width = w.saturating_sub(2 + 2 * PADDING);
        if value.is_empty() {
            let placeholder = self.placeholder.as_str();
            let text: String = placeholder.chars().take(content_width).collect();
            screen.text(x + 1 + PADDING, y + 1, &text, Style::fg(color::DISABLED));
            if focused {
                // The caret sits before the placeholder; the placeholder's
                // first cell shows the block cursor under it.
                screen.put(
                    x + 1 + PADDING,
                    y + 1,
                    placeholder.chars().next().unwrap_or(' '),
                    Style::fg(color::FG).bg(color::DISABLED),
                );
            }
        } else {
            let cursor = self.inner.cursor().1;
            for (index, ch) in value.chars().take(content_width).enumerate() {
                let style = if focused && index == cursor {
                    Style::fg(color::BACKGROUND).bg(color::FG)
                } else {
                    Style::fg(color::FG)
                };
                screen.put(x + 1 + PADDING + index, y + 1, ch, style);
            }
            if focused && cursor >= value.chars().count() && cursor < content_width {
                // Cursor after the last character: an inverted space.
                screen.put(
                    x + 1 + PADDING + cursor,
                    y + 1,
                    ' ',
                    Style::fg(color::BACKGROUND).bg(color::FG),
                );
            }
        }
    }

    /// Live-frame paint (the ratatui path for future screens).
    pub fn draw(&self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect, focused: bool) {
        let border = if focused {
            Theme::BORDER.color()
        } else {
            Theme::BORDER_BLURRED.color()
        };
        let block = Block::default()
            .border_type(BorderType::Rounded)
            .borders(Borders::ALL)
            .border_style(RStyle::default().fg(border))
            .style(RStyle::default().bg(Theme::SURFACE.color()));
        let mut inner = self.inner.clone();
        inner.set_style(RStyle::default().fg(Theme::FOREGROUND.color()));
        if focused {
            inner.set_cursor_style(
                RStyle::default()
                    .fg(Theme::BACKGROUND.color())
                    .bg(Theme::FOREGROUND.color()),
            );
        } else {
            inner.set_cursor_style(RStyle::default());
        }
        inner.set_block(block);
        frame.render_widget(&inner, area);
    }
}

/// Multi-line text area, the port shape of Textual's `TextArea` (same
/// chrome as [`DeltaInput`], grows with content).
pub struct DeltaTextArea {
    inner: Inner<'static>,
    placeholder: String,
}

impl DeltaTextArea {
    pub fn new(placeholder: &str) -> Self {
        let mut inner = Inner::new(vec![String::new()]);
        inner.set_cursor_line_style(RStyle::default());
        inner.set_placeholder_text(placeholder.to_string());
        inner.set_placeholder_style(RStyle::default().fg(Theme::TEXT_DISABLED.color()));
        Self {
            inner,
            placeholder: placeholder.to_string(),
        }
    }

    pub fn value(&self) -> String {
        self.inner.lines().join("\n")
    }

    pub fn handle_key(&mut self, key: &KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc | KeyCode::Tab | KeyCode::BackTab => None,
            _ => {
                self.inner.input(to_textarea_input(key));
                Some(Action::Noop)
            }
        }
    }

    pub fn draw_screen(
        &self,
        screen: &mut Screen,
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        focused: bool,
    ) {
        let border = if focused {
            color::BLUE
        } else {
            color::BORDER_BLURRED
        };
        let edge = Style::fg(border);
        let bg = Style::DEFAULT.bg(color::SURFACE);
        screen.put(x, y, '╭', edge);
        screen.put(x + w - 1, y, '╮', edge);
        screen.put(x, y + h - 1, '╰', edge);
        screen.put(x + w - 1, y + h - 1, '╯', edge);
        for cx in x + 1..x + w - 1 {
            screen.put(cx, y, '─', edge);
            screen.put(cx, y + h - 1, '─', edge);
        }
        for cy in y + 1..y + h - 1 {
            screen.put(x, cy, '│', edge);
            screen.put(x + w - 1, cy, '│', edge);
        }
        screen.fill(x + 1, y + 1, x + w - 1, y + h - 1, bg);
        let (line, col) = self.inner.cursor();
        let lines = self.inner.lines();
        for (row, source) in lines.iter().enumerate().take(h.saturating_sub(2)) {
            let mut cx = x + 1 + PADDING;
            let content_width = w.saturating_sub(2 + 2 * PADDING);
            for (index, ch) in source.chars().take(content_width).enumerate() {
                let style = if focused && row == line && index == col {
                    Style::fg(color::BACKGROUND).bg(color::FG)
                } else {
                    Style::fg(color::FG)
                };
                screen.put(cx, y + 1 + row, ch, style);
                cx += 1;
            }
            if focused && row == line && col >= source.chars().count() {
                screen.put(
                    (x + 1 + PADDING + col).min(x + w - 2),
                    y + 1 + row,
                    ' ',
                    Style::fg(color::BACKGROUND).bg(color::FG),
                );
            }
        }
        let _ = self.placeholder.as_str();
    }
}

/// Map a crossterm key onto tui-textarea's `Input`.
fn to_textarea_input(key: &KeyEvent) -> tui_textarea::Input {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let tui_key = match key.code {
        KeyCode::Char(c) => tui_textarea::Key::Char(c),
        KeyCode::Backspace => tui_textarea::Key::Backspace,
        KeyCode::Delete => tui_textarea::Key::Delete,
        KeyCode::Enter => tui_textarea::Key::Enter,
        KeyCode::Left => tui_textarea::Key::Left,
        KeyCode::Right => tui_textarea::Key::Right,
        KeyCode::Up => tui_textarea::Key::Up,
        KeyCode::Down => tui_textarea::Key::Down,
        KeyCode::Home => tui_textarea::Key::Home,
        KeyCode::End => tui_textarea::Key::End,
        KeyCode::PageUp => tui_textarea::Key::PageUp,
        KeyCode::PageDown => tui_textarea::Key::PageDown,
        _ => tui_textarea::Key::Null,
    };
    tui_textarea::Input {
        key: tui_key,
        ctrl,
        alt,
        shift,
    }
}

impl Component for DeltaInput {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        DeltaInput::handle_key(self, &key)
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        DeltaInput::draw(self, frame, area, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn typing_lands_in_the_value() {
        let mut input = DeltaInput::new("name");
        for ch in "iron-ore".chars() {
            input.handle_key(&key(KeyCode::Char(ch)));
        }
        assert_eq!(input.value(), "iron-ore");
        input.handle_key(&key(KeyCode::Backspace));
        assert_eq!(input.value(), "iron-or");
    }

    #[test]
    fn enter_and_escape_are_reserved_for_the_form() {
        let mut input = DeltaInput::new("name");
        input.handle_key(&key(KeyCode::Char('x')));
        assert_eq!(
            DeltaInput::handle_key(&mut input, &key(KeyCode::Enter)),
            None
        );
        assert_eq!(DeltaInput::handle_key(&mut input, &key(KeyCode::Esc)), None);
        assert_eq!(input.value(), "x");
    }

    #[test]
    fn draw_screen_paints_placeholder_then_value() {
        let mut screen = Screen::new(40, 5);
        let input = DeltaInput::new("name");
        input.draw_screen(&mut screen, 2, 1, 20, false);
        let row: String = (0..20).map(|x| screen.cells[2 * 40 + x].ch).collect();
        assert!(row.contains("name"), "{row:?}");
        let mut filled = DeltaInput::new("name");
        for ch in "ore".chars() {
            filled.handle_key(&key(KeyCode::Char(ch)));
        }
        let mut screen = Screen::new(40, 5);
        filled.draw_screen(&mut screen, 2, 1, 20, true);
        let row: String = (0..20).map(|x| screen.cells[2 * 40 + x].ch).collect();
        assert!(row.contains("ore"), "{row:?}");
        assert!(!row.contains("name"), "{row:?}");
    }

    #[test]
    fn text_area_keeps_multiple_lines() {
        let mut area = DeltaTextArea::new("note");
        for ch in "first".chars() {
            area.handle_key(&key(KeyCode::Char(ch)));
        }
        area.handle_key(&key(KeyCode::Enter));
        for ch in "second".chars() {
            area.handle_key(&key(KeyCode::Char(ch)));
        }
        assert_eq!(area.value(), "first\nsecond");
    }
}
