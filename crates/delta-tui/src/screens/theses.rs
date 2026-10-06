use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};

/// The Theses screen: fleet list, thesis detail, evidence table (all empty
/// states; port of `delta/tui/screens/theses.py`, seeded golden layout).
pub fn draw_theses(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 37 at h=40
    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("n", "new"), ("d", "edit"), ("/", "filter")]),
    );
    screen.pane(
        37,
        0,
        78,
        content_bottom,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("thesis", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
    );
    screen.pane(
        79,
        0,
        118,
        content_bottom,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("f", "find")]),
    );

    screen.fill(2, 1, 36, content_bottom - 1, Style::fg(color::FG));
    screen.text(
        39,
        1,
        "no theses yet — press n to create one",
        Style::fg(color::FG),
    );

    // Evidence table: header row on the panel, blank rows, separator, note.
    screen.fill(80, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        81,
        2,
        "   ±  age   note",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(81, 1, "no evidence yet", Style::fg(color::FG));
    screen.fill(80, 3, 118, 29, Style::fg(color::FG));
    for x in 80..118 {
        screen.put(x, 29, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        81,
        30,
        "no evidence yet — press f to find",
        Style::fg(color::FG),
    );
    screen.text(81, 31, "candidates", Style::fg(color::FG));

    draw_status_bar_theses(screen, screen.h - 1, screen.w);
}

/// Status bar with the Theses tab active (`4 Theses`).
fn draw_status_bar_theses(screen: &mut Screen, y: usize, w: usize) {
    status_bar_tabs(
        screen,
        y,
        w,
        10,
        "4 Theses",
        &[(2, "1"), (5, "2"), (8, "3")],
        &[(21, "5"), (24, "6")],
    );
}

/// Theses at 200x50: thesis list, detail, evidence panes.
pub fn draw_theses_wide(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 47 at h=50
    let mid_bottom = screen.h - 10; // 40 at h=50
    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("n", "new"), ("d", "edit"), ("/", "filter")]),
    );
    screen.pane(
        37,
        0,
        158,
        content_bottom,
        false,
        &[("t thesis", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
    );
    screen.pane(
        159,
        0,
        198,
        content_bottom,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("f", "find")]),
    );
    screen.fill(2, 1, 36, content_bottom - 1, Style::fg(color::FG));
    screen.text(
        39,
        1,
        "no theses yet — press n to create one",
        Style::fg(color::FG),
    );
    screen.text(161, 1, "no evidence yet", Style::fg(color::FG));
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(160, 2, 198, 3, header_style);
    screen.fill(160, 3, 198, mid_bottom, Style::fg(color::FG));
    screen.text(160, 2, "    ±  age   note", header_style);
    // Input separator above the evidence prompt line.
    for x in 160..198 {
        screen.put(x, mid_bottom - 1, '─', Style::fg("#333333"));
    }
    let fg = Style::fg(color::FG);
    screen.text(161, mid_bottom, "no evidence yet — press f to find", fg);
    screen.text(161, mid_bottom + 1, "candidates", fg);
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "4 Theses");
}

/// Theses at 80x24: one focused full-width pane.
pub fn draw_theses_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    let bottom = screen.h - 3; // 21 at h=24
    screen.pane(
        1,
        0,
        x1,
        bottom,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("d", "edit"),
            ("/", "filter"),
            ("enter", "thesis"),
            ("e", "evidence"),
        ]),
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, x1, 2, header_style);
    screen.text(2, 1, "   Claim", header_style);
    screen.text(49, 1, "Health", header_style);
    screen.text(62, 1, "Tilt", header_style);
    screen.text(72, 1, "Queue", header_style);
    // The focused table's empty rows still carry the foreground default.
    screen.fill(2, 2, x1, bottom - 1, Style::fg(color::FG));
    screen.text(
        3,
        bottom - 1,
        "no theses yet — press n to create one",
        Style::fg(color::MUTED),
    );
    status_bar_narrow(screen, NarrowTab::Theses);
}
