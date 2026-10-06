use crate::screen::{color, Screen, Style};

/// Hint runs in the panes' title/hint bars: key (bold blue) + " hint" (muted),
/// pairs joined by two muted spaces, wrapped in single spaces.
pub(super) fn pane_hints(pairs: &[(&str, &str)]) -> Vec<(String, Style)> {
    let mut runs = vec![(" ".to_string(), Style::fg(color::MUTED))];
    let mut first = true;
    for (key, hint) in pairs {
        if !first {
            runs.push(("  ".to_string(), Style::fg(color::MUTED)));
        }
        first = false;
        runs.push(((*key).to_string(), Style::fg(color::BLUE).bold()));
        runs.push((format!(" {hint}"), Style::fg(color::MUTED)));
    }
    runs.push((" ".to_string(), Style::fg(color::MUTED)));
    runs
}

/// Status bar with an arbitrary active tab label.
pub(super) fn draw_status_bar_nav(screen: &mut Screen, y: usize, w: usize, active: &str) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let chip_bg = Style::DEFAULT.bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    // Keys 1..6; the active one expands to "n Label" in the primary chip.
    screen.put(1, y, ' ', panel);
    screen.text(2, y, "1", muted);
    screen.put(4, y, ' ', panel);
    screen.text(5, y, "2", muted);
    screen.put(7, y, ' ', chip_bg);
    x_active_label(screen, y, 8, active);
    let after = 8 + active.chars().count() + 1;
    screen.put(after, y, ' ', panel);
    let mut x = after + 1;
    for key in ["4", "5", "6"] {
        x = screen.text(x, y, key, muted);
        x += 2;
    }
    let cluster = "●  data 1d  openrouter  $0.00  c Settings  ? help · g go ";
    let start = w - 1 - cluster.chars().count();
    while x < start {
        screen.put(x, y, ' ', panel);
        x += 1;
    }
    screen.put(x, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    x += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
        ("  ", panel),
        ("c", muted),
        (" Settings", Style::fg(color::MUTED).bg(color::PANEL)),
        ("  ", panel),
        ("?", Style::fg(color::MUTED).bg(color::PANEL)),
        (" help · g go", Style::fg(color::MUTED).bg(color::PANEL)),
    ] {
        x = screen.text(x, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}

fn x_active_label(screen: &mut Screen, y: usize, mut x: usize, label: &str) {
    // "n Label" with a trailing chip space (no leading one).
    let mut parts = label.chars();
    let key = parts.next().unwrap();
    x = screen.text(x, y, &key.to_string(), active_chip());
    x = screen.text(x, y, &parts.collect::<String>(), active_chip());
    screen.put(x, y, ' ', Style::DEFAULT.bg(color::BLUE_BG));
}

fn active_chip() -> Style {
    Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
}

/// The narrow status bar: `1  2  3 …` keys at 3-cell pitch, the active tab
/// as a chip (labelled), and the right cluster without the provider name or
/// help hint.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum NarrowTab {
    Home,
    Research,
    Theses,
    Ask,
    Decisions,
    Settings,
}

pub(super) fn status_bar_narrow(screen: &mut Screen, active: NarrowTab) {
    let y = screen.h - 1;
    let w = screen.w;
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w - 1, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));

    // Tab keys at 3-cell pitch; the active tab becomes a labelled chip that
    // consumes its width plus padding. The exported chips keep the tab's own
    // number ("1 Home", "3 Research", …).
    let tabs: [(NarrowTab, &str, Option<&str>); 5] = [
        (NarrowTab::Home, "1", Some("1 Home")),
        (NarrowTab::Research, "2", Some("3 Research")),
        (NarrowTab::Theses, "3", Some("4 Theses")),
        (NarrowTab::Ask, "4", Some("5 Ask")),
        (NarrowTab::Decisions, "5", Some("6 Decisions")),
    ];
    let _ = tabs;
    // Six fixed positions; the active tab becomes a chip labelled with its
    // own position digit ("3 Research", "4 Theses", ...).
    // The tabs keep their global numbers: research is tab 3, theses 4, ask
    // 5, decisions 6 (watchlist, 2, has no narrow screen of its own).
    let mut x = 2usize;
    for position in 1..=6u32 {
        let tab = match position {
            1 => Some(NarrowTab::Home),
            3 => Some(NarrowTab::Research),
            4 => Some(NarrowTab::Theses),
            5 => Some(NarrowTab::Ask),
            6 => Some(NarrowTab::Decisions),
            _ => None,
        };
        let key = char::from_digit(position, 10).unwrap().to_string();
        if tab == Some(active) {
            let label = format!(
                "{key} {}",
                match tab.unwrap() {
                    NarrowTab::Home => "Home",
                    NarrowTab::Research => "Research",
                    NarrowTab::Theses => "Theses",
                    NarrowTab::Ask => "Ask",
                    _ => "Decisions",
                }
            );
            let len = label.chars().count();
            screen.fill(
                x - 1,
                y,
                x + len + 1,
                y + 1,
                Style::DEFAULT.bg(color::BLUE_BG),
            );
            screen.text(x, y, &label, active_fg);
            x += len + 2;
        } else {
            screen.text(x, y, &key, muted);
            x += 3;
        }
    }

    // Right cluster: dot, data age, spend, then the settings tab (chip when
    // active, muted label otherwise).
    screen.put(49, y, '\u{25CF}', Style::fg(color::AMBER).bg(color::PANEL));
    screen.text(52, y, "data 1d", plain);
    screen.text(59, y, "  ", panel);
    screen.text(61, y, "$0.00", plain);
    if active == NarrowTab::Settings {
        let label = "c Settings";
        let len = label.chars().count();
        let cx = w - len - 2;
        screen.fill(
            cx - 1,
            y,
            cx + len + 1,
            y + 1,
            Style::DEFAULT.bg(color::BLUE_BG),
        );
        screen.text(cx, y, label, active_fg);
    } else {
        screen.text(68, y, "c", muted);
        screen.text(69, y, " Settings", Style::fg(color::MUTED).bg(color::PANEL));
    }
}

/// Shared tab-bar painter: inactive keys, an optional active chip, then the
/// cluster.
pub(super) fn status_bar_tabs(
    screen: &mut Screen,
    y: usize,
    w: usize,
    chip_x: usize,
    chip: &str,
    before: &[(usize, &str)],
    after: &[(usize, &str)],
) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    if chip.is_empty() {
        for (x, key) in before.iter().chain(after.iter()) {
            screen.text(*x, y, key, muted);
        }
    } else {
        for (x, key) in before {
            screen.text(*x, y, key, muted);
        }
        screen.fill(
            chip_x,
            y,
            chip_x + chip.chars().count() + 2,
            y + 1,
            Style::DEFAULT.bg(color::BLUE_BG),
        );
        let mut x = screen.text(chip_x + 1, y, chip, active_fg);
        screen.put(x, y, ' ', Style::DEFAULT.bg(color::BLUE_BG));
        x += 1;
        for (xk, key) in after {
            screen.text(*xk, y, key, muted);
        }
        let _ = x;
    }
    let cluster = "●  data 1d  openrouter  $0.00  c Settings  ? help · g go ";
    let mut cx = w - 1 - cluster.chars().count();
    screen.put(cx, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    cx += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
        ("  ", panel),
        ("c", muted),
        (" Settings", Style::fg(color::MUTED).bg(color::PANEL)),
        ("  ", panel),
        ("?", Style::fg(color::MUTED).bg(color::PANEL)),
        (" help · g go", Style::fg(color::MUTED).bg(color::PANEL)),
    ] {
        cx = screen.text(cx, y, part, style);
    }
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}

/// Wide status bar: every tab shows its label; `active` gets the chip.
pub(super) fn draw_status_bar_wide(screen: &mut Screen, y: usize, w: usize, active: &str) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let muted_plain = Style::fg(color::MUTED).bg(color::PANEL);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    // Captured positions: keys expand to "n Label" wide; active is the chip.
    if active == "1 Home" {
        screen.fill(1, y, 9, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            2,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(2, y, '1', muted);
        screen.text(3, y, " Home", muted_plain);
    }
    if active == "2 Watchlist" {
        screen.fill(9, y, 22, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            10,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(10, y, '2', muted);
        screen.text(11, y, " Watchlist", muted_plain);
    }
    if active == "3 Research" {
        screen.fill(22, y, 34, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            23,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(23, y, '3', muted);
        screen.text(24, y, " Research", muted_plain);
    }
    if active == "4 Theses" {
        screen.fill(34, y, 44, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            35,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(35, y, '4', muted);
        screen.text(36, y, " Theses", muted_plain);
    }
    if active == "5 Ask" {
        screen.fill(44, y, 51, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            45,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(45, y, '5', muted);
        screen.text(46, y, " Ask", muted_plain);
    }
    if active == "6 Decisions" {
        screen.fill(51, y, 64, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            52,
            y,
            active,
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
    } else {
        screen.put(52, y, '6', muted);
        screen.text(53, y, " Decisions", muted_plain);
    }
    let cluster = "●  data 1d  openrouter  $0.00  c Settings  ? help · g go ";
    let mut cx = w - 1 - cluster.chars().count();
    screen.put(cx, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    cx += 1;
    for (part, style) in [
        ("  ", panel),
        ("data 1d", plain),
        ("  ", panel),
        ("openrouter", plain),
        ("  ", panel),
        ("$0.00", plain),
    ] {
        cx = screen.text(cx, y, part, style);
    }
    if active == "c Settings" {
        // The settings tab chips over the cluster.
        screen.put(cx, y, ' ', panel);
        screen.fill(cx + 1, y, cx + 13, y + 1, Style::DEFAULT.bg(color::BLUE_BG));
        screen.text(
            cx + 2,
            y,
            "c Settings",
            Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
        );
        cx += 13;
        cx = screen.text(cx, y, " ", panel);
    } else {
        cx = screen.text(cx, y, "  ", panel);
        cx = screen.text(cx, y, "c", muted);
        cx = screen.text(cx, y, " Settings", Style::fg(color::MUTED).bg(color::PANEL));
        cx = screen.text(cx, y, "  ", panel);
    }
    cx = screen.text(cx, y, "?", Style::fg(color::MUTED).bg(color::PANEL));
    screen.text(
        cx,
        y,
        " help · g go",
        Style::fg(color::MUTED).bg(color::PANEL),
    );
    screen.put(w - 2, y, ' ', panel);
    screen.put(w - 1, y, ' ', Style::DEFAULT.bg(color::BLACK));
}
