use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};
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

/// The Ask screen: chat transcript pane + targets / citations side panes
/// (port of `delta/tui/screens/chat.py`, seeded golden layout).
pub fn draw_ask(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 37 at h=40
    screen.pane(
        1,
        0,
        82,
        content_bottom,
        true,
        &[
            ("5 ", Style::fg(color::BLUE).bold()),
            ("ask", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("i", "ask"), ("↑↓", "scroll"), ("z", "zoom")]),
    );
    // Targets pane (rows 1..18) and citations pane (19..37).
    screen.pane(
        83,
        0,
        118,
        18,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("targets ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("space", "toggle"), ("a", "all"), ("enter", "ask")]),
    );
    screen.pane(
        83,
        19,
        118,
        content_bottom,
        false,
        &[
            ("o ", Style::fg(color::BLUE).bold()),
            ("citations", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("←→", "citation"), ("o", "open"), ("s", "save")]),
    );

    // Transcript: scope line, empty-state line, separator, input.
    screen.text(
        3,
        1,
        "scope: apple → US:AAPL · 50+ evidence items",
        Style::fg(color::MUTED),
    );
    screen.text(
        3,
        2,
        "no messages yet — press t to pick targets, then i to ask",
        Style::fg(color::MUTED),
    );
    for x in 2..82 {
        screen.put(x, 35, '─', Style::fg(color::PANEL));
    }
    screen.put(2, 36, ' ', Style::DEFAULT);
    screen.text(3, 36, ">", Style::fg(color::BLUE));
    screen.text(4, 36, " ", Style::DEFAULT);
    screen.text(
        5,
        36,
        "ask about the targets in scope…",
        Style::fg(color::DISABLED),
    );
    screen.fill(36, 36, 82, 37, Style::fg(color::FG));

    // Targets table: header on the panel, one selected row in the dark blue.
    screen.fill(84, 1, 118, 2, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        84,
        1,
        "    Target  Kind     Evidence",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(84, 2, 118, 3, selected);
    screen.put(85, 2, '●', selected);
    screen.text(88, 2, "apple", selected);
    screen.text(96, 2, "company", selected);
    screen.text_right(108, 2, "50+", selected);
    screen.fill(84, 3, 118, 17, Style::fg(color::FG));
    screen.fill(84, 21, 118, 36, Style::fg(color::FG));
    screen.text(85, 17, "1 of 1 in scope · US:AAPL", Style::fg(color::MUTED));

    // Citations table header + session footer.
    screen.fill(
        84,
        20,
        118,
        21,
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        85,
        20,
        "#    Evidence  Kind",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        85,
        36,
        "this session: 0 answers · $0.000",
        Style::fg(color::MUTED),
    );

    draw_status_bar_ask(screen, screen.h - 1, screen.w);
}

/// Status bar with the Ask tab active (`5 Ask`).
fn draw_status_bar_ask(screen: &mut Screen, y: usize, w: usize) {
    status_bar_tabs(
        screen,
        y,
        w,
        13,
        "5 Ask",
        &[(2, "1"), (5, "2"), (8, "3"), (11, "4")],
        &[(21, "6")],
    );
}

/// Ask at 200x50: conversation pane + targets pane.
pub fn draw_ask_wide(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 47 at h=50
    screen.pane(
        1,
        0,
        162,
        content_bottom,
        true,
        &[("5 ask", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("i", "ask"), ("↑↓", "scroll"), ("z", "zoom")]),
    );
    screen.pane(
        163,
        0,
        198,
        23,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("targets ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("space", "toggle"), ("a", "all"), ("enter", "ask")]),
    );
    screen.pane(
        163,
        24,
        198,
        content_bottom,
        false,
        &[
            ("o ", Style::fg(color::BLUE).bold()),
            ("citations", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("←→", "citation"), ("o", "open"), ("s", "save")]),
    );
    let muted = Style::fg(color::MUTED);
    screen.text(3, 1, "scope: apple → US:AAPL · 50+ evidence items", muted);
    screen.text(
        3,
        2,
        "no messages yet — press t to pick targets, then i to ask",
        muted,
    );
    // Input separator and prompt line (bottom-anchored).
    for x in 2..162 {
        screen.put(x, content_bottom - 2, '─', Style::fg(color::PANEL));
    }
    screen.text(3, content_bottom - 1, ">", Style::fg(color::BLUE));
    screen.text(
        5,
        content_bottom - 1,
        "ask about the targets in scope…",
        Style::fg(color::DISABLED),
    );
    screen.fill(
        36,
        content_bottom - 1,
        162,
        content_bottom,
        Style::fg(color::FG),
    );
    // Targets table: header strip and the single selected row.
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(164, 1, 198, 2, header_style);
    screen.text(164, 1, "    Target  Kind     Evidence", header_style);
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(164, 2, 198, 3, selected);
    screen.text(164, 2, " ●  apple   company  50+", selected);
    screen.fill(164, 3, 198, 22, Style::fg(color::FG));
    screen.text(
        165,
        22,
        "1 of 1 in scope · US:AAPL",
        Style::fg(color::MUTED),
    );
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(164, 25, 198, 26, header_style);
    screen.text(164, 25, " #    Evidence  Kind", header_style);
    screen.fill(164, 26, 198, content_bottom - 1, Style::fg(color::FG));
    screen.text(
        165,
        content_bottom - 1,
        "this session: 0 answers · $0.000",
        Style::fg(color::MUTED),
    );
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "5 Ask");
}

/// Ask at 80x24: one focused full-width pane with an input strip at the
/// bottom.
pub fn draw_ask_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    let bottom = screen.h - 3; // 21 at h=24
    screen.pane(
        1,
        0,
        x1,
        bottom,
        true,
        &[("5 ask", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("i", "ask"), ("t", "targets"), ("z", "zoom")]),
    );
    let muted = Style::fg(color::MUTED);
    screen.text(3, 1, "scope: apple US:AAPL", muted);
    screen.text(
        3,
        2,
        "no messages yet — press t to pick targets, then i to ask",
        muted,
    );
    // Input separator and prompt line (bottom-anchored).
    for x in 2..x1 {
        screen.put(x, bottom - 2, '─', Style::fg(color::PANEL));
    }
    screen.text(3, bottom - 1, ">", Style::fg(color::BLUE));
    screen.text(
        5,
        bottom - 1,
        "ask about the targets in scope…",
        Style::fg(color::DISABLED),
    );
    screen.fill(36, bottom - 1, x1, bottom, Style::fg(color::FG));
    status_bar_narrow(screen, NarrowTab::Ask);
}

/// Paint mutable Ask content on top of the chrome captured by the empty goldens.
pub fn paint_ask_state(screen: &mut Screen, state: &AskState) {
    if state.history.is_empty()
        && state.input.is_empty()
        && !state.busy
        && state.error.is_none()
        && !state.clear_pending
        && !state.targets_focus
        && !state.scope_changed
    {
        return;
    }
    let narrow = screen.w < 100;
    let right = if narrow {
        screen.w.saturating_sub(2)
    } else {
        screen.w.saturating_sub(38)
    };
    let bottom = screen.h.saturating_sub(3);
    if state.scope_changed {
        let ids = state.instrument_ids();
        let names = state
            .targets
            .iter()
            .filter(|target| state.scope.contains(&target.id))
            .map(|target| target.id.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        screen.fill(3, 1, right, 2, Style::DEFAULT.bg(color::BLACK));
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
        screen.text(3, 1, &scope, Style::fg(color::MUTED));
    }
    let answers = state
        .history
        .iter()
        .filter(|turn| turn.role == "assistant")
        .count();
    let count = state.history.len();
    let title = if count == 1 {
        " · 1 message".to_string()
    } else {
        format!(" · {count} messages")
    };
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
    let title_parts = [
        ("5 ask ", Style::fg("#898989").bold()),
        (title.trim_start(), Style::fg(color::MUTED).bold()),
    ];
    screen.pane(1, 0, right, bottom, true, &title_parts, &refs);
    // Textual fades the focused Ask pane while its transcript is selected.
    for y in 0..=bottom {
        for x in [1, right] {
            let cell = &mut screen.cells[y * screen.w + x];
            cell.fg = Some("#898989");
        }
    }
    for x in 2..right {
        for y in [0, bottom] {
            let cell = &mut screen.cells[y * screen.w + x];
            if cell.fg == Some(color::BLUE) {
                cell.fg = Some("#898989");
            }
        }
    }
    if narrow && state.targets_focus {
        screen.fill(2, 1, right, bottom, Style::fg(color::FG));
        screen.text(
            3,
            1,
            "targets · space toggle · a all · esc back",
            Style::fg(color::MUTED),
        );
        for (index, target) in state
            .targets
            .iter()
            .enumerate()
            .take(bottom.saturating_sub(3))
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
                3,
                index + 3,
                &format!("{marker} {dot} {}", target.id),
                Style::fg(color::FG),
            );
        }
        return;
    }
    screen.fill(
        2,
        2,
        right,
        bottom.saturating_sub(2),
        Style::DEFAULT.bg(color::BLACK),
    );
    let mut y = 2;
    for turn in &state.history {
        if y + 2 >= bottom {
            break;
        }
        let start = y;
        if turn.role == "user" {
            screen.text(4, y, "you", Style::fg(color::MUTED).bold());
        } else {
            let mut x = screen.text(4, y, "assistant", Style::fg("#898989").bold());
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
        for block in crate::markdown::render(&turn.text, right.saturating_sub(6)) {
            y += block.margin_top;
            for line in block.lines {
                if y + 2 >= bottom {
                    break;
                }
                let runs = crate::markdown::Run::screen_runs(&line);
                let mut x = 4;
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
            let lines = crate::wrap::wrap_text(&pills, right.saturating_sub(4));
            for (line_index, line) in lines.iter().enumerate() {
                let mut x = 4;
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
                4,
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
                2,
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
        screen.text(3, y, "assistant · answering…", Style::fg(color::MUTED));
    }
    if let Some(error) = &state.error {
        screen.text(
            3,
            bottom.saturating_sub(3),
            &format!("ask failed: {error}"),
            Style::fg(color::MUTED),
        );
    }
    if state.clear_pending {
        screen.text(
            3,
            bottom.saturating_sub(3),
            "clear transcript? y confirm · esc cancel",
            Style::fg(color::MUTED),
        );
    }
    screen.fill(
        5,
        bottom.saturating_sub(1),
        right,
        bottom,
        Style::fg(color::FG),
    );
    screen.put(3, bottom.saturating_sub(1), '>', Style::fg("#898989"));
    if state.editing || !state.input.is_empty() {
        screen.text(
            5,
            bottom.saturating_sub(1),
            &state.input,
            Style::fg(color::FG),
        );
    } else {
        screen.text(
            5,
            bottom.saturating_sub(1),
            "ask about the targets in scope…",
            Style::fg(color::DISABLED),
        );
    }
    if !narrow {
        let side_x = screen.w.saturating_sub(37);
        let top_y = if screen.w >= 160 { 24 } else { 19 };
        for y in [0, top_y - 1, top_y, bottom] {
            for x in side_x + 2..screen.w.saturating_sub(2) {
                let cell = &mut screen.cells[y * screen.w + x];
                if cell.fg == Some(color::BLUE) {
                    cell.fg = Some("#898989");
                }
            }
        }
        for x in side_x + 1..screen.w.saturating_sub(2) {
            let cell = &mut screen.cells[2 * screen.w + x];
            if cell.bg == Some("#13254b") {
                cell.bg = Some("#242424");
            }
        }
        let cite_x = screen.w.saturating_sub(36);
        let cite_y = if screen.w >= 160 { 26 } else { 21 };
        let cite_bottom = bottom.saturating_sub(1);
        let header = Style::fg(color::FG).bold().bg(color::PANEL);
        screen.fill(
            cite_x,
            cite_y - 1,
            screen.w.saturating_sub(2),
            cite_y,
            header,
        );
        screen.text(cite_x, cite_y - 1, " #    Evidence", header);
        screen.fill(
            cite_x,
            cite_y,
            screen.w.saturating_sub(2),
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
                screen.w.saturating_sub(2),
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
            screen.w.saturating_sub(2),
            cite_bottom,
            Style::fg(color::FG).bg("#0d0d0d"),
        );
        screen.fill(
            cite_x,
            bottom - 1,
            screen.w.saturating_sub(2),
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
