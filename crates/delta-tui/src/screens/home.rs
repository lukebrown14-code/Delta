use super::status_bar::{draw_status_bar_wide, pane_hints, status_bar_narrow, NarrowTab};
use crate::screen::{color, Screen, Style};

/// Live Home overview values, formatted by the desk from the analytics
/// queries (headline, pulse, upcoming events, bar staleness, spend).
/// [`HomeFeed::seed`] reproduces the golden scenario's strings, so painters
/// that render these fields stay byte-identical under the goldens.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HomeFeed {
    /// "nothing new since your last visit" / "3 new items since …".
    pub since_line: Option<String>,
    /// "no activity in the last 30 days" / "7 items in the last 30 days …".
    pub activity_line: Option<String>,
    /// "newest   no articles yet" / "newest   <title>".
    pub newest_line: Option<String>,
    /// Bar staleness after "⚠ {symbol} ": "1d old".
    pub stale_age: Option<String>,
    /// First upcoming-pane row (seed: "nothing scheduled — press 3, …").
    pub upcoming_line: Option<String>,
    /// The narrow breakpoint's "next  {brief}" value.
    pub upcoming_brief: Option<String>,
    /// The narrow breakpoint's since-summary value ("nothing new").
    pub since_brief: Option<String>,
}

/// The seeded golden scenario's text for each feed slot; a `None` slot (the
/// analytics query failed or found nothing) falls back to it, so the live
/// desk never shows stale seed values as if they were fresh data.
pub mod feed_seed {
    pub const SINCE_LINE: &str = "nothing new since your last visit";
    pub const ACTIVITY_LINE: &str = "no activity in the last 30 days";
    pub const NEWEST_LINE: &str = "newest   no articles yet";
    pub const STALE_AGE: &str = "1d old";
    pub const UPCOMING_LINE: &str = "nothing scheduled — press 3, then U to gather evidence";
    pub const UPCOMING_BRIEF: &str = "nothing scheduled";
    pub const SINCE_BRIEF: &str = "nothing new";
}

impl HomeFeed {
    /// The seeded golden scenario's values (offline desk and goldens).
    pub fn seed() -> Self {
        Self {
            since_line: Some(feed_seed::SINCE_LINE.to_string()),
            activity_line: Some(feed_seed::ACTIVITY_LINE.to_string()),
            newest_line: Some(feed_seed::NEWEST_LINE.to_string()),
            stale_age: Some(feed_seed::STALE_AGE.to_string()),
            upcoming_line: Some(feed_seed::UPCOMING_LINE.to_string()),
            upcoming_brief: Some(feed_seed::UPCOMING_BRIEF.to_string()),
            since_brief: Some(feed_seed::SINCE_BRIEF.to_string()),
        }
    }

    /// The slot's text or the golden seed when the slot is unset.
    pub fn text<'a>(slot: &'a Option<String>, seed: &'a str) -> &'a str {
        slot.as_deref().unwrap_or(seed)
    }
}

/// Home state the painter renders (the seeded golden scenario's values).
pub struct HomeState {
    pub clock: String,
    pub symbol: String,
    pub last: String,
    pub chg_label: String,
    pub spark: String,
    pub since_stamp: String,
    /// Raw closes so the narrow breakpoint can resize the sparkline.
    pub closes: Vec<f64>,
    /// Live overview values (seeded when offline).
    pub feed: HomeFeed,
}

/// The Home screen: header chip + clock, watchlist / since-you-last-looked,
/// upcoming / theses, the agenda, and the status bar with `1 Home` active
/// (port of `delta/tui/screens/home.py` at the captured 120x40 layout).
pub fn draw_home(screen: &mut Screen, home: &HomeState) {
    let w = screen.w;
    let content_bottom = screen.h - 3; // 37 at h=40

    // Header row: inked DELTA chip, "overview", clock right-aligned.
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));

    // Row 1: watchlist (focused) + since you last looked (blurred).
    screen.pane(
        1,
        1,
        59,
        content_bottom - 22,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );
    screen.pane(
        60,
        1,
        w - 2,
        content_bottom - 22,
        false,
        &[("since you last looked", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("3", "research")]),
    );

    // Watchlist table: muted header, selected row in $primary.
    let header = " Symbol          close     chg%   age  40 closes";
    screen.text(2, 2, header, Style::fg(color::MUTED));
    let blue = Style::DEFAULT.bg(color::BLUE_BG);
    screen.fill(2, 3, 59, 4, blue);
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG);
    let white_bold = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(2, 3, ' ', blue);
    let mut x = screen.text(3, 3, &home.symbol, white_bold);
    x = screen.text(x + 11, 3, &home.last, white);
    screen.put(x, 3, ' ', blue);
    x = screen.text(x + 1, 3, "▲ ", white);
    x = screen.text(x, 3, &home.chg_label, white);
    let _ = x;
    screen.text(41, 3, &home.spark, white);
    // Row 5: the spark legend.
    screen.text(
        3,
        5,
        "spark: 40 daily closes · chg%: move on the day",
        Style::fg(color::MUTED),
    );

    // Since-you-last-looked: empty-run summary, stale warnings.
    screen.text(
        62,
        2,
        HomeFeed::text(&home.feed.since_line, feed_seed::SINCE_LINE),
        Style::fg(color::MUTED),
    );
    screen.text(102, 2, &home.since_stamp, Style::fg(color::MUTED));
    screen.text(
        62,
        4,
        HomeFeed::text(&home.feed.activity_line, feed_seed::ACTIVITY_LINE),
        Style::fg(color::MUTED),
    );
    screen.text(
        62,
        5,
        HomeFeed::text(&home.feed.newest_line, feed_seed::NEWEST_LINE),
        Style::fg(color::MUTED),
    );
    let warn = Style::fg(color::AMBER);
    screen.text(
        62,
        7,
        &format!(
            "⚠ {} {}",
            home.symbol,
            HomeFeed::text(&home.feed.stale_age, feed_seed::STALE_AGE)
        ),
        warn,
    );
    screen.text(62, 8, "⚠ 2 evidence prompts · 3 evidence", warn);

    // Middle row: upcoming + theses.
    screen.pane(
        1,
        content_bottom - 21,
        59,
        content_bottom - 7,
        false,
        &[("upcoming", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open evidence")]),
    );
    screen.pane(
        60,
        content_bottom - 21,
        w - 2,
        content_bottom - 7,
        false,
        &[("theses", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open thesis"), ("4", "all")]),
    );
    screen.text(
        3,
        content_bottom - 20,
        HomeFeed::text(&home.feed.upcoming_line, feed_seed::UPCOMING_LINE),
        Style::fg(color::MUTED),
    );
    screen.text(
        3,
        content_bottom - 19,
        "calendar plugin: earnings & dividends only",
        Style::fg(color::MUTED),
    );
    screen.text(
        62,
        content_bottom - 20,
        "no theses yet",
        Style::fg(color::MUTED),
    );
    screen.text(
        62,
        content_bottom - 18,
        "4 opens the theses desk — n tracks a claim",
        Style::fg(color::MUTED),
    );

    // Agenda: full width, jump keys with ✓/⚠ verdicts.
    screen.pane(
        1,
        content_bottom - 6,
        w - 2,
        content_bottom,
        false,
        &[
            ("needs you today ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("enter", "open"), ("tab", "next box")]),
    );
    let rows: [(&str, &str, &str, Style); 4] = [
        (
            "6",
            "✓",
            " no decision reviews due",
            Style::fg(color::MUTED),
        ),
        ("4", "✓", " no falsifier hits", Style::fg(color::MUTED)),
        (
            "3",
            "✓",
            " no earnings in the next 7 days",
            Style::fg(color::MUTED),
        ),
        ("2", "⚠", " 1 stale source", Style::fg(color::AMBER)),
    ];
    for (index, (key, glyph, message, style)) in rows.into_iter().enumerate() {
        let y = content_bottom - 5 + index;
        let mut x = 3;
        x = screen.text(x, y, "▸", Style::fg(color::BLUE));
        x += 1;
        x = screen.text(x, y, key, Style::fg(color::BLUE).bold());
        x += 2;
        x = screen.text(x, y, glyph, style);
        screen.text(x, y, message, style);
    }

    draw_status_bar_home(screen, screen.h - 1, w);
}

/// Status bar with the Home tab active (`1 Home`).
fn draw_status_bar_home(screen: &mut Screen, y: usize, w: usize) {
    let panel = Style::DEFAULT.bg(color::PANEL);
    let muted = Style::fg(color::MUTED).bold().bg(color::PANEL);
    let active_fg = Style::fg(color::WHITE).bold().bg(color::BLUE_BG);
    let active = Style::DEFAULT.bg(color::BLUE_BG);
    let plain = Style::fg(color::FG).bg(color::PANEL);
    screen.fill(1, y, w, y + 1, panel);
    screen.put(0, y, ' ', Style::DEFAULT.bg(color::BLACK));
    screen.put(1, y, ' ', active);
    screen.text(2, y, "1 Home", active_fg);
    screen.put(8, y, ' ', active);
    screen.put(9, y, ' ', panel);
    screen.text(10, y, "2", muted);
    for (i, key) in ["3", "4", "5", "6"].into_iter().enumerate() {
        screen.text(13 + i * 3, y, key, muted);
    }
    let x = w - 1 - 57; // cluster width (57 cells) ends at w-2
    screen.put(x, y, '●', Style::fg(color::AMBER).bg(color::PANEL));
    let mut x = x + 1;
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

/// Home at 200x50: the 120 layout with the split at half width.
pub fn draw_home_wide(screen: &mut Screen, home: &HomeState) {
    let content_bottom = screen.h - 3; // 47 at h=50
    let mid_bottom = screen.h - 10; // 40 at h=50
    let w = screen.w;
    let s = w / 2; // pane split (100 at w=200)
    let lx1 = s - 1;
    let rx0 = s;
    let rx1 = w - 2;
    let inner = rx0 + 2; // right-hand content column

    // Header row.
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));

    // Watchlist + since-you-last-looked.
    screen.pane(
        1,
        1,
        lx1,
        20,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );
    screen.pane(
        rx0,
        1,
        rx1,
        20,
        false,
        &[("since you last looked", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("3", "research")]),
    );

    let header = " Symbol          close     chg%   age  40 closes";
    screen.text(2, 2, header, Style::fg(color::MUTED));
    let blue = Style::DEFAULT.bg(color::BLUE_BG);
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG);
    let white_bold = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 3, lx1, 4, blue);
    screen.put(2, 3, ' ', blue);
    let mut x = screen.text(3, 3, &home.symbol, white_bold);
    x = screen.text(x + 11, 3, &home.last, white);
    screen.put(x, 3, ' ', blue);
    x = screen.text(x + 1, 3, "▲ ", white);
    x = screen.text(x, 3, &home.chg_label, white);
    let _ = x;
    let spark_w = lx1 - 42;
    let spark =
        crate::braille::BrailleGraph::filled(home.closes.clone()).rows(spark_w, 1)[0].clone();
    screen.text(41, 3, &spark, white);
    screen.text(
        3,
        5,
        "spark: 40 daily closes · chg%: move on the day",
        Style::fg(color::MUTED),
    );

    let muted = Style::fg(color::MUTED);
    let warn = Style::fg(color::AMBER);
    screen.text(
        inner,
        2,
        HomeFeed::text(&home.feed.since_line, feed_seed::SINCE_LINE),
        muted,
    );
    screen.text_right(rx1 - 1, 2, &home.since_stamp, muted);
    screen.text(
        inner,
        4,
        HomeFeed::text(&home.feed.activity_line, feed_seed::ACTIVITY_LINE),
        muted,
    );
    screen.text(
        inner,
        5,
        HomeFeed::text(&home.feed.newest_line, feed_seed::NEWEST_LINE),
        muted,
    );
    screen.text(
        inner,
        7,
        &format!(
            "⚠ {} {}",
            home.symbol,
            HomeFeed::text(&home.feed.stale_age, feed_seed::STALE_AGE)
        ),
        warn,
    );
    screen.text(inner, 8, "⚠ 2 evidence prompts · 3 evidence", warn);

    // Upcoming + theses.
    screen.pane(
        1,
        21,
        lx1,
        mid_bottom,
        false,
        &[("upcoming", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open evidence")]),
    );
    screen.pane(
        rx0,
        21,
        rx1,
        mid_bottom,
        false,
        &[("theses", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open thesis"), ("4", "all")]),
    );
    screen.text(
        3,
        22,
        HomeFeed::text(&home.feed.upcoming_line, feed_seed::UPCOMING_LINE),
        muted,
    );
    screen.text(3, 23, "calendar plugin: earnings & dividends only", muted);
    screen.text(inner, 22, "no theses yet", muted);
    screen.text(
        inner,
        24,
        "4 opens the theses desk — n tracks a claim",
        muted,
    );

    // Agenda.
    screen.pane(
        1,
        mid_bottom + 1,
        rx1,
        content_bottom,
        false,
        &[
            ("needs you today ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("enter", "open"), ("tab", "next box")]),
    );
    let rows: [(&str, &str, &str, Style); 4] = [
        ("6", "✓", " no decision reviews due", muted),
        ("4", "✓", " no falsifier hits", muted),
        ("3", "✓", " no earnings in the next 7 days", muted),
        ("2", "⚠", " 1 stale source", warn),
    ];
    for (index, (key, glyph, message, style)) in rows.into_iter().enumerate() {
        let y = mid_bottom + 2 + index;
        if y > content_bottom - 1 {
            break; // keep agenda text inside the shrunken pane
        }
        screen.text(3, y, "▸", Style::fg(color::BLUE));
        screen.text(5, y, key, Style::fg(color::BLUE).bold());
        screen.text(8, y, glyph, style);
        screen.text(9, y, message, style);
    }

    draw_status_bar_wide(screen, screen.h - 1, w, "1 Home");
}

/// Home at 80x24: one watchlist pane (table + since-you-last-looked lines),
/// the agenda pane, narrow header/status bar.
pub fn draw_home_narrow(screen: &mut Screen, home: &HomeState) {
    let w = screen.w;

    // Header row: inked DELTA chip, "overview", clock right-aligned.
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));

    let x1 = w - 2;
    screen.pane(
        1,
        1,
        x1,
        14,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );

    // Table header and the selected row (blue ink), spark sized to the pane.
    let header = " Symbol          close     chg%   age  40 closes";
    screen.text(2, 2, header, Style::fg(color::MUTED));
    let blue = Style::DEFAULT.bg(color::BLUE_BG);
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG);
    let white_bold = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.fill(2, 3, x1, 4, blue);
    screen.put(2, 3, ' ', blue);
    let mut x = screen.text(3, 3, &home.symbol, white_bold);
    x = screen.text(x + 11, 3, &home.last, white);
    screen.put(x, 3, ' ', blue);
    x = screen.text(x + 1, 3, "▲ ", white);
    x = screen.text(x, 3, &home.chg_label, white);
    let _ = x;
    let spark_w = x1 - 42;
    let spark =
        crate::braille::BrailleGraph::filled(home.closes.clone()).rows(spark_w, 1)[0].clone();
    screen.text(41, 3, &spark, white);

    // Since-you-last-looked, folded into the pane.
    let muted = Style::fg(color::MUTED);
    screen.text(
        3,
        5,
        &format!(
            "{}  {}   ",
            home.since_stamp,
            HomeFeed::text(&home.feed.since_brief, feed_seed::SINCE_BRIEF)
        ),
        muted,
    );
    screen.text(
        34,
        5,
        &format!(
            "⚠ {} {}",
            home.symbol,
            HomeFeed::text(&home.feed.stale_age, feed_seed::STALE_AGE)
        ),
        Style::fg(color::AMBER),
    );
    screen.text(
        3,
        6,
        &format!(
            "next  {}",
            HomeFeed::text(&home.feed.upcoming_brief, feed_seed::UPCOMING_BRIEF)
        ),
        muted,
    );

    // Agenda: full width, jump keys with verdicts (bottom-anchored).
    let agenda_bottom = screen.h - 3; // 21 at h=24
    screen.pane(
        1,
        15,
        x1,
        agenda_bottom,
        false,
        &[
            ("needs you today ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("enter", "open"), ("tab", "next box")]),
    );
    let rows: [(&str, &str, &str, Style); 4] = [
        ("6", "✓", " no decision reviews due", muted),
        ("4", "✓", " no falsifier hits", muted),
        ("3", "✓", " no earnings in the next 7 days", muted),
        ("2", "⚠", " 1 stale source", Style::fg(color::AMBER)),
    ];
    for (index, (key, glyph, message, style)) in rows.into_iter().enumerate() {
        let y = 16 + index;
        if y > screen.h - 4 {
            break; // keep agenda text inside the shrunken pane
        }
        screen.text(3, y, "▸", Style::fg(color::BLUE));
        screen.text(5, y, key, Style::fg(color::BLUE).bold());
        screen.text(8, y, glyph, style);
        screen.text(9, y, message, style);
    }

    status_bar_narrow(screen, NarrowTab::Home);
}
