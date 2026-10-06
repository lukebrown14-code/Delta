use super::status_bar::{
    draw_status_bar_nav, draw_status_bar_wide, pane_hints, status_bar_narrow, NarrowTab,
};
use crate::screen::{color, Screen, Style};

/// The Research screen: company / report / evidence panes with their
/// empty-state content (port of `delta/tui/screens/research.py`, seeded
/// golden layout).
pub fn draw_research(screen: &mut Screen) {
    let surface_fg_none = Style::DEFAULT.bg("#0d0d0d");
    let surface = Style::fg(color::FG).bg("#0d0d0d");
    let muted = Style::fg(color::MUTED);
    let content_bottom = screen.h - 3; // 37 at h=40

    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("company ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select")]),
    );
    screen.pane(
        37,
        0,
        78,
        content_bottom,
        false,
        &[
            ("r ", Style::fg(color::BLUE).bold()),
            ("report ", Style::fg(color::BLUE).bold()),
            ("· no report", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "scroll"), ("enter", "citation"), ("n", "regene…")]),
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
        &pane_hints(&[
            ("/", "search"),
            ("k", "kind"),
            ("space", "fold"),
            ("l", "m…"),
        ]),
    );

    // Company pane: DataTable header on its own bg, blank rows, empty state.
    screen.fill(2, 1, 36, 2, Style::fg(color::FG).bold().bg("#232323"));
    screen.text(
        2,
        1,
        " Company  Report",
        Style::fg(color::FG).bold().bg("#232323"),
    );
    screen.fill(2, 2, 36, 33, Style::fg(color::FG));
    screen.text(
        3,
        33,
        "no companies yet — press 1 to",
        Style::fg(color::MUTED),
    );
    screen.text(3, 34, "add a target", Style::fg(color::MUTED));
    // DataTable bottom separator, then the hint rows on a narrow surface strip
    // (cols 2..20).
    for x in 2..36 {
        screen.put(x, 32, '─', Style::fg(color::BORDER_BLURRED));
    }
    // Each hint row's surface strip is exactly its text width plus one cell.
    for (y, key, rest, strip_end) in [
        (content_bottom - 2, "u", " gather company", 21usize),
        (content_bottom - 1, "U", " gather all", 17),
    ] {
        screen.fill(
            3,
            y,
            strip_end,
            y + 1,
            Style::fg(color::FG).bold().bg("#0d0d0d"),
        );
        screen.put(strip_end, y, ' ', surface_fg_none);
        screen.put(2, y, ' ', surface_fg_none);
        screen.put(3, y, ' ', Style::fg(color::FG).bold().bg("#0d0d0d"));
        screen.put(
            4,
            y,
            key.chars().next().unwrap(),
            Style::fg(color::BLUE).bold().bg("#0d0d0d"),
        );
        screen.text(5, y, rest, Style::fg(color::FG).bold().bg("#0d0d0d"));
    }

    // Report pane: scroll body on the surface, button, centred heading.
    screen.fill(38, 6, 76, 37, surface_fg_none);
    let button_bg = Style::DEFAULT.bg(color::BLUE_BG);
    screen.fill(40, 2, 61, 3, button_bg);
    screen.put(41, 2, ' ', white_on_blue());
    screen.put(59, 2, ' ', white_on_blue());
    screen.put(42, 2, 'n', Style::fg(color::BLUE).bold().bg(color::BLUE_BG));
    screen.text(43, 2, " generate report", white_on_blue());
    for x in 40..72 {
        for y in 8..9 {
            screen.put(x, y, ' ', Style::fg(color::BLUE).bold().bg("#0d0d0d"));
        }
    }
    screen.text(53, 8, "Report", Style::fg(color::BLUE).bold().bg("#0d0d0d"));
    screen.text(40, 10, "No report yet — press n to", surface);
    screen.text(40, 11, "generate one.", surface);

    // Evidence pane: search field, column header, separator, empty states.
    screen.put(80, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.put(81, 1, ' ', surface_fg_none);
    screen.put(106, 1, ' ', surface_fg_none);
    screen.text(
        82,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(99, 1, 106, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(108, 1, "kind: all", muted);
    // Column header row: DataTable header on the panel background, bold.
    screen.fill(80, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        81,
        2,
        "Evidence",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(94, 2, "Type", Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(107, 2, "Date", Style::fg(color::FG).bold().bg(color::PANEL));
    // Blank DataTable rows carry the foreground style.
    screen.fill(80, 3, 118, 26, Style::fg(color::FG));
    screen.fill(81, 26, 116, 27, Style::fg(color::FG));
    screen.text(81, 26, "no companies yet — press 1 to add a", muted);
    for x in 80..118 {
        screen.put(x, 27, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        81,
        28,
        "select evidence to preview it",
        Style::fg(color::FG),
    );

    draw_status_bar_research(screen, screen.h - 1, w_of(screen));
}

fn white_on_blue() -> Style {
    Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
}

fn w_of(screen: &Screen) -> usize {
    screen.w
}

/// Status bar with the Research tab active (`3 Research`).
fn draw_status_bar_research(screen: &mut Screen, y: usize, w: usize) {
    draw_status_bar_nav(screen, y, w, "3 Research");
}

pub fn draw_research_wide(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 47 at h=50
    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("company ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select")]),
    );
    screen.pane(
        37,
        0,
        158,
        content_bottom,
        false,
        &[
            ("r ", Style::fg(color::BLUE).bold()),
            ("report ", Style::fg(color::BLUE).bold()),
            ("· no report", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "scroll"), ("enter", "citation"), ("n", "regenerate")]),
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
        // Truncated with an ellipsis in the exported frame.
        &vec![
            (" ".to_string(), Style::fg(color::MUTED)),
            ("/".to_string(), Style::fg(color::BLUE).bold()),
            (" search".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("k".to_string(), Style::fg(color::BLUE).bold()),
            (" kind".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("space".to_string(), Style::fg(color::BLUE).bold()),
            (" fold".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("l".to_string(), Style::fg(color::BLUE).bold()),
            (" m…".to_string(), Style::fg(color::MUTED)),
            (" ".to_string(), Style::fg(color::MUTED)),
        ],
    );

    // Company table header.
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, 36, 2, header_style);
    screen.fill(2, 2, 36, 42, Style::fg(color::FG));
    screen.text(2, 1, " Company  Report", header_style);

    // Company pane: input separator, empty state, gather actions.
    for x in 2..36 {
        screen.put(x, 42, '─', Style::fg("#333333"));
    }
    screen.text(
        3,
        43,
        "no companies yet — press 1 to",
        Style::fg(color::MUTED),
    );
    screen.text(3, 44, "add a target", Style::fg(color::MUTED));
    // The action rows sit on a dark well block sized to the text.
    let well_fg = Style::fg(color::FG).bg("#0d0d0d").bold();
    screen.fill(2, 45, 22, 46, Style::DEFAULT.bg("#0d0d0d"));
    screen.fill(2, 46, 18, 47, Style::DEFAULT.bg("#0d0d0d"));
    screen.fill(3, 45, 21, 46, well_fg);
    screen.fill(3, 46, 17, 47, well_fg);
    screen.put(4, 45, 'u', Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(6, 45, "gather company", well_fg);
    screen.put(4, 46, 'U', Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(6, 46, "gather all", well_fg);

    // Report pane: the report well.
    screen.fill(38, 6, 156, 47, Style::DEFAULT.bg("#0d0d0d"));
    // " Report " heading band centred in the well.
    screen.fill(40, 8, 152, 9, Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(93, 8, "Report", Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(
        40,
        10,
        "No report yet — press n to generate one.",
        Style::fg(color::FG).bg("#0d0d0d"),
    );

    // Report pane: the generate button.
    screen.fill(40, 2, 61, 3, Style::DEFAULT.bg(color::BLUE_BG));
    let white_bold_on_blue = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.text(41, 2, " ", white_bold_on_blue);
    screen.put(42, 2, 'n', Style::fg(color::BLUE).bg(color::BLUE_BG).bold());
    screen.text(43, 2, " generate report", white_bold_on_blue);
    screen.put(59, 2, ' ', white_bold_on_blue);

    // Evidence desk: search input, kind filter, table header.
    let well = Style::DEFAULT.bg("#0d0d0d");
    screen.fill(160, 1, 187, 2, well);
    screen.put(160, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        162,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(179, 1, 186, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(188, 1, "kind: all", Style::fg(color::MUTED));
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(160, 2, 198, 3, header_style);
    screen.fill(160, 3, 198, 36, Style::fg(color::FG));
    screen.text(160, 2, " Evidence", header_style);
    screen.text(174, 2, "Type", header_style);
    screen.text(187, 2, "Date", header_style);
    // Empty state, its hint strip, and the clip at the pane edge.
    screen.text(
        161,
        36,
        "no companies yet — press 1 to add a",
        Style::fg(color::MUTED),
    );
    for x in 160..198 {
        screen.put(x, 37, '─', Style::fg("#333333"));
    }
    screen.text(
        161,
        38,
        "select evidence to preview it",
        Style::fg(color::FG),
    );

    draw_status_bar_wide(screen, screen.h - 1, screen.w, "3 Research");
}

pub fn draw_research_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    let bottom = screen.h - 3; // 21 at h=24
    screen.pane(
        1,
        0,
        x1,
        bottom,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("/", "search"),
            ("k", "kind"),
            ("enter", "preview"),
            ("esc", "back"),
        ]),
    );

    // Search input: block cursor, placeholder in the disabled token, the
    // rest of the strip in foreground-on-well.
    screen.fill(2, 1, 67, 2, Style::DEFAULT.bg("#0d0d0d"));
    screen.put(2, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        4,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(21, 1, 66, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(68, 1, "kind: all", Style::fg(color::MUTED));

    // Table header strip.
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(2, 2, x1, 3, header_style);
    screen.text(2, 2, " Evidence", header_style);
    screen.text(54, 2, "Type", header_style);
    screen.text(67, 2, "Date", header_style);

    // Empty rows carry the foreground default.
    screen.fill(2, 3, x1, bottom - 1, Style::fg(color::FG));
    screen.text(
        3,
        bottom - 1,
        "no companies yet — press 1 to add a target",
        Style::fg(color::MUTED),
    );

    status_bar_narrow(screen, NarrowTab::Research);
}
