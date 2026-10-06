use super::status_bar::{draw_status_bar_wide, pane_hints, status_bar_narrow, NarrowTab};
use crate::screen::{color, Screen, Style};

/// The Settings screen: provider/model, plugins, data sources & markets,
/// diagnostics (port of `delta/tui/screens/config.py`, seeded golden layout).
pub fn draw_settings(screen: &mut Screen) {
    let w = screen.w;
    // Provider & model pane.
    screen.pane(
        1,
        0,
        62,
        3,
        true,
        &[("provider & model", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("↑↓", "choose"), ("enter", "change")]),
    );
    let selected_white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 1, 62, 2, selected_white);
    screen.text(3, 1, "provider: ", selected_white);
    screen.text(13, 1, "● openrouter", selected_white);
    screen.fill(2, 2, 62, 3, Style::fg(color::FG));
    screen.text(3, 2, "model: ", Style::fg(color::FG).bold());
    screen.text(10, 2, "not chosen — press m", Style::fg(color::AMBER));

    // Plugins pane.
    screen.pane(
        1,
        4,
        62,
        6,
        false,
        &[
            ("plugins ", Style::fg(color::BLUE).bold()),
            ("· 1 of 1 ok", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "plugin"), ("enter", "details")]),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(2, 5, 62, 6, selected);
    screen.put(3, 5, '●', selected);
    screen.text(6, 5, "sec_edgar  enabled", selected);

    // Data sources & markets pane.
    screen.pane(
        1,
        7,
        62,
        12,
        false,
        &[("data sources & markets", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "configure/edit"),
            ("a", "add market"),
            ("x", "remove market"),
        ]),
    );
    screen.text(3, 8, "sources", Style::fg(color::MUTED).bold());
    screen.text(3, 10, "markets", Style::fg(color::MUTED).bold());
    screen.fill(2, 11, 62, 12, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        3,
        11,
        "ID  Market  Currency  Yahoo",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );

    // Diagnostics pane.
    screen.pane(
        63,
        0,
        118,
        37,
        false,
        &[
            ("diagnostics ", Style::fg(color::BLUE).bold()),
            ("· 80 rows · $0.00", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("r", "refresh"), ("d", "fold"), ("↑↓", "scroll")]),
    );
    let muted = Style::fg(color::MUTED);
    let fg = Style::fg(color::FG);
    screen.text(65, 1, "evidence", Style::fg(color::MUTED).bold());
    screen.text_right(117, 1, "delta.db · 4 KB", muted);
    screen.fill(64, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        65,
        2,
        "Table        Rows",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    let diag_selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(64, 3, 118, 4, diag_selected);
    screen.fill(64, 4, 118, 8, Style::fg(color::FG));
    let mut first = true;
    for (i, (name, count)) in [
        ("bar", "80"),
        ("event", "0"),
        ("fundamental", "0"),
        ("llmcall", "0"),
        ("newsitem", "0"),
    ]
    .into_iter()
    .enumerate()
    {
        let row_style = if first { diag_selected } else { fg };
        first = false;
        screen.text(65, 3 + i, name, row_style);
        screen.text(78, 3 + i, count, row_style);
    }
    screen.text(65, 8, "latest bar US:AAPL 20 Sep 00:00 UTC", muted);
    screen.text(
        65,
        10,
        "model spend · cumulative",
        Style::fg(color::MUTED).bold(),
    );
    screen.fill(
        64,
        11,
        118,
        12,
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        65,
        11,
        "Task  Model  Calls  USD",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(65, 12, "total  0 calls  $0.00", Style::fg(color::FG).bold());
    screen.text(65, 13, "today $0.00", muted);
    screen.text(65, 13, "today $0.00", muted);
    screen.text(65, 15, "refreshed 09:30:00 · press ", muted);
    screen.put(92, 15, 'r', Style::fg(color::BLUE).bold());
    screen.text(93, 15, " to refresh", muted);

    // Status bar: the c Settings chip is active (blue), no numbered tab.
    let y = screen.h - 1;
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let active = Style::DEFAULT.bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    for x in [2usize, 5, 8, 11, 14, 17] {
        screen.text(x, y, &((x / 3) + 1).to_string(), muted);
    }
    let mut x = w - 58; // dot at col w-58 (62 at w=120)
    screen.put(x, y, '\u{25CF}', Style::fg(color::AMBER).bg(color::PANEL));
    x += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
        ("  ", panel),
    ] {
        x = screen.text(x, y, part, style);
    }
    screen.fill(x - 1, y, x + 11, y + 1, active);
    screen.text(x, y, "c Settings", active_fg);
    x += 11;
    screen.put(x, y, ' ', panel);
    x += 1;
    for (part, style) in [
        ("?", Style::fg(color::MUTED).bg(color::PANEL)),
        (" help · g go", Style::fg(color::MUTED).bg(color::PANEL)),
    ] {
        x = screen.text(x, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}

/// Settings at 200x50: provider/plugins/sources panes left, diagnostics
/// right.
pub fn draw_settings_wide(screen: &mut Screen) {
    // Provider & model.
    screen.pane(
        1,
        0,
        62,
        3,
        true,
        &[("provider & model", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("↑↓", "choose"), ("enter", "change")]),
    );
    let selected_white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 1, 62, 2, selected_white);
    screen.text(3, 1, "provider: ", selected_white);
    screen.text(13, 1, "● openrouter", selected_white);
    screen.fill(2, 2, 62, 3, Style::fg(color::FG));
    screen.text(3, 2, "model: ", Style::fg(color::FG).bold());
    screen.text(10, 2, "not chosen — press m", Style::fg(color::AMBER));

    // Plugins.
    screen.pane(
        1,
        4,
        62,
        6,
        false,
        &[
            ("plugins ", Style::fg(color::BLUE).bold()),
            ("· 1 of 1 ok", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "plugin"), ("enter", "details")]),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(2, 5, 62, 6, selected);
    screen.put(3, 5, '●', selected);
    screen.text(6, 5, "sec_edgar  enabled", selected);

    // Data sources & markets.
    screen.pane(
        1,
        7,
        62,
        12,
        false,
        &[("data sources & markets", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "configure/edit"),
            ("a", "add market"),
            ("x", "remove market"),
        ]),
    );
    screen.text(3, 8, "sources", Style::fg(color::MUTED).bold());
    screen.text(3, 10, "markets", Style::fg(color::MUTED).bold());
    let table_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(2, 11, 62, 12, table_style);
    screen.text(3, 11, "ID  Market  Currency  Yahoo", table_style);

    // Diagnostics.
    screen.pane(
        63,
        0,
        198,
        screen.h - 3,
        false,
        &[
            ("diagnostics ", Style::fg(color::BLUE).bold()),
            ("· 80 rows · $0.00", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("r", "refresh"), ("d", "fold"), ("↑↓", "scroll")]),
    );
    let muted = Style::fg(color::MUTED);
    let fg = Style::fg(color::FG);
    screen.text(65, 1, "evidence", Style::fg(color::MUTED).bold());
    screen.text_right(197, 1, "delta.db · 4 KB", muted);
    screen.fill(64, 2, 198, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        65,
        2,
        "Table        Rows",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    let diag_selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(64, 3, 198, 4, diag_selected);
    screen.fill(64, 4, 198, 8, fg);
    let mut first = true;
    for (i, (name, count)) in [
        ("bar", "80"),
        ("event", "0"),
        ("fundamental", "0"),
        ("llmcall", "0"),
        ("newsitem", "0"),
    ]
    .into_iter()
    .enumerate()
    {
        let row_style = if first { diag_selected } else { fg };
        first = false;
        screen.text(65, 3 + i, name, row_style);
        screen.text(78, 3 + i, count, row_style);
    }
    screen.text(65, 8, "latest bar US:AAPL 20 Sep 00:00 UTC", muted);
    screen.text(
        65,
        10,
        "model spend · cumulative",
        Style::fg(color::MUTED).bold(),
    );
    screen.fill(
        64,
        11,
        198,
        12,
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        65,
        11,
        "Task  Model  Calls  USD",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(65, 12, "total  0 calls  $0.00", Style::fg(color::FG).bold());
    screen.text(65, 13, "today $0.00", muted);
    screen.text(65, 15, "refreshed 09:30:00 · press ", muted);
    screen.put(92, 15, 'r', Style::fg(color::BLUE).bold());
    screen.text(93, 15, " to refresh", muted);

    draw_status_bar_wide(screen, screen.h - 1, screen.w, "c Settings");
}

/// Settings at 80x24: four panes stacked full-width.
pub fn draw_settings_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    // Provider & model pane.
    screen.pane(
        1,
        0,
        x1,
        3,
        true,
        &[("provider & model", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("↑↓", "choose"), ("enter", "change")]),
    );
    let selected_white = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 1, x1, 2, selected_white);
    screen.text(3, 1, "provider: ", selected_white);
    screen.text(13, 1, "● openrouter", selected_white);
    screen.fill(2, 2, x1, 3, Style::fg(color::FG));
    screen.text(3, 2, "model: ", Style::fg(color::FG).bold());
    screen.text(10, 2, "not chosen — press m", Style::fg(color::AMBER));

    // Plugins pane.
    screen.pane(
        1,
        4,
        x1,
        6,
        false,
        &[
            ("plugins ", Style::fg(color::BLUE).bold()),
            ("· 1 of 1 ok", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "plugin"), ("enter", "details")]),
    );
    let selected = Style::fg(color::FG).bg("#13254b");
    screen.fill(2, 5, x1, 6, selected);
    screen.put(3, 5, '●', selected);
    screen.text(6, 5, "sec_edgar  enabled", selected);

    // Data sources & markets pane.
    screen.pane(
        1,
        7,
        x1,
        12,
        false,
        &[("data sources & markets", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "configure/edit"),
            ("a", "add market"),
            ("x", "remove market"),
        ]),
    );
    screen.text(3, 8, "sources", Style::fg(color::MUTED).bold());
    screen.text(3, 10, "markets", Style::fg(color::MUTED).bold());
    let table_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(2, 11, x1, 12, table_style);
    screen.text(3, 11, "ID  Market  Currency  Yahoo", table_style);

    // Diagnostics pane (folded to its summary line).
    screen.pane(
        1,
        13,
        x1,
        15,
        false,
        &[
            ("diagnostics ", Style::fg(color::BLUE).bold()),
            ("· 80 rows · $0.00", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("d", "expand"), ("r", "refresh")]),
    );
    screen.text(3, 14, "▸", Style::fg(color::MUTED));
    screen.text(
        4,
        14,
        " 80 rows · latest bar 20 Sep 00:00 UTC · spend $0.00",
        Style::fg(color::FG),
    );

    status_bar_narrow(screen, NarrowTab::Settings);
}
