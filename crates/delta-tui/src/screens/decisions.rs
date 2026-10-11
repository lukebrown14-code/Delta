use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};
use delta_services::{Decision, DecisionReview};

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

/// Paint the populated journal on the same cell-grid chrome as the empty state.
pub fn draw_decisions_live(screen: &mut Screen, data: &DecisionsData) {
    let (left, right) = if screen.w < 100 {
        draw_decisions_narrow(screen);
        (32, 33)
    } else if screen.w >= 160 {
        draw_decisions_wide(screen);
        (79, 80)
    } else {
        draw_decisions(screen);
        (47, 48)
    };
    let bottom = screen.h - 3;
    let narrow_hint = (screen.w < 100)
        .then(|| screen.cells[bottom * screen.w + 1..bottom * screen.w + left + 1].to_vec());
    let title = Style::fg(color::BLUE).bold();
    let count = format!("· {}", data.decisions.len());
    screen.pane(
        1,
        0,
        left,
        bottom,
        true,
        &[
            ("decisions ", title),
            (&count, Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("e", "edit"),
            ("d", "delete"),
            ("/", "filter"),
        ]),
    );
    if let Some(hint) = narrow_hint {
        screen.cells[bottom * screen.w + 1..bottom * screen.w + left + 1].clone_from_slice(&hint);
    }
    for y in 0..=bottom {
        for x in 1..=left {
            if y == 0 || y == bottom || x == 1 || x == left {
                let cell = &mut screen.cells[y * screen.w + x];
                if cell.fg == Some(color::BLUE) {
                    cell.fg = Some("#898989");
                }
            }
        }
    }
    let review_count = format!(" · {} reviews", data.reviews.len());
    screen.pane(
        right,
        0,
        screen.w - 2,
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
    for x in right + 2..screen.w - 2 {
        let cell = &mut screen.cells[x];
        if cell.fg == Some(color::BLUE) {
            cell.fg = Some("#898989");
        }
    }
    screen.cells[right + 11].fg = Some("#898989");
    for x in right + 1..screen.w - 2 {
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

    let list_style = Style::fg(color::FG);
    screen.fill(2, 2, left, bottom - 1, list_style);
    if screen.w < 100 {
        screen.fill(
            2,
            bottom - 1,
            10,
            bottom,
            Style::fg("#3a3a3a").attrs_reverse(),
        );
        screen.put(10, bottom - 1, '▌', Style::fg("#3a3a3a").bg("#0d0d0d"));
        screen.fill(11, bottom - 1, left, bottom, list_style.bg("#0d0d0d"));
    } else if screen.w < 160 {
        screen.fill(
            2,
            bottom - 1,
            21,
            bottom,
            Style::fg("#3a3a3a").attrs_reverse(),
        );
        screen.put(21, bottom - 1, '▏', Style::fg("#3a3a3a").bg("#0d0d0d"));
        screen.fill(22, bottom - 1, left, bottom, list_style.bg("#0d0d0d"));
    } else {
        screen.fill(
            2,
            bottom - 1,
            58,
            bottom,
            Style::fg("#3a3a3a").attrs_reverse(),
        );
        screen.fill(58, bottom - 1, left, bottom, list_style.bg("#0d0d0d"));
    }
    for (index, decision) in data.decisions.iter().enumerate() {
        let y = 2 + index;
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
        let visible: String = text.chars().take(left - 2).collect();
        let style = if index == data.selected {
            Style::fg("#ffffff").bg("#494949").bold()
        } else {
            list_style
        };
        if index == data.selected {
            screen.fill(2, y, left, y + 1, style);
        }
        screen.text(2, y, &visible, style);
    }

    screen.fill(right + 1, 1, screen.w - 4, bottom, Style::DEFAULT);
    if screen.w < 100 {
        for y in 1..bottom {
            screen.fill(
                screen.w - 4,
                y,
                screen.w - 2,
                y + 1,
                Style::fg("#3a3a3a").attrs_reverse(),
            );
        }
    }
    if screen.w < 100 {
        for x in screen.w - 4..screen.w - 2 {
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
    let x = right + 2;
    let width = screen.w.saturating_sub(x + 5);
    let mut row = 1;
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
    let content_bottom = screen.h - 3; // 37 at h=40
    screen.pane(
        1,
        0,
        47,
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
        48,
        0,
        118,
        content_bottom,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );

    // Ledger: header row on its own bg + blank rows + the reversed cursor.
    screen.fill(2, 1, 47, 2, Style::fg(color::FG).bold().bg("#232323"));
    screen.text(
        2,
        1,
        " status      review      instrument      rati",
        Style::fg(color::FG).bold().bg("#232323"),
    );
    screen.fill(2, 2, 47, content_bottom - 1, Style::fg(color::FG));
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
        47,
        content_bottom,
        Style::fg(color::FG).bg("#0d0d0d"),
    );

    // Timeline empty state (wrapped).
    let timeline_hint = Style::fg(color::MUTED);
    let timeline_key = Style::fg(color::BLUE).bold();
    screen.text(50, 1, "no decisions yet — press ", timeline_hint);
    screen.put(75, 1, 'n', timeline_key);
    screen.text(76, 1, " to record the context you want to", timeline_hint);
    screen.text(50, 2, "revisit", timeline_hint);

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
    let content_bottom = screen.h - 3; // 47 at h=50
    screen.pane(
        1,
        0,
        79,
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
        80,
        0,
        198,
        content_bottom,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, 79, 2, header_style);
    screen.text(
        2,
        1,
        " status      review      instrument      rationale",
        header_style,
    );
    screen.fill(2, 2, 79, content_bottom, Style::fg(color::FG));
    let muted = Style::fg(color::MUTED);
    screen.text(82, 1, "no decisions yet — press ", muted);
    screen.put(107, 1, 'n', Style::fg(color::BLUE).bold());
    screen.text(108, 1, " to record the context you want to revisit", muted);
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "6 Decisions");
}

/// Decisions at 80x24: journal (focused) + timeline panes side by side.
pub fn draw_decisions_narrow(screen: &mut Screen) {
    let bottom = screen.h - 3; // 21 at h=24
                               // Left: the decision journal table.
    screen.pane(
        1,
        0,
        32,
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
    screen.fill(2, 1, 32, 2, header_style);
    screen.text(2, 1, " status      review      instr", header_style);
    // The focused table's empty rows still carry the foreground default.
    screen.fill(2, 2, 32, bottom - 1, Style::fg(color::FG));
    // Bottom horizontal scrollbar: track, anchor, thumb to the edge.
    screen.fill(2, bottom - 1, 19, bottom, Style::fg("#3a3a3a"));
    screen.put(19, bottom - 1, '▊', Style::fg("#3a3a3a").bg("#0d0d0d"));
    screen.fill(
        20,
        bottom - 1,
        32,
        bottom,
        Style::fg(color::FG).bg("#0d0d0d"),
    );

    // Right: the timeline.
    screen.pane(
        33,
        0,
        screen.w - 2,
        bottom,
        false,
        &[("timeline", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("r", "review"), ("o", "research"), ("↑↓", "scroll")]),
    );
    let muted = Style::fg(color::MUTED);
    screen.text(35, 1, "no decisions yet — press ", muted);
    screen.put(60, 1, 'n', Style::fg(color::BLUE).bold());
    screen.text(61, 1, " to record the", muted);
    screen.text(35, 2, "context you want to revisit", muted);

    status_bar_narrow(screen, NarrowTab::Decisions);
}
