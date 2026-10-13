use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};
use crate::Action;
use crossterm::event::{KeyCode, KeyEvent};
use delta_services::{Decision, DecisionReview};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionCommand {
    SelectionChanged,
    New,
    Edit,
    Review,
    Filter,
    ConfirmDelete,
    Delete,
    Research,
    Cancel,
}

#[derive(Default)]
pub struct DecisionsScreen {
    data: Option<DecisionsData>,
    confirm_delete: bool,
}

impl DecisionsScreen {
    pub fn data(&self) -> Option<&DecisionsData> {
        self.data.as_ref()
    }

    pub fn selected(&self) -> Option<&Decision> {
        self.data.as_ref()?.selected()
    }

    pub fn filter(&self) -> &str {
        self.data.as_ref().map_or("", |data| data.filter.as_str())
    }

    pub fn selected_index(&self) -> usize {
        self.data.as_ref().map_or(0, |data| data.selected)
    }

    pub fn set_data(&mut self, mut data: Option<DecisionsData>) {
        if let Some(data) = &mut data {
            data.confirm_delete = self.confirm_delete;
        }
        self.data = data;
    }

    pub fn set_filter(&mut self, filter: String) {
        if let Some(data) = &mut self.data {
            data.filter = filter;
            data.selected = 0;
        }
    }

    pub fn paint(&self, screen: &mut Screen) {
        let empty = DecisionsData {
            decisions: Vec::new(),
            selected: 0,
            reviews: Vec::new(),
            current_price: None,
            filter: String::new(),
            spend: 0.0,
            confirm_delete: false,
        };
        let data = self.data.as_ref().unwrap_or(&empty);
        draw_decisions_frame(screen, data, data.decisions.is_empty());
    }
}

impl crate::Screen for DecisionsScreen {
    fn name(&self) -> &'static str {
        "decisions"
    }

    fn paint_into(&self, screen: &mut Screen) {
        self.paint(screen);
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let command = match key.code {
            KeyCode::Up | KeyCode::Down => {
                if let Some(data) = &mut self.data {
                    data.selected = if key.code == KeyCode::Up {
                        data.selected.saturating_sub(1)
                    } else {
                        (data.selected + 1).min(data.decisions.len().saturating_sub(1))
                    };
                }
                DecisionCommand::SelectionChanged
            }
            KeyCode::Char('n') => DecisionCommand::New,
            KeyCode::Char('e') if self.selected().is_some() => DecisionCommand::Edit,
            KeyCode::Char('r') if self.selected().is_some() => DecisionCommand::Review,
            KeyCode::Char('/') => DecisionCommand::Filter,
            KeyCode::Char('d') if self.selected().is_some() => {
                self.confirm_delete = true;
                if let Some(data) = &mut self.data {
                    data.confirm_delete = true;
                }
                DecisionCommand::ConfirmDelete
            }
            KeyCode::Char('y') if self.confirm_delete => {
                self.confirm_delete = false;
                if let Some(data) = &mut self.data {
                    data.confirm_delete = false;
                }
                DecisionCommand::Delete
            }
            KeyCode::Char('o') if self.selected().is_some() => DecisionCommand::Research,
            KeyCode::Esc => {
                self.confirm_delete = false;
                if let Some(data) = &mut self.data {
                    data.confirm_delete = false;
                }
                DecisionCommand::Cancel
            }
            _ => return None,
        };
        Some(Action::Decision(command))
    }
}

/// Data supplied by the application service layer for the decision journal.
pub struct DecisionsData {
    pub decisions: Vec<Decision>,
    pub selected: usize,
    pub reviews: Vec<DecisionReview>,
    pub current_price: Option<f64>,
    pub filter: String,
    pub spend: f64,
    pub confirm_delete: bool,
}

impl DecisionsData {
    pub fn selected(&self) -> Option<&Decision> {
        self.decisions.get(self.selected)
    }
}

/// Textual's journal/timeline split, expressed as two pane rectangles.
/// The one-cell adjustment at normal and wide sizes is the border shared by
/// the panes; the narrow table keeps that cell for its horizontal scrollbar.
fn pane_columns(screen: &Screen) -> (super::PaneRect, super::PaneRect) {
    let bottom = screen.h.saturating_sub(3);
    let journal_width = screen.w.saturating_mul(2) / 5 - usize::from(screen.w >= 100);
    let panes = super::pane_layout(
        1,
        0,
        screen.w.saturating_sub(1),
        bottom + 1,
        &[bottom + 1],
        &[journal_width, screen.w.saturating_sub(2 + journal_width)],
    );
    (panes[0][0], panes[0][1])
}

/// Paint the populated journal on the same cell-grid chrome as the empty state.
pub fn draw_decisions_live(screen: &mut Screen, data: &DecisionsData) {
    draw_decisions_frame(screen, data, false);
}

fn draw_decisions_frame(screen: &mut Screen, data: &DecisionsData, component_empty: bool) {
    let (journal, timeline) = pane_columns(screen);
    let (left, right, bottom) = (journal.2 - 1, timeline.0, journal.3 - 1);
    if screen.w >= 160 {
        draw_status_bar_wide(screen, screen.h - 1, screen.w, "6 Decisions");
    } else if screen.w < 100 {
        status_bar_narrow(screen, NarrowTab::Decisions);
    } else {
        draw_status_bar_decisions(screen, screen.h - 1, screen.w);
    }
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(
        journal.0 + 1,
        journal.1 + 1,
        left,
        journal.1 + 2,
        header_style,
    );
    let header = if screen.w < 100 {
        " status      review      instr"
    } else if screen.w >= 160 {
        " status      review      instrument      rationale"
    } else {
        " status      review      instrument      rati"
    };
    screen.text(journal.0 + 1, journal.1 + 1, header, header_style);
    let title = Style::fg(color::BLUE).bold();
    let count = format!("· {}", data.decisions.len());
    screen.pane(
        journal.0,
        journal.1,
        left,
        bottom,
        true,
        &[
            ("decisions ", title),
            (&count, Style::fg(color::MUTED).bold()),
        ],
        &if screen.w < 100 {
            vec![
                (" ".to_string(), Style::fg(color::MUTED)),
                ("n".to_string(), Style::fg(color::BLUE).bold()),
                (" new".to_string(), Style::fg(color::MUTED)),
                ("  ".to_string(), Style::fg(color::MUTED)),
                ("e".to_string(), Style::fg(color::BLUE).bold()),
                (" edit".to_string(), Style::fg(color::MUTED)),
                ("  ".to_string(), Style::fg(color::MUTED)),
                ("d".to_string(), Style::fg(color::BLUE).bold()),
                (" delete".to_string(), Style::fg(color::MUTED)),
                ("  ".to_string(), Style::fg(color::MUTED)),
                ("…".to_string(), Style::fg(color::BLUE).bold()),
                (" ".to_string(), Style::fg(color::MUTED)),
            ]
        } else {
            pane_hints(&[
                ("n", "new"),
                ("e", "edit"),
                ("d", "delete"),
                ("/", "filter"),
            ])
        },
    );
    let review_count = if data.decisions.is_empty() {
        String::new()
    } else {
        format!(" · {} reviews", data.reviews.len())
    };
    screen.pane(
        timeline.0,
        timeline.1,
        timeline.2 - 1,
        bottom,
        false,
        &[
            ("timeline", title),
            (&review_count, Style::fg(color::MUTED).bold()),
        ],
        &if data.confirm_delete {
            pane_hints(&[("y", "confirm delete"), ("esc", "cancel")])
        } else {
            pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")])
        },
    );
    if component_empty {
        paint_empty_decisions(screen, journal, timeline, true);
        return;
    }
    for y in journal.1..=bottom {
        for x in journal.0..=left {
            if y == journal.1 || y == bottom || x == journal.0 || x == left {
                let cell = &mut screen.cells[y * screen.w + x];
                if cell.fg == Some(color::BLUE) {
                    cell.fg = Some("#898989");
                }
            }
        }
    }
    for x in timeline.0 + 2..timeline.2 - 1 {
        let cell = &mut screen.cells[x];
        if cell.fg == Some(color::BLUE) {
            cell.fg = Some("#898989");
        }
    }
    screen.cells[right + 11].fg = Some("#898989");
    for x in timeline.0 + 1..timeline.2 - 1 {
        let cell = &mut screen.cells[bottom * screen.w + x];
        if cell.fg == Some(color::BLUE) {
            cell.fg = Some("#898989");
        }
    }
    let tab_start = if screen.w >= 160 { 51 } else { 16 };
    for x in tab_start..tab_start + 13 {
        screen.cells[(screen.h - 1) * screen.w + x].bg = Some("#494949");
    }
    let status = (screen.h - 1) * screen.w;
    if let Some(dot) = (0..screen.w).find(|&x| screen.cells[status + x].ch == '●') {
        screen.cells[status + dot].fg = Some("#a6a6a6");
    }
    let footer = format!("${:.2}", data.spend);
    if let Some(footer_x) = (0..screen.w.saturating_sub(5)).find(|&x| {
        screen.cells[status + x].ch == '$'
            && screen.cells[status + x + 1].ch == '0'
            && screen.cells[status + x + 2].ch == '.'
    }) {
        screen.text(footer_x, screen.h - 1, &footer, Style::fg(color::FG));
    }

    if data.decisions.is_empty() {
        paint_empty_decisions(screen, journal, timeline, false);
        return;
    }

    let list_style = Style::fg(color::FG);
    screen.fill(journal.0 + 1, journal.1 + 2, left, bottom - 1, list_style);
    if screen.w < 100 {
        screen.fill(
            journal.0 + 1,
            bottom - 1,
            journal.0 + 9,
            bottom,
            Style::fg("#3a3a3a").attrs_reverse(),
        );
        screen.put(
            journal.0 + 9,
            bottom - 1,
            '▌',
            Style::fg("#3a3a3a").bg("#0d0d0d"),
        );
        screen.fill(
            journal.0 + 10,
            bottom - 1,
            left,
            bottom,
            list_style.bg("#0d0d0d"),
        );
    } else if screen.w < 160 {
        screen.fill(
            journal.0 + 1,
            bottom - 1,
            journal.0 + 20,
            bottom,
            Style::fg("#3a3a3a").attrs_reverse(),
        );
        screen.put(
            journal.0 + 20,
            bottom - 1,
            '▏',
            Style::fg("#3a3a3a").bg("#0d0d0d"),
        );
        screen.fill(
            journal.0 + 21,
            bottom - 1,
            left,
            bottom,
            list_style.bg("#0d0d0d"),
        );
    } else {
        screen.fill(
            journal.0 + 1,
            bottom - 1,
            journal.0 + 57,
            bottom,
            Style::fg("#3a3a3a").attrs_reverse(),
        );
        screen.fill(
            journal.0 + 57,
            bottom - 1,
            left,
            bottom,
            list_style.bg("#0d0d0d"),
        );
    }
    for (index, decision) in data.decisions.iter().enumerate() {
        let y = journal.1 + 2 + index;
        if y >= bottom - 1 {
            break;
        }
        let text = format!(
            " {:<11} {:<11} {:<15} {}",
            decision.status,
            decision.review_date.to_string(),
            decision.instrument_id,
            decision.rationale
        );
        let visible: String = text.chars().take(left - journal.0 - 1).collect();
        let style = if index == data.selected {
            Style::fg("#ffffff").bg("#494949").bold()
        } else {
            list_style
        };
        if index == data.selected {
            screen.fill(journal.0 + 1, y, left, y + 1, style);
        }
        screen.text(journal.0 + 1, y, &visible, style);
    }

    screen.fill(
        timeline.0 + 1,
        timeline.1 + 1,
        timeline.2 - 3,
        bottom,
        Style::DEFAULT,
    );
    if screen.w < 100 {
        for y in timeline.1 + 1..bottom {
            screen.fill(
                timeline.2 - 3,
                y,
                timeline.2 - 1,
                y + 1,
                Style::fg("#3a3a3a").attrs_reverse(),
            );
        }
    }
    if screen.w < 100 {
        for x in timeline.2 - 3..timeline.2 - 1 {
            if data.reviews.is_empty() {
                screen.put(
                    x,
                    bottom - 1,
                    '▇',
                    Style::fg("#3a3a3a").bg("#0d0d0d").attrs_reverse(),
                );
            } else {
                screen.put(x, bottom - 4, '▂', Style::fg("#3a3a3a").bg("#0d0d0d"));
                screen.fill(x, bottom - 3, x + 1, bottom, list_style.bg("#0d0d0d"));
            }
        }
    }
    let Some(decision) = data.selected() else {
        return;
    };
    let x = timeline.0 + 2;
    let width = timeline.2.saturating_sub(x + 4);
    let mut row = timeline.1 + 1;
    let line = |screen: &mut Screen, text: &str, style: Style, row: &mut usize| {
        if *row < bottom {
            screen.text(x, *row, text, style);
            *row += 1;
        }
    };
    line(screen, "decision", Style::fg(color::MUTED).bold(), &mut row);
    line(
        screen,
        &format!("instrument    {}", decision.instrument_id),
        list_style,
        &mut row,
    );
    if screen.w >= 160 {
        line(
            screen,
            &format!(
                "thesis        {}",
                decision.thesis_id.as_deref().unwrap_or("")
            ),
            list_style,
            &mut row,
        );
    } else {
        line(screen, "thesis", list_style, &mut row);
        if let Some(thesis) = &decision.thesis_id {
            for wrapped in crate::wrap::wrap_text(thesis, width) {
                line(screen, wrapped.trim_end(), list_style, &mut row);
            }
        }
    }
    line(
        screen,
        &format!("horizon       {}", decision.time_horizon),
        list_style,
        &mut row,
    );
    if let Some(price) = data.current_price {
        line(
            screen,
            &format!("now           {price:.2}"),
            list_style,
            &mut row,
        );
    }
    row += 1;
    for (heading, body) in [
        ("rationale", decision.rationale.as_str()),
        (
            "valuation / price context",
            decision.valuation_context.as_str(),
        ),
        (
            "invalidation criteria",
            decision.invalidation_criteria.as_str(),
        ),
    ] {
        line(screen, heading, Style::fg(color::MUTED).bold(), &mut row);
        for wrapped in crate::wrap::wrap_text(body, width) {
            line(screen, wrapped.trim_end(), list_style, &mut row);
        }
        row += 1;
    }
    line(screen, "timeline", Style::fg(color::MUTED).bold(), &mut row);
    if row < bottom {
        screen.text(
            right + 1,
            row,
            &format!("{}  open", decision.created_at.date()),
            Style::fg("#9b9b9b"),
        );
        row += 1;
    }
    for review in &data.reviews {
        if row < bottom {
            screen.text(
                right + 1,
                row,
                &format!(
                    "{}  {}",
                    review.created_at.date(),
                    review.status.as_deref().unwrap_or("reviewed")
                ),
                Style::fg(color::MUTED),
            );
            row += 1;
        }
        for wrapped in crate::wrap::wrap_text(&review.note, width) {
            line(screen, &wrapped, list_style, &mut row);
        }
    }
    if row < bottom {
        screen.text(
            right + 1,
            row,
            &format!("next review due  {}", decision.review_date),
            Style::fg("#a6a6a6"),
        );
    }
}

fn paint_empty_decisions(
    screen: &mut Screen,
    journal: super::PaneRect,
    timeline: super::PaneRect,
    component_empty: bool,
) {
    let (left, right, bottom) = (journal.2 - 1, timeline.0, journal.3 - 1);
    screen.fill(
        journal.0 + 1,
        journal.1 + 2,
        left,
        bottom - 1,
        Style::fg(color::FG),
    );
    if screen.w < 160 {
        let thumb = journal.0 + if screen.w < 100 { 18 } else { 40 };
        screen.fill(
            journal.0 + 1,
            bottom - 1,
            thumb,
            bottom,
            Style::fg("#3a3a3a"),
        );
        screen.put(thumb, bottom - 1, '▊', Style::fg("#3a3a3a").bg("#0d0d0d"));
        screen.fill(
            thumb + 1,
            bottom - 1,
            left,
            bottom,
            Style::fg(color::FG).bg("#0d0d0d"),
        );
    } else {
        screen.fill(
            journal.0 + 1,
            bottom - 1,
            left,
            bottom,
            Style::fg(color::FG),
        );
    }
    let muted = Style::fg(color::MUTED);
    screen.text(
        right + 2,
        timeline.1 + 1,
        "no decisions yet — press ",
        muted,
    );
    screen.put(
        right + 27,
        timeline.1 + 1,
        'n',
        Style::fg(if component_empty {
            color::BLUE
        } else {
            "#898989"
        })
        .bold(),
    );
    let continuation = if screen.w < 100 {
        " to record the"
    } else if screen.w >= 160 {
        " to record the context you want to revisit"
    } else {
        " to record the context you want to"
    };
    screen.text(right + 28, timeline.1 + 1, continuation, muted);
    if screen.w < 160 {
        screen.text(
            right + 2,
            timeline.1 + 2,
            if screen.w < 100 {
                "context you want to revisit"
            } else {
                "revisit"
            },
            muted,
        );
    }
}

/// The review modal over a populated journal. The note and status are supplied
/// by the app's form state; its initial frame is an oracle state.
pub fn draw_decision_review_form(screen: &mut Screen, note: &str, status: &str) {
    for cell in &mut screen.cells {
        cell.fg = Some(match cell.fg {
            None => color::FG,
            Some("#898989") => "#363636",
            Some("#8a8a8a") => "#373737",
            Some("#333333") => "#141414",
            Some("#d4d4d4") => "#545454",
            Some("#3a3a3a") => "#171717",
            Some("#ffffff") => "#666666",
            Some("#a6a6a6") => "#424242",
            Some(other) => other,
        });
        cell.bg = match cell.bg {
            Some("#232323") => Some("#0e0e0e"),
            Some("#494949") => Some("#1d1d1d"),
            Some("#0d0d0d") => Some("#050505"),
            Some("#1a1a1a") => Some("#0a0a0a"),
            other => other,
        };
    }
    let x = (screen.w - 64) / 2;
    let top = if screen.h < 30 { 1 } else { 2 };
    let bottom = if screen.h >= 50 {
        screen.h - 4
    } else {
        screen.h - 3
    };
    let bg = Style::DEFAULT.bg("#0d0d0d");
    let edge = Style::fg("#333333").bg("#0d0d0d");
    let active = Style::fg("#898989").bg("#0d0d0d");
    screen.fill(x, top, x + 64, bottom + 1, bg);
    for y in top..=bottom {
        screen.put(x, y, '│', edge);
        screen.put(x + 63, y, '│', edge);
    }
    for col in x + 1..x + 63 {
        screen.put(col, top, '─', edge);
        screen.put(col, bottom, '─', edge);
    }
    screen.put(x, top, '┌', edge);
    screen.put(x + 63, top, '┐', edge);
    screen.put(x, bottom, '└', edge);
    screen.put(x + 63, bottom, '┘', edge);

    let title = format!("{:^58}", "review decision");
    screen.text(x + 3, top + 2, &title, active.bold());
    for y in top + 4..=bottom - 5 {
        screen.put(x + 3, y, '▊', active.attrs_reverse());
        screen.put(x + 60, y, '▎', active);
    }
    for col in x + 4..x + 60 {
        screen.put(col, top + 4, '▔', active);
        screen.put(col, bottom - 5, '▁', active);
    }
    screen.fill(
        x + 5,
        top + 5,
        x + 59,
        bottom - 5,
        Style::fg(color::FG).bg("#0d0d0d"),
    );
    if !note.is_empty() {
        screen.text(x + 5, top + 5, note, Style::fg(color::FG).bg("#0d0d0d"));
    } else {
        screen.put(x + 5, top + 5, ' ', Style::fg("#000000").bg(color::FG));
    }
    screen.put(x + 3, bottom - 4, '█', edge);
    screen.fill(
        x + 5,
        bottom - 4,
        x + 60,
        bottom - 3,
        Style::fg(color::FG).bg("#0d0d0d"),
    );
    screen.text(
        x + 5,
        bottom - 4,
        status,
        Style::fg(color::FG).bg("#0d0d0d"),
    );
    let muted = Style::fg(color::MUTED).bg("#0d0d0d");
    let hint = format!("{:^58}", "ctrl+s save  esc cancel");
    screen.text(x + 3, bottom - 2, &hint, muted);
    screen.text(x + 20, bottom - 2, "ctrl+s", active.bold());
    screen.text(x + 33, bottom - 2, "esc", active.bold());
}

/// The Decisions screen: ledger + timeline (empty states; port of
/// `delta/tui/screens/decisions.py`, seeded golden layout).
pub fn draw_decisions(screen: &mut Screen) {
    let (journal, timeline) = pane_columns(screen);
    let (left, right, content_bottom) = (journal.2 - 1, timeline.0, journal.3 - 1);
    screen.pane(
        1,
        0,
        left,
        content_bottom,
        true,
        &[
            ("decisions ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("e", "edit"),
            ("d", "delete"),
            ("/", "filter"),
        ]),
    );
    screen.pane(
        right,
        0,
        screen.w - 2,
        content_bottom,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );

    // Ledger: header row on its own bg + blank rows + the reversed cursor.
    screen.fill(2, 1, left, 2, Style::fg(color::FG).bold().bg("#232323"));
    screen.text(
        2,
        1,
        " status      review      instrument      rati",
        Style::fg(color::FG).bold().bg("#232323"),
    );
    screen.fill(2, 2, left, content_bottom - 1, Style::fg(color::FG));
    screen.fill(
        2,
        content_bottom - 1,
        41,
        content_bottom,
        Style::fg("#3a3a3a"),
    );
    screen.put(
        41,
        content_bottom - 1,
        '▊',
        Style::fg("#3a3a3a").bg("#0d0d0d"),
    );
    screen.fill(
        42,
        content_bottom - 1,
        left,
        content_bottom,
        Style::fg(color::FG).bg("#0d0d0d"),
    );

    // Timeline empty state (wrapped).
    let timeline_hint = Style::fg(color::MUTED);
    let timeline_key = Style::fg(color::BLUE).bold();
    screen.text(right + 2, 1, "no decisions yet — press ", timeline_hint);
    screen.put(right + 27, 1, 'n', timeline_key);
    screen.text(
        right + 28,
        1,
        " to record the context you want to",
        timeline_hint,
    );
    screen.text(right + 2, 2, "revisit", timeline_hint);

    draw_status_bar_decisions(screen, screen.h - 1, screen.w);
}

/// Status bar with the Decisions tab active (`6 Decisions`).
fn draw_status_bar_decisions(screen: &mut Screen, y: usize, w: usize) {
    status_bar_tabs(
        screen,
        y,
        w,
        16,
        "6 Decisions",
        &[(2, "1"), (5, "2"), (8, "3"), (11, "4"), (14, "5")],
        &[],
    );
}

/// Decisions at 200x50: journal table + timeline.
pub fn draw_decisions_wide(screen: &mut Screen) {
    let (journal, timeline) = pane_columns(screen);
    let (left, right, content_bottom) = (journal.2 - 1, timeline.0, journal.3 - 1);
    screen.pane(
        1,
        0,
        left,
        content_bottom,
        true,
        &[
            ("decisions ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("e", "edit"),
            ("d", "delete"),
            ("/", "filter"),
        ]),
    );
    screen.pane(
        right,
        0,
        screen.w - 2,
        content_bottom,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, left, 2, header_style);
    screen.text(
        2,
        1,
        " status      review      instrument      rationale",
        header_style,
    );
    screen.fill(2, 2, left, content_bottom, Style::fg(color::FG));
    let muted = Style::fg(color::MUTED);
    screen.text(right + 2, 1, "no decisions yet — press ", muted);
    screen.put(right + 27, 1, 'n', Style::fg(color::BLUE).bold());
    screen.text(
        right + 28,
        1,
        " to record the context you want to revisit",
        muted,
    );
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "6 Decisions");
}

/// Decisions at 80x24: journal (focused) + timeline panes side by side.
pub fn draw_decisions_narrow(screen: &mut Screen) {
    let (journal, timeline) = pane_columns(screen);
    let (left, right, bottom) = (journal.2 - 1, timeline.0, journal.3 - 1);
    // Left: the decision journal table.
    screen.pane(
        1,
        0,
        left,
        bottom,
        true,
        &[
            ("decisions ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &[
            (" ".to_string(), Style::fg(color::MUTED)),
            ("n".to_string(), Style::fg(color::BLUE).bold()),
            (" new".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("e".to_string(), Style::fg(color::BLUE).bold()),
            (" edit".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("d".to_string(), Style::fg(color::BLUE).bold()),
            (" delete".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("…".to_string(), Style::fg(color::BLUE).bold()),
            (" ".to_string(), Style::fg(color::MUTED)),
        ],
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, left, 2, header_style);
    screen.text(2, 1, " status      review      instr", header_style);
    // The focused table's empty rows still carry the foreground default.
    screen.fill(2, 2, left, bottom - 1, Style::fg(color::FG));
    // Bottom horizontal scrollbar: track, anchor, thumb to the edge.
    screen.fill(2, bottom - 1, 19, bottom, Style::fg("#3a3a3a"));
    screen.put(19, bottom - 1, '▊', Style::fg("#3a3a3a").bg("#0d0d0d"));
    screen.fill(
        20,
        bottom - 1,
        left,
        bottom,
        Style::fg(color::FG).bg("#0d0d0d"),
    );

    // Right: the timeline.
    screen.pane(
        right,
        0,
        screen.w - 2,
        bottom,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );
    let muted = Style::fg(color::MUTED);
    screen.text(right + 2, 1, "no decisions yet — press ", muted);
    screen.put(right + 27, 1, 'n', Style::fg(color::BLUE).bold());
    screen.text(right + 28, 1, " to record the", muted);
    screen.text(right + 2, 2, "context you want to revisit", muted);

    status_bar_narrow(screen, NarrowTab::Decisions);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Screen as _;
    use crossterm::event::KeyModifiers;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn empty_component_matches_the_static_oracle_frame() {
        for (w, h) in [(80, 24), (120, 40), (200, 50)] {
            let mut expected = Screen::new(w, h);
            match crate::Breakpoint::from_width(w as u16) {
                crate::Breakpoint::Narrow => draw_decisions_narrow(&mut expected),
                crate::Breakpoint::Normal => draw_decisions(&mut expected),
                crate::Breakpoint::Wide => draw_decisions_wide(&mut expected),
            }
            let mut actual = Screen::new(w, h);
            DecisionsScreen::default().paint(&mut actual);
            if let Some((index, (got, want))) = actual
                .cells
                .iter()
                .zip(&expected.cells)
                .enumerate()
                .find(|(_, (got, want))| got != want)
            {
                panic!(
                    "empty Decisions at {w}x{h} ({}, {}): got {got:?}, want {want:?}",
                    index % w,
                    index / w
                );
            }
        }
    }

    #[test]
    fn screen_owns_selection_filter_and_delete_confirmation() {
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 21).unwrap();
        let decision = Decision {
            id: "d1".into(),
            instrument_id: "US:AAPL".into(),
            rationale: "test".into(),
            valuation_context: String::new(),
            time_horizon: "1y".into(),
            review_date: date,
            invalidation_criteria: String::new(),
            thesis_id: None,
            thesis_claim_snapshot: None,
            created_at: date.and_hms_opt(0, 0, 0).unwrap(),
            status: "open".into(),
        };
        let mut screen = DecisionsScreen::default();
        screen.set_data(Some(DecisionsData {
            decisions: vec![decision.clone(), decision],
            selected: 0,
            reviews: Vec::new(),
            current_price: None,
            filter: String::new(),
            spend: 0.0,
            confirm_delete: false,
        }));
        assert_eq!(
            screen.handle_key(key(KeyCode::Down)),
            Some(Action::Decision(DecisionCommand::SelectionChanged))
        );
        assert_eq!(screen.selected_index(), 1);
        assert_eq!(
            screen.handle_key(key(KeyCode::Char('d'))),
            Some(Action::Decision(DecisionCommand::ConfirmDelete))
        );
        assert!(screen.data().unwrap().confirm_delete);
        assert_eq!(
            screen.handle_key(key(KeyCode::Esc)),
            Some(Action::Decision(DecisionCommand::Cancel))
        );
        assert!(!screen.data().unwrap().confirm_delete);
        assert_eq!(screen.handle_key(key(KeyCode::Char('y'))), None);
        screen.set_filter("AAPL".into());
        assert_eq!(screen.filter(), "AAPL");
        assert_eq!(screen.selected_index(), 0);
    }

    #[test]
    fn pane_layout_tracks_terminal_width() {
        for (width, expected_left) in [(80, 32), (120, 47), (140, 55), (200, 79)] {
            let screen = Screen::new(width, 40);
            let (journal, timeline) = pane_columns(&screen);
            let (left, right, bottom) = (journal.2 - 1, timeline.0, journal.3 - 1);
            assert_eq!(
                (left, right, bottom),
                (expected_left, expected_left + 1, 37)
            );
        }
    }
}
