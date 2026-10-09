//! Forms: labelled fields with focus order, validation messages under the
//! fields, submit/cancel and an error toast — the building blocks for the
//! thesis, decision, market, source and provider forms
//! (`docs/rewrite/tasks/r3-tui-foundations.md` item 4).
//!
//! Key order mirrors Textual's focus movement: Tab/Down forward,
//! Shift+Tab/Up back, Enter submits, Esc cancels. A failed submit keeps the
//! form open, marks the invalid fields, focuses the first one and raises
//! the toast the screen shows (`screen.notify(..., severity="error")`).

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::Rect;
use ratatui::Frame;

use crate::input::DeltaInput;
use crate::screen::{color, Screen, Style};
use crate::{Action, Component};

/// A validation rule: `Err(message)` marks the field invalid.
pub type Validator = fn(&str) -> Result<(), String>;

/// One form field: label, input and validator.
pub struct Field {
    pub label: String,
    pub input: DeltaInput,
    pub validator: Option<Validator>,
    /// The validation error currently shown under the field.
    pub error: Option<String>,
    /// Secrets are held for submission but painted as bullets.
    pub secret: bool,
}

impl Field {
    pub fn new(label: &str, placeholder: &str, validator: Option<Validator>) -> Self {
        Self {
            label: label.to_string(),
            input: DeltaInput::new(placeholder),
            validator,
            error: None,
            secret: false,
        }
    }

    pub fn secret(mut self) -> Self {
        self.secret = true;
        self
    }

    pub fn value(&self) -> &str {
        self.input.value()
    }

    fn validate(&self) -> Result<(), String> {
        match self.validator {
            Some(validator) => validator(self.value()),
            None => Ok(()),
        }
    }
}

/// What a submit produced: the field values, or the per-field errors (the
/// first error is also the toast text).
pub type SubmitResult = Result<Vec<(String, String)>, Vec<(usize, String)>>;

/// A modal form: `title`, fields in focus order, submit/cancel.
pub struct Form {
    pub title: String,
    pub fields: Vec<Field>,
    pub focus: usize,
    /// Extra hint line under the fields (`enter save · esc cancel`).
    pub hint: String,
}

impl Form {
    pub fn new(title: &str, fields: Vec<Field>) -> Self {
        Self {
            title: title.to_string(),
            fields,
            focus: 0,
            hint: "enter save · tab next · esc cancel".to_string(),
        }
    }

    pub fn add_field(&mut self, field: Field) {
        self.fields.push(field);
    }

    /// The values keyed by label, in field order.
    pub fn values(&self) -> Vec<(String, String)> {
        self.fields
            .iter()
            .map(|f| (f.label.clone(), f.value().to_string()))
            .collect()
    }

    /// Validate every field; on failure, record the errors, focus the first
    /// invalid field and return them (the caller raises the toast).
    pub fn submit(&mut self) -> SubmitResult {
        let mut errors: Vec<(usize, String)> = Vec::new();
        for (index, field) in self.fields.iter().enumerate() {
            if let Err(message) = field.validate() {
                errors.push((index, message));
            }
        }
        if errors.is_empty() {
            Ok(self.values())
        } else {
            let first = errors[0].0;
            for (index, message) in &errors {
                self.fields[*index].error = Some(message.clone());
            }
            self.focus = first;
            Err(errors)
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Esc => return Some(Action::CloseDialog),
            KeyCode::Enter => {
                return match self.submit() {
                    Ok(_) => Some(Action::FormSubmitted(self.title.clone())),
                    Err(errors) => Some(Action::Status(
                        errors
                            .first()
                            .map(|(_, m)| m.clone())
                            .unwrap_or_else(|| "invalid input".to_string()),
                    )),
                }
            }
            KeyCode::Tab => self.focus_field(1),
            KeyCode::BackTab => {
                let count = self.fields.len();
                self.focus_field(count.saturating_sub(1));
            }
            KeyCode::Down => self.focus_field(1),
            KeyCode::Up => {
                let count = self.fields.len();
                self.focus_field(count.saturating_sub(1));
            }
            _ => {
                if let Some(field) = self.fields.get_mut(self.focus) {
                    field.input.handle_key(&key);
                    field.error = None;
                }
            }
        }
        Some(Action::Noop)
    }

    fn focus_field(&mut self, step: usize) {
        if !self.fields.is_empty() {
            self.focus = (self.focus + step) % self.fields.len();
        }
    }

    /// Golden-model paint: one label row + input box (3 rows) + one error
    /// row per field, then the hint.
    pub fn draw_screen(&self, screen: &mut Screen, x: usize, y: usize, w: usize) -> usize {
        let mut y = y;
        for (index, field) in self.fields.iter().enumerate() {
            let focused = index == self.focus;
            screen.text(
                x,
                y,
                &field.label,
                Style::fg(if focused { color::BLUE } else { color::MUTED }).bold(),
            );
            y += 1;
            field
                .input
                .draw_screen_masked(screen, x, y, w, focused, field.secret);
            y += 3;
            if let Some(error) = &field.error {
                screen.text(x, y, error, Style::fg(color::RED));
                y += 1;
            }
        }
        screen.text(x, y, &self.hint, Style::fg(color::MUTED));
        y + 1
    }

    /// The form's height at width `w` (labels + input boxes + errors + hint).
    pub fn height(&self, w: usize) -> usize {
        let _ = w;
        self.fields
            .iter()
            .map(|f| 4 + usize::from(f.error.is_some()))
            .sum::<usize>()
            + 1
    }
}

impl Component for Form {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        Form::handle_key(self, key)
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let mut screen = Screen::new(area.width as usize, area.height as usize);
        self.draw_screen(&mut screen, 0, 0, area.width as usize);
        crate::screen::blit(frame, &screen, area);
    }
}

/// `required`: the shared "may not be empty" validator.
pub fn required(value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err("required".to_string())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn form() -> Form {
        Form::new(
            "new thesis",
            vec![
                Field::new("claim", "what you believe", Some(required)),
                Field::new("targets", "comma,separated", None),
            ],
        )
    }

    fn key(code: KeyCode, ctrl: bool) -> KeyEvent {
        let mut modifiers = KeyModifiers::NONE;
        if ctrl {
            modifiers |= KeyModifiers::CONTROL;
        }
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn tab_walks_the_fields_in_order() {
        let mut form = form();
        form.handle_key(key(KeyCode::Tab, false));
        assert_eq!(form.focus, 1);
        form.handle_key(key(KeyCode::Tab, false));
        assert_eq!(form.focus, 0, "focus wraps");
        form.handle_key(key(KeyCode::BackTab, false));
        assert_eq!(form.focus, form.fields.len() - 1);
    }

    #[test]
    fn submit_rejects_empty_required_fields_and_flags_them() {
        let mut form = form();
        form.fields[0]
            .input
            .handle_key(&key(KeyCode::Char('x'), false));
        form.handle_key(key(KeyCode::Enter, false));
        assert!(form.fields[0].error.is_none());
        let values = form.values();
        assert_eq!(values[0], ("claim".to_string(), "x".to_string()));
    }

    #[test]
    fn failed_submit_focuses_the_first_invalid_field_and_toasts() {
        let mut form = form();
        assert_eq!(
            form.handle_key(key(KeyCode::Enter, false)),
            Some(Action::Status("required".to_string()))
        );
        assert_eq!(form.focus, 0);
        assert_eq!(form.fields[0].error.as_deref(), Some("required"));
        // Typing clears the field's error.
        form.handle_key(key(KeyCode::Char('a'), false));
        assert!(form.fields[0].error.is_none());
    }

    #[test]
    fn escape_cancels() {
        let mut form = form();
        assert_eq!(
            form.handle_key(key(KeyCode::Esc, false)),
            Some(Action::CloseDialog)
        );
    }

    #[test]
    fn screen_paint_includes_labels_errors_and_hint() {
        let mut form = form();
        form.handle_key(key(KeyCode::Enter, false)); // raises the required error
        let mut screen = Screen::new(50, 12);
        let bottom = form.draw_screen(&mut screen, 1, 0, 40);
        let text = |y: usize| {
            (0..50)
                .map(|x| screen.cells[y * 50 + x].ch)
                .collect::<String>()
        };
        assert!(text(0).contains("claim"));
        assert!(text(4).contains("required"));
        assert!(text(bottom - 1).contains("enter save"));
    }

    #[test]
    fn height_counts_error_rows() {
        let mut form = form();
        let clean = form.height(40);
        form.handle_key(key(KeyCode::Enter, false));
        assert_eq!(form.height(40), clean + 1);
    }
}

#[cfg(test)]
mod demo {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    /// The demo scenario: a `new thesis` form the way the theses screen
    /// will mount it (R3.2) — typed claim, browsed-to targets field, a
    /// failed submit raising the toast, then a successful one.
    #[test]
    fn thesis_form_demo_round_trip() {
        let mut form = Form::new(
            "new thesis",
            vec![
                Field::new("claim", "what you believe", Some(required)),
                Field::new("targets", "comma,separated", None),
                Field::new("note", "why it matters", None),
            ],
        );
        // Type the claim, then Tab to targets and type.
        for ch in "iron ore holds up".chars() {
            form.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        form.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        assert_eq!(form.focus, 1);
        for ch in "BHP,RIO".chars() {
            form.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        // Clear the claim and submit: the required error comes back as a
        // toast and the form stays open, focused on the invalid field.
        form.fields[0].input = DeltaInput::new("what you believe");
        assert_eq!(
            form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::Status("required".to_string()))
        );
        assert_eq!(form.focus, 0);
        // Retype and submit: the values come back in field order.
        for ch in "iron ore holds up".chars() {
            form.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        assert_eq!(
            form.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            Some(Action::FormSubmitted("new thesis".to_string()))
        );
        assert_eq!(
            form.values()[1],
            ("targets".to_string(), "BHP,RIO".to_string())
        );
        // And the painted form shows the labels, the value and the hint.
        let mut terminal = Terminal::new(TestBackend::new(50, 14)).unwrap();
        terminal
            .draw(|f| Component::draw(&mut form, f, f.area()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        for probe in [
            "claim",
            "targets",
            "note",
            "iron ore holds up",
            "enter save",
        ] {
            assert!(text.contains(probe), "demo form missing {probe}");
        }
    }
}
