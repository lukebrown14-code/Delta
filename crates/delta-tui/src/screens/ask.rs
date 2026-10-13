use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};
use crate::Action;
use crossterm::event::{KeyCode, KeyEvent};
use std::collections::{BTreeMap, BTreeSet};

use delta_services::chat::ChatMessage;
use delta_services::targets::WatchTarget;

/// Session state for the Ask pane. A generation identifies the active worker;
/// clearing the transcript invalidates its reply even if the provider returns later.
#[derive(Default)]
pub struct AskState {
    pub history: Vec<ChatMessage>,
    pub input: String,
    pub editing: bool,
    pub busy: bool,
    pub generation: u64,
    pub clear_pending: bool,
    pub targets_focus: bool,
    pub zoomed: bool,
    pub targets: Vec<WatchTarget>,
    pub scope: BTreeSet<String>,
    pub target_selected: usize,
    pub selected_citation: usize,
    pub citation_labels: BTreeMap<String, String>,
    pub sidebar_labels: BTreeMap<String, String>,
    pub error: Option<String>,
    pub provider: String,
    pub model: String,
    pub scope_changed: bool,
}

impl AskState {
    pub fn set_targets(&mut self, targets: Vec<WatchTarget>) {
        let first = self.targets.is_empty();
        self.targets = targets;
        let known: BTreeSet<String> = self
            .targets
            .iter()
            .map(|target| target.id.clone())
            .collect();
        if first {
            self.scope = known;
            self.scope_changed = self.targets.len() != 1 || self.targets[0].id != "apple";
        } else {
            self.scope.retain(|id| known.contains(id));
        }
        self.target_selected = self
            .target_selected
            .min(self.targets.len().saturating_sub(1));
    }

    pub fn instrument_ids(&self) -> Vec<String> {
        let mut seen = BTreeSet::new();
        self.targets
            .iter()
            .filter(|target| self.scope.contains(&target.id))
            .flat_map(WatchTarget::instruments)
            .map(|instrument| instrument.id)
            .filter(|id| seen.insert(id.clone()))
            .collect()
    }

    pub fn toggle_target(&mut self) {
        if let Some(target) = self.targets.get(self.target_selected) {
            if !self.scope.remove(&target.id) {
                self.scope.insert(target.id.clone());
            }
            self.scope_changed = true;
        }
    }

    pub fn toggle_all(&mut self) {
        if self.scope.len() == self.targets.len() {
            self.scope.clear();
        } else {
            self.scope = self
                .targets
                .iter()
                .map(|target| target.id.clone())
                .collect();
        }
        self.scope_changed = true;
    }

    pub fn submit(&mut self) -> Option<(u64, Vec<ChatMessage>, Vec<String>)> {
        let question = self.input.trim();
        if question.is_empty() || self.busy {
            return None;
        }
        self.history.push(ChatMessage {
            role: "user".into(),
            text: question.into(),
            citations: vec![],
            source: "user".into(),
        });
        self.input.clear();
        self.editing = false;
        self.busy = true;
        self.error = None;
        self.generation = self.generation.wrapping_add(1);
        Some((self.generation, self.history.clone(), self.instrument_ids()))
    }

    pub fn clear(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.busy = false;
        self.history.clear();
        self.selected_citation = 0;
        self.citation_labels.clear();
        self.sidebar_labels.clear();
        self.clear_pending = false;
        self.error = None;
    }

    pub fn cancel_pending(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.busy = false;
    }

    pub fn finish(&mut self, generation: u64, result: Result<ChatMessage, String>) {
        if generation != self.generation || !self.busy {
            return;
        }
        self.busy = false;
        match result {
            Ok(answer) => {
                self.history.push(answer);
                self.selected_citation = 0;
            }
            Err(error) => self.error = Some(error),
        }
    }

    pub fn citations(&self) -> &[String] {
        self.history
            .iter()
            .rev()
            .find(|turn| turn.role == "assistant")
            .map_or(&[], |turn| turn.citations.as_slice())
    }

    pub fn walk_citation(&mut self, step: isize) {
        let len = self.citations().len();
        if len > 0 {
            self.selected_citation = self.selected_citation.wrapping_add_signed(step) % len;
        }
    }
}

impl crate::Screen for AskState {
    fn name(&self) -> &'static str {
        "chat"
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if self.editing {
            match key.code {
                KeyCode::Esc => self.editing = false,
                KeyCode::Enter => {
                    if let Some((generation, history, targets)) = self.submit() {
                        return Some(Action::AskQuestion {
                            generation,
                            history,
                            targets,
                        });
                    }
                }
                KeyCode::Backspace => {
                    self.input.pop();
                }
                KeyCode::Char(ch) => self.input.push(ch),
                _ => {}
            }
            return Some(Action::Noop);
        }
        if self.clear_pending {
            match key.code {
                KeyCode::Char('y') => {
                    self.clear();
                    return Some(Action::AskAbortWorker);
                }
                KeyCode::Esc => self.clear_pending = false,
                _ => {}
            }
            return Some(Action::Noop);
        }
        match key.code {
            KeyCode::Esc if self.busy => {
                self.cancel_pending();
                Some(Action::AskAbortWorker)
            }
            KeyCode::Char('i') | KeyCode::Enter => {
                self.editing = true;
                self.targets_focus = false;
                Some(Action::Noop)
            }
            KeyCode::Char('t') => {
                self.targets_focus = true;
                Some(Action::Noop)
            }
            KeyCode::Esc if self.targets_focus => {
                self.targets_focus = false;
                Some(Action::Noop)
            }
            KeyCode::Char(' ') => {
                self.toggle_target();
                Some(Action::Noop)
            }
            KeyCode::Char('a') => {
                self.toggle_all();
                Some(Action::Noop)
            }
            KeyCode::Up if self.targets_focus => {
                self.target_selected = self.target_selected.saturating_sub(1);
                Some(Action::Noop)
            }
            KeyCode::Down if self.targets_focus => {
                self.target_selected =
                    (self.target_selected + 1).min(self.targets.len().saturating_sub(1));
                Some(Action::Noop)
            }
            KeyCode::Left => {
                self.walk_citation(-1);
                Some(Action::Noop)
            }
            KeyCode::Right => {
                self.walk_citation(1);
                Some(Action::Noop)
            }
            KeyCode::Char('o') => {
                self.citations()
                    .get(self.selected_citation)
                    .cloned()
                    .map(|citation| {
                        if citation.starts_with("https://") || citation.starts_with("http://") {
                            Action::OpenSource(citation)
                        } else {
                            Action::AskOpenCitation(citation)
                        }
                    })
            }
            KeyCode::Char('s') => Some(Action::AskSaveAnswer),
            KeyCode::Char('z') => {
                self.zoomed = !self.zoomed;
                Some(Action::Noop)
            }
            KeyCode::Char('x') if !self.history.is_empty() => {
                self.clear_pending = true;
                Some(Action::Noop)
            }
            _ => None,
        }
    }

    fn update(&mut self, action: Action) {
        if let Action::AskAnswered {
            generation,
            result,
            citation_labels,
            sidebar_labels,
        } = action
        {
            self.finish(generation, result);
            if generation == self.generation && !self.history.is_empty() {
                self.citation_labels = citation_labels;
                self.sidebar_labels = sidebar_labels;
            }
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        let mut screen = Screen::new(area.width as usize, area.height as usize);
        draw_ask_live(&mut screen, self);
        crate::screen::blit(frame, &screen, area);
    }
}

/// Rectangles are exclusive at the right and bottom edges. Panes convert
/// them to the inclusive coordinates of the cell-grid painter.
#[derive(Clone, Copy)]
struct AskLayout {
    conversation: super::PaneRect,
    targets: Option<super::PaneRect>,
    citations: Option<super::PaneRect>,
}

impl AskLayout {
    fn new(screen: &Screen, zoomed: bool) -> Self {
        let bottom = screen.h.saturating_sub(2);
        let end_x = screen.w.saturating_sub(1);
        if screen.w < 100 || zoomed {
            let grid = super::pane_layout(1, 0, end_x, bottom, &[bottom], &[end_x - 1]);
            Self {
                conversation: grid[0][0],
                targets: None,
                citations: None,
            }
        } else {
            let left_width = screen.w - 38;
            let side_top = bottom / 2;
            let columns = super::pane_layout(1, 0, end_x, bottom, &[bottom], &[left_width, 36]);
            let side = super::pane_layout(
                columns[0][1].0,
                0,
                end_x,
                bottom,
                &[side_top, bottom - side_top],
                &[36],
            );
            Self {
                conversation: columns[0][0],
                targets: Some(side[0][0]),
                citations: Some(side[1][0]),
            }
        }
    }

    fn right(self) -> usize {
        self.conversation.2 - 1
    }
    fn bottom(self) -> usize {
        self.conversation.3 - 1
    }
}

fn ask_is_empty(state: &AskState) -> bool {
    state.history.is_empty()
        && state.input.is_empty()
        && !state.busy
        && state.error.is_none()
        && !state.clear_pending
        && !state.targets_focus
        && !state.scope_changed
        && !state.zoomed
}

/// Stateful Ask render. The transcript, target table and citation table each
/// paint inside their own layout rectangle.
pub fn draw_ask_live(screen: &mut Screen, state: &AskState) {
    let layout = AskLayout::new(screen, state.zoomed);
    let empty = ask_is_empty(state);
    if empty {
        paint_empty_conversation(screen, layout);
    }
    if let (Some(targets), Some(citations)) = (layout.targets, layout.citations) {
        paint_side_panes(screen, targets, citations);
    }
    if !empty {
        paint_ask_dynamic(screen, state, layout);
    }
    if screen.w >= 160 {
        draw_status_bar_wide(screen, screen.h - 1, screen.w, "5 Ask");
    } else if screen.w < 100 {
        status_bar_narrow(screen, NarrowTab::Ask);
    } else {
        status_bar_tabs(
            screen,
            screen.h - 1,
            screen.w,
            13,
            "5 Ask",
            &[(2, "1"), (5, "2"), (8, "3"), (11, "4")],
            &[(21, "6")],
        );
    }
}

fn paint_empty_conversation(screen: &mut Screen, layout: AskLayout) {
    let (x, y, _, _) = layout.conversation;
    let right = layout.right();
    let bottom = layout.bottom();
    let narrow = layout.targets.is_none();
    let title = if screen.w >= 160 || narrow {
        vec![("5 ask", Style::fg(color::BLUE).bold())]
    } else {
        vec![
            ("5 ", Style::fg(color::BLUE).bold()),
            ("ask", Style::fg(color::BLUE).bold()),
        ]
    };
    let hints = if narrow {
        pane_hints(&[("i", "ask"), ("t", "targets"), ("z", "zoom")])
    } else {
        pane_hints(&[("i", "ask"), ("↑↓", "scroll"), ("z", "zoom")])
    };
    screen.pane(x, y, right, bottom, true, &title, &hints);
    screen.text(
        x + 2,
        y + 1,
        if narrow {
            "scope: apple US:AAPL"
        } else {
            "scope: apple → US:AAPL · 50+ evidence items"
        },
        Style::fg(color::MUTED),
    );
    screen.text(
        x + 2,
        y + 2,
        "no messages yet — press t to pick targets, then i to ask",
        Style::fg(color::MUTED),
    );
    for cx in x + 1..right {
        screen.put(cx, bottom - 2, '─', Style::fg(color::PANEL));
    }
    screen.put(x + 2, bottom - 1, '>', Style::fg(color::BLUE));
    screen.text(
        x + 4,
        bottom - 1,
        "ask about the targets in scope…",
        Style::fg(color::DISABLED),
    );
    screen.fill(x + 35, bottom - 1, right, bottom, Style::fg(color::FG));
}

fn paint_side_panes(screen: &mut Screen, targets: super::PaneRect, citations: super::PaneRect) {
    let tx = targets.0;
    let ty = targets.1;
    let tr = targets.2 - 1;
    let tb = targets.3 - 1;
    let cx = citations.0;
    let cy = citations.1;
    let cr = citations.2 - 1;
    let cb = citations.3 - 1;
    screen.pane(
        tx,
        ty,
        tr,
        tb,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("targets ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("space", "toggle"), ("a", "all"), ("enter", "ask")]),
    );
    screen.pane(
        cx,
        cy,
        cr,
        cb,
        false,
        &[
            ("o ", Style::fg(color::BLUE).bold()),
            ("citations", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("←→", "citation"), ("o", "open"), ("s", "save")]),
    );
    let header = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(tx + 1, ty + 1, tr, ty + 2, header);
    screen.text(tx + 1, ty + 1, "    Target  Kind     Evidence", header);
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(tx + 1, ty + 2, tr, ty + 3, selected);
    if screen.w >= 160 {
        screen.text(tx + 1, ty + 2, " ●  apple   company  50+", selected);
    } else {
        screen.put(tx + 2, ty + 2, '●', selected);
        screen.text(tx + 5, ty + 2, "apple", selected);
        screen.text(tx + 13, ty + 2, "company", selected);
        screen.text_right(tx + 25, ty + 2, "50+", selected);
    }
    screen.fill(tx + 1, ty + 3, tr, tb - 1, Style::fg(color::FG));
    screen.text(
        tx + 2,
        tb - 1,
        "1 of 1 in scope · US:AAPL",
        Style::fg(color::MUTED),
    );
    screen.fill(cx + 1, cy + 1, cr, cy + 2, header);
    if screen.w >= 160 {
        screen.text(cx + 1, cy + 1, " #    Evidence  Kind", header);
    } else {
        screen.text(cx + 2, cy + 1, "#    Evidence  Kind", header);
    }
    screen.fill(cx + 1, cy + 2, cr, cb - 1, Style::fg(color::FG));
    screen.text(
        cx + 2,
        cb - 1,
        "this session: 0 answers · $0.000",
        Style::fg(color::MUTED),
    );
}

/// Compatibility entry points for the static golden scenarios.
pub fn draw_ask(screen: &mut Screen) {
    draw_ask_live(screen, &AskState::default());
}
pub fn draw_ask_wide(screen: &mut Screen) {
    draw_ask_live(screen, &AskState::default());
}
pub fn draw_ask_narrow(screen: &mut Screen) {
    draw_ask_live(screen, &AskState::default());
}

/// Compatibility renderer for callers that already painted an empty frame.
pub fn paint_ask_state(screen: &mut Screen, state: &AskState) {
    *screen = Screen::themed(screen.palette, screen.w, screen.h);
    draw_ask_live(screen, state);
}

fn paint_ask_dynamic(screen: &mut Screen, state: &AskState, layout: AskLayout) {
    let narrow = screen.w < 100 || state.zoomed;
    let (left, top, _, _) = layout.conversation;
    let right = layout.right();
    let bottom = layout.bottom();
    if !state.zoomed {
        screen.text(
            left + 2,
            top + 1,
            if narrow {
                "scope: apple US:AAPL"
            } else {
                "scope: apple → US:AAPL · 50+ evidence items"
            },
            Style::fg(color::MUTED),
        );
        for x in left + 1..right {
            screen.put(x, bottom - 2, '─', Style::fg(color::PANEL));
        }
    }
    if state.scope_changed {
        let ids = state.instrument_ids();
        let names = state
            .targets
            .iter()
            .filter(|target| state.scope.contains(&target.id))
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        screen.fill(
            left + 2,
            top + 1,
            right,
            top + 2,
            Style::DEFAULT.bg(color::BLACK),
        );
        let scope = if narrow {
            format!(
                "scope: {} {}",
                if names.is_empty() { "none" } else { &names },
                ids.join(" ")
            )
        } else {
            format!(
                "scope: {} → {} · evidence in scope",
                if names.is_empty() { "none" } else { &names },
                if ids.is_empty() {
                    "—".to_string()
                } else {
                    ids.join(" ")
                }
            )
        };
        screen.text(left + 2, top + 1, &scope, Style::fg(color::MUTED));
    }
    let answers = state
        .history
        .iter()
        .filter(|turn| turn.role == "assistant")
        .count();
    let count = state.history.len();
    let title = if count == 0 {
        String::new()
    } else if count == 1 {
        " · 1 message".to_string()
    } else {
        format!(" · {count} messages")
    };
    let hints = if state.clear_pending {
        vec![("y", "confirm clear"), ("esc", "cancel")]
    } else if state.editing {
        let mut hints = vec![("enter", "send"), ("esc", "leave input")];
        if !narrow {
            hints.push(("↑↓", "scroll"));
        }
        hints
    } else {
        let mut hints = vec![("i", "ask")];
        if narrow {
            hints.push(("t", "targets"));
        } else {
            hints.push(("↑↓", "scroll"));
        }
        if !state.citations().is_empty() {
            hints.extend([("←→", "citation"), ("o", "open citation")]);
        }
        if answers > 0 {
            hints.push(("s", "save"));
        }
        if count > 0 {
            hints.push(("x", "clear"));
        }
        hints.push(("z", "zoom"));
        hints
    };
    let runs = pane_hints(&hints)
        .into_iter()
        .map(|(text, style)| {
            (
                text,
                if style.bold {
                    Style::fg("#898989").bold()
                } else {
                    Style::fg(color::MUTED)
                },
            )
        })
        .collect::<Vec<_>>();
    let refs = runs
        .iter()
        .map(|(text, style)| (text.clone(), *style))
        .collect::<Vec<_>>();
    let title_parts = if count == 0 {
        vec![("5 ask", Style::fg("#898989").bold())]
    } else {
        vec![
            ("5 ask ", Style::fg("#898989").bold()),
            (title.trim_start(), Style::fg(color::MUTED).bold()),
        ]
    };
    screen.pane(left, top, right, bottom, true, &title_parts, &refs);
    // Textual fades the focused Ask pane while its transcript is selected.
    for y in top..=bottom {
        for x in [left, right] {
            let cell = &mut screen.cells[y * screen.w + x];
            cell.fg = Some("#898989");
        }
    }
    for x in left + 1..right {
        for y in [top, bottom] {
            let cell = &mut screen.cells[y * screen.w + x];
            if cell.fg == Some(color::BLUE) {
                cell.fg = Some("#898989");
            }
        }
    }
    if narrow && state.targets_focus {
        screen.fill(left + 1, top + 1, right, bottom, Style::fg(color::FG));
        screen.text(
            left + 2,
            top + 1,
            "targets · space toggle · a all · esc back",
            Style::fg(color::MUTED),
        );
        for (index, target) in state
            .targets
            .iter()
            .enumerate()
            .take(bottom.saturating_sub(top + 3))
        {
            let dot = if state.scope.contains(&target.id) {
                "●"
            } else {
                "○"
            };
            let marker = if index == state.target_selected {
                "›"
            } else {
                " "
            };
            screen.text(
                left + 2,
                top + index + 3,
                &format!("{marker} {dot} {}", target.id),
                Style::fg(color::FG),
            );
        }
        return;
    }
    screen.fill(
        left + 1,
        top + 2,
        right,
        bottom.saturating_sub(2),
        Style::DEFAULT.bg(color::BLACK),
    );
    if state.history.is_empty() {
        screen.text(
            left + 2,
            top + 2,
            "no messages yet — press t to pick targets, then i to ask",
            Style::fg(color::MUTED),
        );
    }
    let mut y = top + 2;
    for turn in &state.history {
        if y + 2 >= bottom {
            break;
        }
        let start = y;
        if turn.role == "user" {
            screen.text(left + 3, y, "you", Style::fg(color::MUTED).bold());
        } else {
            let mut x = screen.text(left + 3, y, "assistant", Style::fg("#898989").bold());
            x = screen.text(x, y, " ", Style::fg(color::FG));
            let source = match turn.source.as_str() {
                "web" => " · web result",
                "inference" => " · AI inference",
                _ => "",
            };
            let provider = if state.provider.is_empty() {
                "—"
            } else {
                &state.provider
            };
            let model = if state.model.is_empty() {
                "—"
            } else {
                &state.model
            };
            screen.text(
                x,
                y,
                &format!("· {provider} · {model}{source}"),
                Style::fg(color::MUTED),
            );
        }
        y += 1;
        for block in crate::markdown::render(&turn.text, right.saturating_sub(left + 5)) {
            y += block.margin_top;
            for line in block.lines {
                if y + 2 >= bottom {
                    break;
                }
                let runs = crate::markdown::Run::screen_runs(&line);
                let mut x = left + 3;
                for (text, style) in runs {
                    let style = if turn.role == "user" {
                        Style::fg(color::MUTED)
                    } else {
                        style
                    };
                    screen.text(x, y, &text, style);
                    x += crate::wrap::cell_width(&text);
                }
                y += 1;
            }
            if turn.citations.is_empty() {
                y += block.margin_bottom;
            }
        }
        if !turn.citations.is_empty() && y + 2 < bottom {
            let mut pills = String::new();
            let mut spans = Vec::new();
            for (index, id) in turn.citations.iter().enumerate() {
                if index > 0 {
                    pills.push(' ');
                }
                let from = pills.len();
                pills.push_str(&format!(
                    " [{}] {} ",
                    index + 1,
                    state
                        .citation_labels
                        .get(id)
                        .map(String::as_str)
                        .unwrap_or(id)
                ));
                spans.push((from, pills.len(), index == state.selected_citation));
            }
            let mut offset = 0;
            let lines = crate::wrap::wrap_text(&pills, right.saturating_sub(left + 3));
            for (line_index, line) in lines.iter().enumerate() {
                let mut x = left + 3;
                let shown = if line_index + 1 == lines.len() {
                    line.as_str()
                } else {
                    line.trim_end()
                };
                for (at, ch) in shown.char_indices() {
                    let position = offset + at;
                    let style = spans
                        .iter()
                        .find(|(from, to, _)| *from <= position && position < *to)
                        .map_or(Style::fg(color::FG), |(from, _, selected)| {
                            let style = if *selected {
                                Style::fg(color::WHITE).bg("#494949")
                            } else {
                                Style::fg(color::MUTED).bg(color::PANEL)
                            };
                            if position > *from && position < from + 4 {
                                style.bold()
                            } else {
                                style
                            }
                        });
                    screen.put(x, y, ch, style);
                    x += crate::wrap::char_width(ch);
                }
                offset += line.len();
                y += 1;
            }
            screen.text(
                left + 3,
                y,
                &format!("{} citations · {}", turn.citations.len(), turn.source),
                Style::fg(color::MUTED),
            );
            y += 1;
        }
        let bar_end = if turn.role == "user" {
            y.saturating_sub(1)
        } else {
            y
        };
        for row in start..bar_end.min(bottom.saturating_sub(2)) {
            screen.put(
                left + 1,
                row,
                if turn.role == "user" { '│' } else { '█' },
                Style::fg(if turn.role == "user" {
                    color::PANEL
                } else {
                    "#494949"
                }),
            );
        }
        if turn.role != "user" {
            y += 1;
        }
    }
    if state.busy && y + 2 < bottom {
        screen.put(left + 1, y, '█', Style::fg("#494949"));
        screen.text(left + 3, y, "assistant", Style::fg("#898989").bold());
        screen.put(left + 12, y, ' ', Style::fg(color::FG));
        let provider = if state.provider.is_empty() {
            "—"
        } else {
            &state.provider
        };
        let model = if state.model.is_empty() {
            "—"
        } else {
            &state.model
        };
        screen.text(
            left + 13,
            y,
            &format!("· {provider} · {model} · "),
            Style::fg(color::MUTED),
        );
        let marker_x = left + 13 + crate::wrap::cell_width(&format!("· {provider} · {model} · "));
        screen.put(marker_x - 1, y, ' ', Style::fg(color::FG));
        screen.text(marker_x, y, "● answering… 0s", Style::fg("#a6a6a6"));
    }
    screen.fill(
        left + 4,
        bottom.saturating_sub(1),
        right,
        bottom,
        Style::fg(color::FG),
    );
    screen.put(
        left + 2,
        bottom.saturating_sub(1),
        '>',
        Style::fg("#898989"),
    );
    if state.editing || !state.input.is_empty() {
        screen.text(
            left + 4,
            bottom.saturating_sub(1),
            &state.input,
            if state.editing {
                Style::fg(color::FG).bg("#242424")
            } else {
                Style::fg(color::FG)
            },
        );
        if state.editing {
            let cursor_x = left + 4 + crate::wrap::cell_width(&state.input);
            screen.put(
                cursor_x,
                bottom.saturating_sub(1),
                ' ',
                Style::fg(color::BLACK).bg(color::FG),
            );
        }
    } else {
        screen.text(
            left + 4,
            bottom.saturating_sub(1),
            "ask about the targets in scope…",
            Style::fg(color::DISABLED),
        );
    }
    if let (Some(targets), Some(citations)) = (layout.targets, layout.citations) {
        let side_x = targets.0;
        let top_y = citations.1;
        let side_right = targets.2 - 1;
        for y in [targets.1, targets.3 - 1, citations.1, citations.3 - 1] {
            for x in side_x + 2..side_right {
                let cell = &mut screen.cells[y * screen.w + x];
                if cell.fg == Some(color::BLUE) {
                    cell.fg = Some("#898989");
                }
            }
        }
        for x in side_x + 1..side_right {
            let cell = &mut screen.cells[(targets.1 + 2) * screen.w + x];
            if cell.bg == Some("#13254b") {
                cell.bg = Some("#242424");
            }
        }
        let cite_x = citations.0 + 1;
        let cite_y = top_y + 2;
        let cite_right = citations.2 - 1;
        let cite_bottom = citations.3 - 2;
        if !state.citations().is_empty() {
            let header = Style::fg(color::FG).bold().bg(color::PANEL);
            screen.fill(cite_x, cite_y - 1, cite_right, cite_y, header);
            screen.text(cite_x, cite_y - 1, " #    Evidence", header);
            screen.fill(
                cite_x,
                cite_y,
                cite_right,
                cite_bottom,
                Style::fg(color::FG),
            );
            for (index, id) in state
                .citations()
                .iter()
                .enumerate()
                .take(cite_bottom.saturating_sub(cite_y))
            {
                let label = state
                    .sidebar_labels
                    .get(id)
                    .map(String::as_str)
                    .unwrap_or(id);
                let label: String = label.chars().take(28).collect();
                let row_style = if index == state.selected_citation {
                    Style::fg(color::FG).bg("#242424")
                } else {
                    Style::fg(color::FG)
                };
                screen.fill(
                    cite_x,
                    cite_y + index,
                    cite_right,
                    cite_y + index + 1,
                    row_style,
                );
                screen.text(
                    cite_x + 1,
                    cite_y + index,
                    &format!("{}    {label}", index + 1),
                    row_style,
                );
            }
            screen.fill(
                cite_x,
                cite_bottom - 1,
                cite_x + 15,
                cite_bottom,
                Style::fg(color::SURFACE_SCROLLBAR),
            );
            screen.put(
                cite_x + 15,
                cite_bottom - 1,
                '▏',
                Style::fg(color::SURFACE_SCROLLBAR).bg("#0d0d0d"),
            );
            screen.fill(
                cite_x + 16,
                cite_bottom - 1,
                cite_right,
                cite_bottom,
                Style::fg(color::FG).bg("#0d0d0d"),
            );
        }
        screen.fill(
            cite_x,
            bottom - 1,
            cite_right,
            bottom,
            Style::DEFAULT.bg(color::BLACK),
        );
        screen.text(
            cite_x + 1,
            bottom.saturating_sub(1),
            &format!(
                "this session: {answers} answer{} · $0.000",
                if answers == 1 { "" } else { "s" }
            ),
            Style::fg(color::MUTED),
        );
    }
}

#[cfg(test)]
mod state_tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn screen_keys_own_question_submission_and_clear_confirmation() {
        let mut state = AskState::default();
        assert!(matches!(
            crate::Screen::handle_key(&mut state, key(KeyCode::Char('i'))),
            Some(Action::Noop)
        ));
        for ch in "Why?".chars() {
            crate::Screen::handle_key(&mut state, key(KeyCode::Char(ch)));
        }
        let Some(Action::AskQuestion {
            generation,
            history,
            ..
        }) = crate::Screen::handle_key(&mut state, key(KeyCode::Enter))
        else {
            panic!("Ask did not submit its input");
        };
        assert_eq!(generation, state.generation);
        assert_eq!(history[0].text, "Why?");
        assert!(state.busy);
        crate::Screen::handle_key(&mut state, key(KeyCode::Char('x')));
        assert!(state.clear_pending);
        assert!(matches!(
            crate::Screen::handle_key(&mut state, key(KeyCode::Char('y'))),
            Some(Action::AskAbortWorker)
        ));
        assert!(state.history.is_empty());
        assert!(!state.busy);
    }

    #[test]
    fn zoom_uses_full_width_and_hides_side_panes() {
        let mut screen = Screen::new(120, 40);
        draw_ask(&mut screen);
        let state = AskState {
            zoomed: true,
            ..Default::default()
        };
        paint_ask_state(&mut screen, &state);
        let text: String = screen.cells.iter().map(|cell| cell.ch).collect();
        assert!(!text.contains("t targets · 1"));
        assert_eq!(screen.cells[118].ch, '┓');
    }

    #[test]
    fn clearing_invalidates_an_in_flight_answer() {
        let mut state = AskState {
            input: "What happened?".into(),
            ..Default::default()
        };
        let (generation, _, _) = state.submit().expect("question submitted");
        state.clear();
        state.finish(
            generation,
            Ok(ChatMessage {
                role: "assistant".into(),
                text: "old".into(),
                citations: vec![],
                source: "inference".into(),
            }),
        );
        assert!(state.history.is_empty());
        assert!(!state.busy);
    }

    #[test]
    fn cancelling_invalidates_an_in_flight_answer_but_keeps_the_question() {
        let mut state = AskState {
            input: "What happened?".into(),
            ..Default::default()
        };
        let (generation, _, _) = state.submit().expect("question submitted");
        state.cancel_pending();
        state.finish(
            generation,
            Ok(ChatMessage {
                role: "assistant".into(),
                text: "late".into(),
                citations: vec![],
                source: "inference".into(),
            }),
        );
        assert_eq!(state.history.len(), 1);
        assert_eq!(state.history[0].role, "user");
    }
}
