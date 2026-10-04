use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};

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
