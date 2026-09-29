//! Screen furniture from `delta/tui/components.py`: EmptyState,
//! SectionHeading, the SuggestionList autocomplete, a command palette and a
//! which-key strip.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::theme::Theme;
use crate::{is_quit_key, Action, Component};

/// The empty state: a muted `✓`/`!` glyph line plus a hint
/// (port of `components.py::EmptyState`; `severe` turns the glyph red).
pub struct EmptyState {
    pub message: String,
    pub hint: String,
    pub severe: bool,
}

impl EmptyState {
    /// `✓ no earnings in the next 7 days` — the glyph answers "is this bad?".
    pub fn glyph(&self) -> &'static str {
        if self.severe {
            "!"
        } else {
            "✓"
        }
    }

    pub fn line(&self) -> Line<'_> {
        let glyph_style = Style::default().fg(if self.severe {
            Theme::TEXT_ERROR.color()
        } else {
            Theme::TEXT_SUCCESS.color()
        });
        Line::from(vec![
            Span::styled(format!("{} ", self.glyph()), glyph_style),
            Span::styled(
                self.message.clone(),
                Style::default().fg(Theme::TEXT_MUTED.color()),
            ),
        ])
    }
}

impl Component for EmptyState {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<Action> {
        None
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_widget(
            Paragraph::new(self.line()).alignment(Alignment::Center),
            area,
        );
    }
}

/// `▌ Heading` — the blue block + bold title
/// (port of `components.py::SectionHeading`).
pub struct SectionHeading {
    pub title: String,
}

impl Component for SectionHeading {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<Action> {
        None
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("▌ ", Style::default().fg(Theme::TEXT_PRIMARY.color())),
                Span::styled(
                    self.title.clone(),
                    Style::default()
                        .fg(Theme::TEXT_PRIMARY.color())
                        .add_modifier(Modifier::BOLD),
                ),
            ])),
            area,
        );
    }
}

/// The autocomplete list: a filtered option list with a highlight cursor
/// (port of `components.py::SuggestionList`).
#[derive(Debug, Clone, Default)]
pub struct SuggestionList {
    pub options: Vec<String>,
    filter: String,
    selected: usize,
}

impl SuggestionList {
    pub fn new(options: Vec<String>) -> Self {
        Self {
            options,
            filter: String::new(),
            selected: 0,
        }
    }

    /// The options matching the current filter, order preserved.
    pub fn visible(&self) -> Vec<String> {
        if self.filter.is_empty() {
            return self.options.clone();
        }
        let needle = self.filter.to_lowercase();
        self.options
            .iter()
            .filter(|o| o.to_lowercase().contains(&needle))
            .cloned()
            .collect()
    }

    pub fn selected(&self) -> Option<String> {
        let visible = self.visible();
        visible.get(self.selected).cloned()
    }

    /// Type-ahead: typing filters and resets the cursor; up/down move it;
    /// Enter accepts into `Action::Goto`. Quit keys still quit.
    pub fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let visible_len = self.visible().len();
        match key.code {
            _ if is_quit_key(key) => return Some(Action::Quit),
            KeyCode::Down => self.selected = (self.selected + 1).min(visible_len.saturating_sub(1)),
            KeyCode::Up => self.selected = self.selected.saturating_sub(1),
            KeyCode::Esc => return Some(Action::CloseDialog),
            KeyCode::Enter => {
                return self
                    .selected()
                    .map(|s| Action::Goto(Box::leak(s.into_boxed_str())))
            }
            KeyCode::Backspace => {
                self.filter.pop();
                self.selected = 0;
            }
            KeyCode::Char(c) => {
                self.filter.push(c);
                self.selected = 0;
            }
            _ => {}
        }
        Some(Action::Noop)
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self.visible().into_iter().map(ListItem::new).collect();
        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Theme::BORDER_BLURRED.color())),
            )
            .highlight_style(
                Style::default()
                    .bg(Theme::PRIMARY.color())
                    .fg(Theme::FOREGROUND.color()),
            )
            .highlight_symbol("▌ ");
        frame.render_stateful_widget(
            list,
            area,
            &mut ListState::default().with_selected(Some(self.selected)),
        );
    }
}

/// The `ctrl+k` command palette: a suggestion list over action names.
pub struct CommandPalette {
    pub suggestions: SuggestionList,
}

impl CommandPalette {
    pub fn new(commands: Vec<String>) -> Self {
        Self {
            suggestions: SuggestionList::new(commands),
        }
    }
}

impl Component for CommandPalette {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        self.suggestions.handle_key(key)
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        // Centred, top-anchored overlay, like the Python palette modal.
        let width = 50.min(area.width);
        let height = 12.min(area.height);
        let x = area.x + (area.width - width) / 2;
        let y = area.y + 2;
        self.suggestions.draw(
            frame,
            Rect {
                x,
                y,
                width,
                height,
            },
        );
    }
}

/// The which-key strip: `key hint · key hint` in the footer style
/// (port of `widgets.py::KeyStrip`/`hint_markup`).
pub struct WhichKey {
    pub bindings: Vec<(String, String)>,
}

impl WhichKey {
    /// `q quit · r refresh · / search`
    pub fn line(&self) -> String {
        self.bindings
            .iter()
            .map(|(key, hint)| format!("{key} {hint}"))
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

impl Component for WhichKey {
    fn handle_key(&mut self, _key: KeyEvent) -> Option<Action> {
        None
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let spans: Vec<Span> = self
            .bindings
            .iter()
            .flat_map(|(key, hint)| {
                vec![
                    Span::styled(key.clone(), Style::default().fg(Theme::FOOTER_KEY.color())),
                    Span::styled(
                        format!(" {hint}"),
                        Style::default().fg(Theme::TEXT_MUTED.color()),
                    ),
                    Span::raw(" · "),
                ]
            })
            .collect();
        let mut spans = spans;
        if !spans.is_empty() {
            spans.pop(); // trailing separator
        }
        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_state_glyph_flips_severe() {
        let ok = EmptyState {
            message: "no earnings in the next 7 days".to_string(),
            hint: String::new(),
            severe: false,
        };
        assert_eq!(ok.glyph(), "✓");
        let bad = EmptyState { severe: true, ..ok };
        assert_eq!(bad.glyph(), "!");
    }

    #[test]
    fn suggestion_filter_and_cursor() {
        let mut list = SuggestionList::new(vec![
            "watchlist AAPL".to_string(),
            "watchlist BHP".to_string(),
            "settings".to_string(),
        ]);
        assert_eq!(list.visible().len(), 3);
        list.handle_key(KeyEvent::from(KeyCode::Char('w')));
        list.handle_key(KeyEvent::from(KeyCode::Char('a')));
        assert_eq!(
            list.visible(),
            vec!["watchlist AAPL".to_string(), "watchlist BHP".to_string()]
        );
        list.handle_key(KeyEvent::from(KeyCode::Down));
        assert_eq!(list.selected(), Some("watchlist BHP".to_string()));
        // Cursor resets when the filter changes.
        list.handle_key(KeyEvent::from(KeyCode::Backspace));
        assert_eq!(list.selected(), Some("watchlist AAPL".to_string()));
    }

    #[test]
    fn suggestion_enter_accepts() {
        let mut list = SuggestionList::new(vec!["settings".to_string()]);
        assert_eq!(
            list.handle_key(KeyEvent::from(KeyCode::Enter)),
            Some(Action::Goto("settings"))
        );
    }

    #[test]
    fn which_key_line_format() {
        let wk = WhichKey {
            bindings: vec![
                ("q".to_string(), "quit".to_string()),
                ("r".to_string(), "refresh".to_string()),
            ],
        };
        assert_eq!(wk.line(), "q quit · r refresh");
    }
}
