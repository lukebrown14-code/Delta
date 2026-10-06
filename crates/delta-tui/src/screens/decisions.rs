use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};

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
