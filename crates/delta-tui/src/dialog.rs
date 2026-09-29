//! Modal dialog + modal stack (port of `delta/tui/widgets.py::Dialog` and the
//! screen-level modal handling). Hand-drawn to match Textual's bordered,
//! centred box on a dimmed backdrop.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};
use ratatui::Frame;

use crate::theme::Theme;
use crate::{is_quit_key, Action};

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

    /// The dialog box centred inside `area`, at most 60 columns / 80% height,
    /// mirroring the Python `Dialog` CSS (width: 60; max-height: 80%).
    fn box_area(&self, area: Rect) -> Rect {
        let width = (self
            .body
            .iter()
            .map(String::len)
            .max()
            .unwrap_or(0)
            .max(self.title.len())
            .max(self.confirm_hint.len())
            .min(60)
            + 4) as u16; // borders + padding
        let width = width.min(area.width).max(8);
        let height = (self.body.len() as u16 + 4).min(area.height * 4 / 5).max(4);
        let [inner] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        let [inner] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(inner);
        inner
    }

    fn draw(&self, frame: &mut Frame, area: Rect) {
        let box_area = self.box_area(area);
        let focused = Style::default().fg(Theme::TEXT_PRIMARY.color());
        let muted = Style::default().fg(Theme::TEXT_MUTED.color());
        let block = Block::default()
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Theme::BORDER.color()))
            .borders(Borders::ALL)
            .title(Span::styled(self.title.clone(), focused));
        let mut lines: Vec<Line> = self
            .body
            .iter()
            .map(|l| {
                Line::from(Span::styled(
                    l.clone(),
                    Style::default().fg(Theme::FOREGROUND.color()),
                ))
            })
            .collect();
        lines.push(Line::default());
        lines.push(Line::from(Span::styled(self.confirm_hint.clone(), muted)));
        frame.render_widget(Clear, box_area);
        frame.render_widget(
            Paragraph::new(lines)
                .block(block)
                .wrap(Wrap { trim: false }),
            box_area,
        );
    }
}

/// The modal stack: dialogs layer over the pane content, last on top.
#[derive(Debug, Clone, Default)]
pub struct ModalStack {
    pub dialogs: Vec<Dialog>,
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
            KeyCode::Esc => {
                self.pop();
                Some(Action::CloseDialog)
            }
            KeyCode::Enter => {
                self.pop();
                Some(Action::CloseDialog)
            }
            _ if is_quit_key(key) => Some(Action::Quit),
            _ => None,
        }
    }

    /// Draw every open dialog over `area` (call after the panes).
    pub fn draw(&self, frame: &mut Frame, area: Rect) {
        for dialog in &self.dialogs {
            dialog.draw(frame, area);
        }
    }
}

/// Draw the dimmed backdrop the Python modal screen paints over the content.
pub fn dim_backdrop(frame: &mut Frame, area: Rect) {
    frame.render_widget(
        Block::default().borders(Borders::NONE).style(
            Style::default()
                .bg(Theme::BACKGROUND.color())
                .add_modifier(Modifier::DIM),
        ),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn stack_keys_answer_top_dialog() {
        let mut stack = ModalStack::default();
        stack.push(Dialog::new("Confirm", vec!["really?".to_string()]));
        assert!(stack.is_open());
        assert_eq!(
            stack.handle_key(KeyEvent::from(KeyCode::Esc)),
            Some(Action::CloseDialog)
        );
        assert!(!stack.is_open());
        stack.push(Dialog::new("Confirm", vec![]));
        assert_eq!(
            stack.handle_key(KeyEvent::from(KeyCode::Enter)),
            Some(Action::CloseDialog)
        );
        assert!(!stack.is_open());
        assert_eq!(
            stack.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn dialog_renders_centred_box() {
        let stack = ModalStack {
            dialogs: vec![Dialog::new("Confirm", vec!["really?".to_string()])],
        };
        let backend = TestBackend::new(40, 12);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| stack.draw(f, f.area())).unwrap();
        let cell = |x: u16, y: u16| terminal.backend().buffer()[(x, y)].symbol().to_string();
        let text: Vec<String> = (0..12)
            .map(|y| (0..40).map(|x| cell(x, y)).collect())
            .collect();
        let box_rows: Vec<&String> = text.iter().filter(|r| r.contains('╭')).collect();
        assert_eq!(box_rows.len(), 1, "exactly one dialog box: {text:?}");
        let row = text.iter().find(|r| r.contains("really?")).unwrap();
        assert!(row.contains("really?"));
        assert!(text
            .iter()
            .any(|r| r.contains("enter confirm · esc cancel")));
    }
}
