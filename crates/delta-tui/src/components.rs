//! Screen furniture from `delta/tui/components.py`: EmptyState,
//! SectionHeading, the SuggestionList autocomplete, a command palette and a
//! which-key strip.

use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::screen::{color, Screen, Style as CellStyle};
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

/// The `ctrl+k`/`ctrl+p` command palette (port of `app.py::DeltaCommands`
/// over `textual.command.Provider`, drawn like Textual's `CommandPalette`).
///
/// Commands: Textual's five system commands (what the empty query shows —
/// the Python `DeltaCommands` matcher scores zero on empty queries, so its
/// hits only appear once the query is non-empty), then the Go-to-panel
/// items, Gather and the theme toggle. Fuzzy matching is `nucleo`
/// (the plan's approved crate).
pub struct CommandPalette {
    pub query: String,
    /// `(label, hint, action)` in Python's search order.
    pub commands: Vec<PaletteCommand>,
    selected: usize,
}

/// One palette command.
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub label: String,
    pub hint: &'static str,
    pub action: Action,
}

impl CommandPalette {
    /// The command list the Python app installs (system commands first,
    /// then `Go to {label}` per nav item, Gather, theme toggle).
    pub fn delta_commands() -> Vec<PaletteCommand> {
        let mut commands = vec![
            PaletteCommand {
                label: "Keys".to_string(),
                hint: "Show help for the focused widget and a summary of available keys",
                action: Action::ShowHelp,
            },
            PaletteCommand {
                label: "Maximize".to_string(),
                hint: "Maximize the focused widget",
                action: Action::Noop,
            },
            PaletteCommand {
                label: "Quit".to_string(),
                hint: "Quit the application as soon as possible",
                action: Action::Quit,
            },
            PaletteCommand {
                label: "Screenshot".to_string(),
                hint: "Save an SVG 'screenshot' of the current screen",
                action: Action::Noop,
            },
            PaletteCommand {
                label: "Theme".to_string(),
                hint: "Change the current theme",
                action: Action::ToggleTheme,
            },
        ];
        for (key, name, label) in crate::keymap::all_items() {
            commands.push(PaletteCommand {
                label: format!("Go to {label}"),
                hint: Box::leak(format!("shortcut: {key}").into_boxed_str()),
                action: Action::GotoScreen(name.to_string()),
            });
        }
        commands.push(PaletteCommand {
            label: "Gather evidence".to_string(),
            hint: "ingest + extract",
            action: Action::Gather,
        });
        commands.push(PaletteCommand {
            label: "Toggle light/dark theme".to_string(),
            hint: "",
            action: Action::ToggleTheme,
        });
        commands
    }

    pub fn new() -> Self {
        Self {
            query: String::new(),
            commands: Self::delta_commands(),
            selected: 0,
        }
    }

    /// Matches for the current query: the system commands always, the
    /// Delta commands once the query is non-empty (the Python matcher
    /// scores zero on empty patterns), best score first, ties in install
    /// order.
    pub fn hits(&self) -> Vec<usize> {
        if self.query.is_empty() {
            return (0..5).collect();
        }
        let mut config = nucleo::Config::DEFAULT;
        config.ignore_case = true;
        let mut matcher = nucleo::Matcher::new(config);
        let mut needle_buf = Vec::new();
        let needle = nucleo::Utf32Str::new(self.query.as_str(), &mut needle_buf);
        let mut scored: Vec<(u32, usize)> = self
            .commands
            .iter()
            .enumerate()
            .filter_map(|(index, command)| {
                let mut haystack_buf = Vec::new();
                let haystack = nucleo::Utf32Str::new(command.label.as_str(), &mut haystack_buf);
                matcher
                    .fuzzy_match(haystack, needle)
                    .map(|score| (u32::from(score), index))
            })
            .collect();
        scored.sort_by(|a, b| b.0.cmp(&a.0));
        scored.into_iter().map(|(_, index)| index).collect()
    }

    pub fn type_char(&mut self, ch: char) {
        self.query.push(ch);
        self.selected = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
    }

    /// Paint the palette overlay on the golden `Screen`: full width, rows
    /// 3..16 (Textual's bottom palette strip), exactly as exported — the
    /// `▔`/`▁` borders, the surface input row (block cursor + placeholder),
    /// and two rows per hit (bold title, muted description) with the first
    /// hit on the `$block-cursor-blurred` highlight.
    pub fn draw_screen(&self, screen: &mut Screen) {
        use crate::screen::color;
        // The palette is a modal screen: it resolves the default fg behind
        // itself and dims the content (the exported rows outside the
        // strip carry the 60% blend).
        screen.dim();
        let w = screen.w;
        let top = 3usize.min(screen.h.saturating_sub(14));
        let bottom = (top + 13).min(screen.h.saturating_sub(1));
        let border_top = CellStyle::fg(color::SCRIM_FG).bg(color::SCRIM_BG);
        let border_bottom = CellStyle::fg("#000000").bg(color::SCRIM_BG);
        let scrim = CellStyle::fg("#00ff00").bg(color::SCRIM_BG);
        for x in 0..w {
            screen.put(x, top, '▔', border_top);
            screen.put(x, bottom, '▁', border_bottom);
        }
        // Input row.
        let input_y = top + 1;
        if input_y < bottom {
            let surface = CellStyle::DEFAULT.bg(color::SURFACE);
            let input_default = CellStyle::fg(color::FG).bg(color::SURFACE);
            screen.put(0, input_y, ' ', border_top);
            screen.fill(
                1,
                input_y,
                4,
                input_y + 1,
                CellStyle::DEFAULT.bg(color::SCRIM_BG),
            );
            screen.fill(4, input_y, w - 1, input_y + 1, surface);
            screen.put(w - 1, input_y, ' ', border_top);
            screen.put(
                4,
                input_y,
                '█',
                CellStyle::fg(color::BLUE).bg(color::SURFACE),
            );
            // The cell after the cursor and the cell before the border stay
            // unstyled (the exported input row's two None-fg cells).
            screen.put(5, input_y, ' ', CellStyle::DEFAULT.bg(color::SURFACE));
            if self.query.is_empty() {
                // The exported placeholder: its first cell in the selection
                // colours, the rest `$text-disabled`, the row after in the
                // value style.
                let placeholder = "Search for commands…";
                screen.put(
                    6,
                    input_y,
                    placeholder.chars().next().unwrap_or('S'),
                    CellStyle::fg("#000000").bg(color::FG),
                );
                let rest: String = placeholder.chars().skip(1).collect();
                screen.text(
                    7,
                    input_y,
                    &rest,
                    CellStyle::fg(color::DISABLED).bg(color::SURFACE),
                );
                let after = 7 + rest.chars().count();
                screen.fill(after, input_y, w - 2, input_y + 1, input_default);
            } else {
                let text: String = self.query.chars().take(w.saturating_sub(9)).collect();
                let len = text.chars().count();
                screen.text(6, input_y, &text, input_default);
                screen.fill(6 + len, input_y, w - 2, input_y + 1, input_default);
            }
            screen.put(w - 2, input_y, ' ', CellStyle::DEFAULT.bg(color::SURFACE));
            screen.fill(0, input_y + 1, w, input_y + 2, scrim);
        }
        // Hit rows: two per hit, first on the highlight.
        let hits = self.hits();
        let mut y = top + 3;
        for (position, index) in hits.iter().enumerate() {
            if y + 1 >= bottom {
                break;
            }
            let command = &self.commands[*index];
            let selected = position == self.selected;
            let bg = if selected {
                color::HIT_SELECTED_BG
            } else {
                color::SCRIM_BG
            };
            let bold_fg = CellStyle::fg(color::FG).bold().bg(bg);
            screen.fill(0, y, w, y + 1, bold_fg);
            screen.text(2, y, &command.label, bold_fg);
            let desc_fg = if selected {
                color::HIT_DESC_SELECTED
            } else {
                color::HIT_DESC
            };
            screen.fill(0, y + 1, w, y + 2, bold_fg);
            screen.text(2, y + 1, command.hint, CellStyle::fg(desc_fg).bg(bg));
            y += 2;
        }
    }

    pub fn selected_action(&self) -> Option<Action> {
        self.hits()
            .get(self.selected)
            .map(|i| self.commands[*i].action.clone())
    }
}

impl Default for CommandPalette {
    fn default() -> Self {
        Self::new()
    }
}

impl Component for CommandPalette {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let hits = self.hits().len();
        match key.code {
            KeyCode::Esc => Some(Action::CloseDialog),
            KeyCode::Enter => Some(self.selected_action().unwrap_or(Action::Noop)),
            KeyCode::Down => {
                self.selected = (self.selected + 1).min(hits.saturating_sub(1));
                Some(Action::Noop)
            }
            KeyCode::Up => {
                self.selected = self.selected.saturating_sub(1);
                Some(Action::Noop)
            }
            KeyCode::Backspace => {
                self.backspace();
                Some(Action::Noop)
            }
            KeyCode::Char(ch) => {
                self.type_char(ch);
                Some(Action::Noop)
            }
            _ => Some(Action::Noop),
        }
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let mut screen = Screen::new(area.width as usize, area.height as usize);
        self.draw_screen(&mut screen);
        crate::screen::blit(frame, &screen, area);
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
    use crossterm::event::KeyModifiers;

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

    #[test]
    fn palette_empty_query_shows_only_the_system_commands() {
        let palette = CommandPalette::new();
        let hits = palette.hits();
        let labels: Vec<&str> = hits
            .iter()
            .map(|i| palette.commands[*i].label.as_str())
            .collect();
        assert_eq!(
            labels,
            vec!["Keys", "Maximize", "Quit", "Screenshot", "Theme"]
        );
    }

    #[test]
    fn palette_query_finds_delta_commands() {
        let mut palette = CommandPalette::new();
        for ch in "goto".chars() {
            palette.type_char(ch);
        }
        let hits = palette.hits();
        assert!(!hits.is_empty());
        let labels: Vec<&str> = hits
            .iter()
            .map(|i| palette.commands[*i].label.as_str())
            .collect();
        assert!(labels.iter().any(|l| l.starts_with("Go to")));
    }

    #[test]
    fn palette_typing_filters_and_enter_picks() {
        let mut palette = CommandPalette::new();
        for ch in "quit".chars() {
            palette.type_char(ch);
        }
        palette.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        // Enter resolves to an action without touching the app state.
        assert!(matches!(
            palette.selected_action(),
            Some(Action::Quit) | Some(Action::Noop)
        ));
        assert_eq!(
            palette.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
            Some(Action::CloseDialog)
        );
    }
}

// ---------------------------------------------------------------- widgets
// The design-system primitives from `delta/tui/widgets.py` the R3 screens
// draw with: chips, dots, pills, the modal key grid and scrollbars.

/// The `ActionChip`: one-row `[key] label` button (`widgets.py::ActionChip`).
/// `-active` marks the selected tab; disabled chips mute their text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ChipState {
    #[default]
    Normal,
    Active,
    Disabled,
    Primary,
}

#[derive(Debug, Clone)]
pub struct ActionChip {
    pub key: String,
    pub label: String,
    pub state: ChipState,
}

impl ActionChip {
    pub fn new(key: &str, label: &str) -> Self {
        Self {
            key: key.to_string(),
            label: label.to_string(),
            state: ChipState::Normal,
        }
    }

    pub fn active(key: &str, label: &str) -> Self {
        Self {
            state: ChipState::Active,
            ..Self::new(key, label)
        }
    }

    /// Rendered width: padding (0 1) + key + space + label.
    pub fn width(&self) -> usize {
        self.key.chars().count() + self.label.chars().count() + 4
    }

    /// Paint at `(x, y)`; returns the x after the chip's trailing margin
    /// (`margin: 0 1 0 0`).
    pub fn draw_screen(&self, screen: &mut Screen, x: usize, y: usize) -> usize {
        use crate::screen::color;
        let (bg, label_fg) = match self.state {
            ChipState::Normal | ChipState::Primary => (color::PANEL, color::MUTED),
            ChipState::Active => (color::BLUE_BG, color::WHITE),
            ChipState::Disabled => (color::PANEL, color::DISABLED),
        };
        let width = self.width();
        screen.fill(x, y, x + width, y + 1, CellStyle::DEFAULT.bg(bg));
        let cx = screen.text(
            x + 1,
            y,
            &self.key,
            CellStyle::fg(color::BLUE).bold().bg(bg),
        );
        if self.state == ChipState::Active {
            let cx = screen.text(cx, y, " ", CellStyle::fg(color::WHITE).bg(bg));
            let _ = screen.text(cx, y, &self.label, CellStyle::fg(color::WHITE).bg(bg));
        } else {
            let cx = screen.text(cx, y, " ", CellStyle::fg(label_fg).bg(bg));
            let _ = screen.text(cx, y, &self.label, CellStyle::fg(label_fg).bg(bg));
        }
        x + width + 1
    }
}

/// `StatusDot`: a coloured bullet (`widgets.py::StatusDot`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DotState {
    Ok,
    #[default]
    Warn,
    Error,
    Dim,
}

impl DotState {
    pub fn color(self) -> &'static str {
        match self {
            DotState::Ok => "#22c55e",
            DotState::Warn => "#f59e0b",
            DotState::Error => "#f87171",
            DotState::Dim => "#8a8a8a",
        }
    }

    pub fn from_state(state: &str) -> Self {
        match state {
            "ok" => DotState::Ok,
            "warn" => DotState::Warn,
            "error" => DotState::Error,
            _ => DotState::Dim,
        }
    }
}

#[derive(Debug, Clone)]
pub struct StatusDot {
    pub state: DotState,
}

impl StatusDot {
    pub fn new(state: DotState) -> Self {
        Self { state }
    }

    pub fn set_state(&mut self, state: DotState) {
        self.state = state;
    }

    pub fn draw_screen(&self, screen: &mut Screen, x: usize, y: usize) {
        screen.put(x, y, '●', CellStyle::fg(self.state.color()));
    }
}

/// `Pill`: a small inline token for kinds, statuses and thesis health
/// (`widgets.py::Pill` + `health_variant`/`sentiment_variant`).
#[derive(Debug, Clone)]
pub struct Pill {
    pub text: String,
    pub variant: PillVariant,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PillVariant {
    Ok,
    Warn,
    Error,
    #[default]
    Dim,
}

impl Pill {
    pub fn new(text: &str, variant: PillVariant) -> Self {
        Self {
            text: text.to_string(),
            variant,
        }
    }

    /// `health_variant`: thesis-health word to Pill class.
    pub fn health_variant(state: &str) -> PillVariant {
        match state {
            "building" => PillVariant::Ok,
            "mixed" | "weakening" => PillVariant::Warn,
            "challenged" => PillVariant::Error,
            _ => PillVariant::Dim,
        }
    }

    /// `sentiment_variant`: report sentiment (-1..1) to Pill class.
    pub fn sentiment_variant(score: f64) -> PillVariant {
        if score <= -0.3 {
            PillVariant::Error
        } else if score >= 0.3 {
            PillVariant::Ok
        } else {
            PillVariant::Warn
        }
    }

    pub fn fg(&self) -> &'static str {
        match self.variant {
            PillVariant::Ok => "#22c55e",
            PillVariant::Warn => "#f59e0b",
            PillVariant::Error => "#f87171",
            PillVariant::Dim => "#8a8a8a",
        }
    }

    pub fn draw_screen(&self, screen: &mut Screen, x: usize, y: usize) -> usize {
        screen.text(x, y, &self.text, CellStyle::fg(self.fg()))
    }
}

/// `KeyGrid`: the modal keymap pattern — two key/description column pairs
/// (`widgets.py::KeyGrid`: `grid-columns: 8 1fr 8 1fr`, `grid-gutter: 0 1`).
pub struct KeyGrid {
    pub items: Vec<(String, String)>,
}

impl KeyGrid {
    pub fn new(items: Vec<(&str, &str)>) -> Self {
        Self {
            items: items
                .into_iter()
                .map(|(k, d)| (k.to_string(), d.to_string()))
                .collect(),
        }
    }

    /// One key chip: ` key ` on the panel background, key bold blue
    /// (the exported KeyHint cells).
    fn chip(screen: &mut Screen, x: usize, y: usize, key: &str) {
        screen.put(x, y, ' ', CellStyle::DEFAULT.bg(color::PANEL));
        screen.text(
            x + 1,
            y,
            key,
            CellStyle::fg(color::BLUE).bold().bg(color::PANEL),
        );
        screen.put(x + 2, y, ' ', CellStyle::DEFAULT.bg(color::PANEL));
    }

    /// Paint at `(x, y)` across `w` columns; returns the height used.
    pub fn draw_screen(&self, screen: &mut Screen, x: usize, y: usize, w: usize) -> usize {
        const GUTTER: usize = 1;
        const KEY_TRACK: usize = 8;
        let rows = self.items.len().div_ceil(2);
        if rows == 0 {
            return 0;
        }
        let fixed = 2 * KEY_TRACK + 3 * GUTTER;
        let desc = w.saturating_sub(fixed);
        // The two 1fr description tracks split the remainder; Textual's
        // rounding puts the extra cell in the second (right) track.
        let left = desc / 2;
        let left_col = x;
        let left_desc = left_col + KEY_TRACK + GUTTER;
        let right_col = left_desc + left + GUTTER;
        let right_desc = right_col + KEY_TRACK + GUTTER;
        let (split_left, split_right) = self.items.split_at(self.items.len().div_ceil(2));
        for (index, (key, desc_label)) in split_left.iter().enumerate() {
            let ry = y + index;
            Self::chip(screen, left_col, ry, key);
            screen.text(left_desc, ry, desc_label, CellStyle::fg(color::FG));
        }
        for (index, (key, desc_label)) in split_right.iter().enumerate() {
            let ry = y + index;
            Self::chip(screen, right_col, ry, key);
            screen.text(right_desc, ry, desc_label, CellStyle::fg(color::FG));
        }
        rows
    }
}

/// A vertical scrollbar drawn like Textual's `VerticalScroll` (the glyphs
/// the glossary golden pinned): 2-cell track in `$scrollbar` on the
/// surface, proportional thumb in the foreground token, half-block `▄`
/// top cap when the proportional start rounds up mid-cell.
pub struct Scrollbar;

impl Scrollbar {
    /// Paint a 2-cell scrollbar at columns `x0..x0+2`, rows `y0..y0+bar`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw_screen(
        screen: &mut Screen,
        x0: usize,
        y0: usize,
        bar: usize,
        virtual_size: usize,
        window: usize,
        offset: usize,
    ) {
        use crate::screen::color;
        let thumb =
            (((bar * window) as f32 / virtual_size.max(1) as f32).round() as usize).clamp(1, bar);
        let max_offset = virtual_size.saturating_sub(window);
        let exact = (bar - thumb) as f32 * offset as f32 / max_offset.max(1) as f32;
        let thumb_start = y0
            + if max_offset == 0 {
                0
            } else {
                exact.round() as usize
            };
        let thumb_start = thumb_start.min(y0 + bar - 1);
        let cap = exact.fract() > 0.5;
        let track = CellStyle::fg(color::SURFACE_SCROLLBAR).bg(color::SURFACE);
        let thumb_style = CellStyle::fg(color::FG).bg(color::SURFACE);
        for y in y0..y0 + bar {
            for x in x0..x0 + 2 {
                if y < thumb_start {
                    screen.put(x, y, ' ', track);
                } else if y == thumb_start && cap {
                    screen.put(x, y, '▄', track);
                } else {
                    screen.put(x, y, ' ', thumb_style);
                }
            }
        }
    }
}

#[cfg(test)]
mod widget_tests {
    use super::*;
    use crate::screen::color;

    fn row(screen: &Screen, y: usize) -> String {
        (0..screen.w)
            .map(|x| screen.cells[y * screen.w + x].ch)
            .collect()
    }

    #[test]
    fn chip_paints_key_label_pair_with_trailing_margin() {
        let mut screen = Screen::new(30, 1);
        let next = ActionChip::new("a", "add").draw_screen(&mut screen, 0, 0);
        assert_eq!(next, 9, "1 pad + key + space + 3 label + 1 pad + margin");
        let text = row(&screen, 0);
        assert!(text.contains(" a add "), "{text:?}");
        // The key keeps its bold blue on the chip background.
        let key_cell = &screen.cells[1];
        assert_eq!(key_cell.fg, Some(color::BLUE));
        assert!(key_cell.bold);
    }

    #[test]
    fn active_chip_inverts_to_the_block_cursor() {
        let mut screen = Screen::new(30, 1);
        ActionChip::active("1", "Home").draw_screen(&mut screen, 0, 0);
        // The key keeps its bold blue on the primary ground (the markup
        // keeps the `[bold $text-primary]` run); the label reads in the
        // block-cursor foreground.
        let key_cell = &screen.cells[1];
        assert_eq!(key_cell.bg, Some(color::BLUE_BG));
        assert_eq!(key_cell.fg, Some(color::BLUE));
        let label_cell = &screen.cells[4];
        assert_eq!(label_cell.fg, Some(color::WHITE));
        assert_eq!(label_cell.bg, Some(color::BLUE_BG));
    }

    #[test]
    fn status_dot_colours_by_state() {
        assert_eq!(DotState::from_state("ok"), DotState::Ok);
        assert_eq!(DotState::from_state("error"), DotState::Error);
        let mut screen = Screen::new(4, 1);
        StatusDot::new(DotState::Warn).draw_screen(&mut screen, 0, 0);
        assert_eq!(screen.cells[0].ch, '●');
        assert_eq!(screen.cells[0].fg, Some("#f59e0b"));
    }

    #[test]
    fn pill_variant_tables_match_widgets_py() {
        assert_eq!(Pill::health_variant("building"), PillVariant::Ok);
        assert_eq!(Pill::health_variant("weakening"), PillVariant::Warn);
        assert_eq!(Pill::health_variant("challenged"), PillVariant::Error);
        assert_eq!(Pill::health_variant("emerging"), PillVariant::Dim);
        assert_eq!(Pill::sentiment_variant(-0.5), PillVariant::Error);
        assert_eq!(Pill::sentiment_variant(0.0), PillVariant::Warn);
        assert_eq!(Pill::sentiment_variant(0.4), PillVariant::Ok);
    }

    #[test]
    fn key_grid_two_columns_with_gutters() {
        let mut screen = Screen::new(64, 4);
        let grid = KeyGrid::new(vec![
            ("1", "Home"),
            ("2", "Watchlist"),
            ("3", "Research"),
            ("4", "Theses"),
            ("5", "Ask"),
            ("6", "Decisions"),
            ("c", "Settings"),
        ]);
        let height = grid.draw_screen(&mut screen, 0, 0, 58);
        assert_eq!(height, 4, "(7 items + 1) / 2");
        // Row 0: `1 Home` left, `5 Ask` right (the go golden's layout).
        let text = row(&screen, 0);
        assert!(text.contains("1"), "{text:?}");
        assert!(text.contains("Home"), "{text:?}");
        assert!(text.contains("Ask"), "{text:?}");
    }

    #[test]
    fn scrollbar_tracks_thumb_and_cap() {
        let mut screen = Screen::new(4, 10);
        // Thumb position is proportional to the offset (the glossary
        // painter's "runs to the bottom" thumb).
        Scrollbar::draw_screen(&mut screen, 0, 0, 10, 30, 10, 10);
        assert_eq!(screen.cells[0].fg, Some(color::SURFACE_SCROLLBAR));
        assert_eq!(screen.cells[6 * 4].fg, Some(color::FG));
    }
}

/// `SuggestionList`'s big sibling: Textual's `OptionList` with the
/// full-row block-cursor highlight (`components.py::SuggestionList` draws
/// on one; the watchlist filter uses the same look). Arrow keys browse
/// with wraparound, as `SuggestionList.browse` does.
pub struct OptionList {
    pub options: Vec<String>,
    pub selected: usize,
}

impl OptionList {
    pub fn new(options: Vec<String>) -> Self {
        Self {
            options,
            selected: 0,
        }
    }

    pub fn highlighted(&self) -> Option<&str> {
        self.options.get(self.selected).map(String::as_str)
    }

    /// `browse`: up/down with wraparound; True when the key was consumed.
    pub fn browse(&mut self, key: &KeyEvent) -> bool {
        let count = self.options.len();
        match key.code {
            KeyCode::Down if count > 0 => {
                self.selected = (self.selected + 1) % count;
                true
            }
            KeyCode::Up if count > 0 => {
                self.selected = (self.selected + count - 1) % count;
                true
            }
            _ => false,
        }
    }

    /// The highlighted row paints `$block-cursor-background` /
    /// `$block-cursor-foreground` full width (Textual's OptionList
    /// highlight), like the watchlist's exported rows.
    pub fn draw_screen(&self, screen: &mut Screen, x: usize, y: usize, w: usize, visible: usize) {
        // The window keeps the highlight on screen without jumping.
        let start = self.selected.saturating_sub(visible.saturating_sub(1)).min(
            self.options
                .len()
                .saturating_sub(visible.min(self.options.len())),
        );
        for (index, option) in self.options.iter().skip(start).take(visible).enumerate() {
            let row = y + index;
            if start + index == self.selected {
                screen.fill(
                    x,
                    row,
                    x + w,
                    row + 1,
                    CellStyle::fg(color::WHITE).bg(color::BLUE_BG),
                );
                screen.text(
                    x + 1,
                    row,
                    option,
                    CellStyle::fg(color::WHITE).bg(color::BLUE_BG),
                );
            } else {
                screen.text(x + 1, row, option, CellStyle::fg(color::FG));
            }
        }
    }
}

#[cfg(test)]
mod option_list_tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn browse_wraps_at_both_ends() {
        let mut list = OptionList::new(vec!["a".into(), "b".into(), "c".into()]);
        assert!(list.browse(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)));
        assert_eq!(list.highlighted(), Some("b"));
        list.browse(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        list.browse(&KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(list.highlighted(), Some("a"), "down wraps to the top");
        list.browse(&KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        assert_eq!(list.highlighted(), Some("c"), "up wraps to the bottom");
    }
}
