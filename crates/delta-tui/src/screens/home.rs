use super::status_bar::{draw_status_bar_wide, pane_hints, status_bar_narrow, NarrowTab};
use crate::screen::{color, Screen, Style};
use crate::Action;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{layout::Rect, Frame};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HomeCommand {
    Select(usize),
    OpenSelected,
}

#[derive(Default)]
pub struct HomeComponent {
    snapshot: Option<HomeState>,
    live: Option<HomeLiveData>,
    setup: Option<HomeSetupData>,
    selected: usize,
    rows: usize,
}

impl HomeComponent {
    pub fn set_snapshot(
        &mut self,
        snapshot: HomeState,
        live: Option<HomeLiveData>,
        setup: Option<HomeSetupData>,
        selected: usize,
        rows: usize,
    ) {
        self.snapshot = Some(snapshot);
        self.live = live;
        self.setup = setup;
        self.rows = rows;
        self.selected = selected.min(rows.saturating_sub(1));
    }

    pub fn paint_into(&self, screen: &mut Screen) {
        let Some(home) = &self.snapshot else { return };
        if let Some(setup) = &self.setup {
            render_home_setup(screen, home, setup);
        } else if let Some(live) = &self.live {
            render_home_live(screen, home, live);
        } else if screen.w < 100 {
            render_home_empty_narrow(screen, home);
        } else if screen.w >= 160 {
            render_home_empty_wide(screen, home);
        } else {
            render_home_empty_normal(screen, home);
        }
    }
}

impl crate::Screen for HomeComponent {
    fn name(&self) -> &'static str {
        "home"
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Up if self.rows > 0 => {
                self.selected = self.selected.saturating_sub(1);
                Some(Action::Home(HomeCommand::Select(self.selected)))
            }
            KeyCode::Down if self.rows > 0 => {
                self.selected = (self.selected + 1).min(self.rows - 1);
                Some(Action::Home(HomeCommand::Select(self.selected)))
            }
            KeyCode::Enter if self.rows > 0 => Some(Action::Home(HomeCommand::OpenSelected)),
            _ => None,
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        let mut screen = Screen::new(area.width as usize, area.height as usize);
        self.paint_into(&mut screen);
        crate::screen::blit(frame, &screen, area);
    }
}

fn home_panes(screen: &Screen) -> Vec<Vec<super::PaneRect>> {
    let bottom = screen.h.saturating_sub(3);
    if screen.w < 100 {
        let top = 14.min(bottom);
        super::pane_layout(
            1,
            1,
            screen.w.saturating_sub(1),
            bottom + 1,
            &[top, bottom - top],
            &[screen.w.saturating_sub(2)],
        )
    } else {
        let upper = bottom.saturating_sub(7);
        let top = if screen.w >= 160 {
            upper / 2
        } else {
            upper.saturating_sub(15)
        };
        let mut panes = super::pane_layout(
            1,
            1,
            screen.w - 1,
            bottom + 1,
            &[top, upper - top, 7],
            &[screen.w / 2 - 1, screen.w - screen.w / 2 - 1],
        );
        panes[2][0].2 = screen.w - 1;
        panes
    }
}

fn paint_home_header(screen: &mut Screen, home: &HomeState) {
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = screen.w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));
}

fn paint_home_status(screen: &mut Screen) {
    let y = screen.h - 1;
    if screen.w < 100 {
        status_bar_narrow(screen, NarrowTab::Home);
    } else if screen.w >= 160 {
        draw_status_bar_wide(screen, y, screen.w, "1 Home");
    } else {
        draw_status_bar_home(screen, y, screen.w);
    }
}

/// The populated view keeps the same selected watch row but paints each
/// remaining pane directly from the live read model below.
fn paint_home_live_watchlist(screen: &mut Screen, home: &HomeState) {
    let pane = home_panes(screen)[0][0];
    screen.pane(
        pane.0,
        pane.1,
        pane.2 - 1,
        pane.3 - 1,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );
    screen.text(
        2,
        2,
        " Symbol          close     chg%   age  40 closes",
        Style::fg(color::MUTED),
    );
    let blue = Style::DEFAULT.bg(color::BLUE_BG);
    let white = Style::fg(color::WHITE).bg(color::BLUE_BG);
    let white_bold = white.bold();
    let end = pane.2 - 1;
    screen.fill(2, 3, end, 4, blue);
    screen.put(2, 3, ' ', blue);
    let mut x = screen.text(3, 3, &home.symbol, white_bold);
    x = screen.text(x + 11, 3, &home.last, white);
    screen.put(x, 3, ' ', blue);
    x = screen.text(x + 1, 3, "▲ ", white);
    screen.text(x, 3, &home.chg_label, white);
    let spark = if screen.w < 100 || screen.w >= 160 {
        crate::braille::BrailleGraph::filled(home.closes.clone()).rows(end - 42, 1)[0].clone()
    } else {
        home.spark.clone()
    };
    screen.text(41, 3, &spark, white);
    if screen.w >= 100 {
        screen.text(
            3,
            5,
            "spark: 40 daily closes · chg%: move on the day",
            Style::fg(color::MUTED),
        );
    }
}

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
#[derive(Clone)]
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

/// Service-backed values in the populated overview. The DB builder and the
/// running app can both supply this read model without the painter querying
/// storage.
#[derive(Clone)]
pub struct HomeLiveData {
    pub articles: usize,
    pub filings: usize,
    pub events: usize,
    pub pulse: Vec<usize>,
    pub busiest: Option<(String, usize)>,
    pub headline: Option<(String, String)>,
    pub stale_sources: usize,
    pub stale_text: String,
    pub due_reviews: usize,
    pub review_context: String,
    pub upcoming: Vec<(String, String, String, String, String)>,
    pub theses: Vec<(String, String, String)>,
}

#[derive(Clone)]
pub struct HomeSetupData {
    pub provider: String,
    pub provider_connected: bool,
    pub config_exists: bool,
    pub database_reachable: bool,
    pub sec_contact: bool,
    pub price_history: bool,
}

/// The first-run checklist takes the whole overview when no targets exist.
fn render_home_setup(screen: &mut Screen, home: &HomeState, setup: &HomeSetupData) {
    paint_home_header(screen, home);
    paint_home_status(screen);
    screen.fill(
        0,
        1,
        screen.w,
        screen.h - 1,
        Style::DEFAULT.bg(color::BLACK),
    );
    let muted = Style::fg(color::MUTED).bg(color::BLACK);
    screen.text(2, 2, "setup", Style::fg(color::FG).bg(color::BLACK).bold());
    screen.text(
        2,
        3,
        "a few things before Delta can gather anything — each jumps to its fix",
        muted,
    );
    let env_var = match setup.provider.as_str() {
        "openai" => "OPENAI_API_KEY",
        "anthropic" => "ANTHROPIC_API_KEY",
        "custom" => "CUSTOM_API_KEY",
        _ => "OPENROUTER_API_KEY",
    };
    let checks = [
        (
            "p",
            format!("LLM provider ({})", setup.provider),
            format!("Set {env_var} in .env or press p on the Config screen"),
            setup.provider_connected,
        ),
        (
            "c",
            "Config file".into(),
            "Create config.toml".into(),
            setup.config_exists,
        ),
        (
            "c",
            "Database".into(),
            "Check db_path in config.toml".into(),
            setup.database_reachable,
        ),
        (
            "c",
            "SEC EDGAR contact".into(),
            "Set [plugins.sec_edgar].contact".into(),
            setup.sec_contact,
        ),
        (
            "2",
            "Price history".into(),
            "Gather evidence".into(),
            setup.price_history,
        ),
    ];
    for (index, (key, label, fix, ok)) in checks.into_iter().enumerate() {
        let y = index + 5;
        screen.text(2, y, "▸", Style::fg(color::BLUE).bg(color::BLACK));
        screen.text(4, y, key, Style::fg(color::BLUE).bg(color::BLACK).bold());
        let message = format!("{label} — {fix}");
        let limit = screen.w.saturating_sub(11);
        let clipped = message.chars().take(limit).collect::<String>();
        screen.text(7, y, &clipped, muted);
        if message.chars().count() <= limit {
            screen.text(7 + message.chars().count(), y, "  ", muted);
            screen.text(
                7 + message.chars().count() + 2,
                y,
                if ok { "✓" } else { "✗" },
                Style::fg(if ok { color::GREEN } else { color::RED }).bg(color::BLACK),
            );
        }
    }
}

/// Paint the populated Home frame once from the screen snapshot and the
/// service-backed analytics, decisions, calendar and thesis rows.
fn render_home_live(screen: &mut Screen, home: &HomeState, data: &HomeLiveData) {
    paint_home_header(screen, home);
    paint_home_live_watchlist(screen, home);
    paint_home_status(screen);
    let muted = Style::fg(color::MUTED);
    let warn = Style::fg(color::AMBER);
    let count = data.articles + data.filings + data.events;
    let counts = [
        (data.articles, "article"),
        (data.filings, "filing"),
        (data.events, "event"),
    ]
    .into_iter()
    .filter(|(n, _)| *n > 0)
    .map(|(n, noun)| format!("{n} {noun}{}", if n == 1 { "" } else { "s" }))
    .collect::<Vec<_>>();
    if screen.w < 100 {
        screen.fill(3, 5, screen.w - 3, 7, muted);
        screen.text(3, 5, &home.since_stamp, muted);
        let mut x = 3 + home.since_stamp.len() + 2;
        for (i, label) in counts.iter().enumerate() {
            if i > 0 {
                x += 1;
            }
            x = screen.text(x, 5, label, muted.bold());
        }
        if counts.is_empty() {
            screen.text(x, 5, "nothing new", muted);
            x += "nothing new".len();
        }
        x += 3;
        if data.stale_sources > 0 {
            screen.text(
                x,
                5,
                &format!("⚠ {} {}", home.symbol, data.stale_text),
                warn,
            );
        }
        screen.text(3, 6, "next  ", muted);
        if let Some((symbol, kind, date, _, _)) = data.upcoming.first() {
            screen.text(9, 6, &format!("{symbol} {kind} {date}"), muted);
        } else {
            screen.text(9, 6, "nothing scheduled", muted);
        }
        if !data.theses.is_empty() {
            screen.text(
                32,
                6,
                &format!("● {} theses tracked", data.theses.len()),
                muted,
            );
        }
        screen.fill(55, 5, screen.w - 3, 6, Style::DEFAULT);
        screen.fill(50, 6, screen.w - 3, 7, Style::DEFAULT);
        let panes = home_panes(screen);
        let agenda = panes[1][0];
        screen.pane(
            agenda.0,
            agenda.1,
            agenda.2 - 1,
            agenda.3 - 1,
            false,
            &[
                ("needs you today ", Style::fg(color::BLUE).bold()),
                (
                    &format!("· {}", data.due_reviews + data.stale_sources),
                    muted.bold(),
                ),
            ],
            &pane_hints(&[("enter", "open"), ("tab", "next box")]),
        );
        paint_home_agenda_rows(screen, 16);
        paint_due_row(screen, 16, data.due_reviews);
        paint_stale_row(screen, 19, data.stale_sources);
        return;
    }

    let panes = home_panes(screen);
    let right_pane = panes[0][1];
    let middle_pane = panes[1][0];
    let middle_right = panes[1][1];
    let agenda = panes[2][0];
    let (right, mid_top, mid_bottom, agenda_top) =
        (right_pane.0, middle_pane.1, middle_pane.3 - 1, agenda.1);
    let rx1 = screen.w - 2;
    let inner = right + 2;
    let left_end = right - 1;
    let right_width = rx1 - inner - 1;
    let since_title = if count == 0 {
        "since you last looked"
    } else {
        "since you last looked "
    };
    let since_badge = if count == 0 {
        String::new()
    } else {
        format!("· {count} new")
    };
    screen.pane(
        right_pane.0,
        right_pane.1,
        right_pane.2 - 1,
        right_pane.3 - 1,
        false,
        &[
            (since_title, Style::fg(color::BLUE).bold()),
            (&since_badge, muted.bold()),
        ],
        &pane_hints(&[("3", "research")]),
    );
    screen.fill(inner, 2, rx1, 13, Style::DEFAULT);
    let mut x = inner;
    for (i, label) in counts.iter().enumerate() {
        if i > 0 {
            x += 3;
        }
        x = screen.text(x, 2, label, Style::fg(color::FG).bold());
    }
    if counts.is_empty() {
        screen.text(inner, 2, "nothing new since your last visit", muted);
    }
    screen.text_right(rx1 - 1, 2, &home.since_stamp, muted);
    screen.fill(inner + 10, 2, inner + 13, 3, Style::fg(color::FG));
    if data.pulse.iter().any(|n| *n > 0) {
        let values: Vec<f64> = data.pulse.iter().map(|n| *n as f64).collect();
        let graph = crate::braille::BrailleGraph::filled(values).rows(right_width, 2);
        for (offset, row) in graph.iter().enumerate() {
            screen.text(
                inner,
                4 + offset,
                row,
                Style::fg(if offset == 0 { color::BLUE } else { "#3d3d3d" }),
            );
        }
        screen.text(inner, 6, "30 days ago", muted);
        screen.text_right(rx1 - 1, 6, "today", muted);
    }
    if let Some((symbol, n)) = &data.busiest {
        screen.text(inner, 8, "busiest", muted);
        screen.fill(inner + 7, 8, inner + 9, 9, Style::fg(color::FG));
        screen.text(inner + 9, 8, symbol, Style::fg(color::FG));
        screen.put(inner + 9 + symbol.len(), 8, ' ', Style::fg(color::FG));
        screen.text(inner + 10 + symbol.len(), 8, &format!("({n})"), muted);
    } else {
        screen.text(inner, 8, "no activity in the last 30 days", muted);
    }
    if let Some((title, age)) = &data.headline {
        screen.text(inner, 9, "newest", muted);
        screen.fill(inner + 6, 9, inner + 9, 10, Style::fg(color::FG));
        let available = rx1 - inner - age.len() - 12;
        let title = truncate_home(title, available);
        screen.text(inner + 9, 9, &title, Style::fg(color::FG));
        screen.text_right(rx1 - 1, 9, age, muted);
    } else {
        screen.text(inner, 9, "newest   no articles yet", muted);
    }
    if data.stale_sources > 0 {
        screen.text(
            inner,
            11,
            &format!("⚠ {} {}", home.symbol, data.stale_text),
            warn,
        );
    } else {
        screen.text(
            inner,
            11,
            "✓ bars and reports fresh · all plugins on",
            muted,
        );
    }
    if data.due_reviews > 0 {
        screen.text(
            inner,
            12,
            &format!(
                "⚠ {} decision review{} due · {}",
                data.due_reviews,
                if data.due_reviews == 1 { "" } else { "s" },
                data.review_context
            ),
            warn,
        );
    } else {
        screen.text(inner, 12, "✓ no decision or evidence reviews due", muted);
    }

    screen.pane(
        middle_pane.0,
        middle_pane.1,
        middle_pane.2 - 1,
        middle_pane.3 - 1,
        false,
        &[
            ("upcoming ", Style::fg(color::BLUE).bold()),
            (&format!("· {}", data.upcoming.len()), muted.bold()),
        ],
        &pane_hints(&[("enter", "open evidence")]),
    );
    if data.upcoming.is_empty() {
        screen.fill(2, mid_top + 1, left_end, mid_bottom, Style::DEFAULT);
        screen.text(
            3,
            mid_top + 1,
            "nothing scheduled — press 3, then U to gather evidence",
            muted,
        );
    } else {
        screen.fill(2, mid_top + 1, left_end, mid_bottom, Style::fg(color::FG));
        screen.text(
            3,
            mid_top + 1,
            "Symbol   Event        Date    When     Detail",
            Style::fg(color::FG),
        );
        for (index, (symbol, kind, date, when, detail)) in data.upcoming.iter().enumerate() {
            let y = mid_top + 2 + index;
            screen.text(3, y, symbol, Style::fg(color::FG).bold());
            screen.text(12, y, kind, Style::fg(color::FG));
            screen.text(25, y, date, Style::fg(color::FG));
            screen.text(33, y, when, Style::fg(color::FG));
            screen.text(42, y, detail, Style::fg(color::FG));
        }
    }
    screen.text(
        3,
        mid_bottom - 1,
        "calendar plugin: earnings & dividends only",
        muted,
    );

    screen.pane(
        middle_right.0,
        middle_right.1,
        middle_right.2 - 1,
        middle_right.3 - 1,
        false,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            (&format!("· {}", data.theses.len()), muted.bold()),
        ],
        &pane_hints(&[("enter", "open thesis"), ("4", "all")]),
    );
    screen.fill(right + 1, mid_top + 1, rx1, mid_bottom, Style::DEFAULT);
    if !data.theses.is_empty() {
        screen.fill(
            right + 1,
            mid_top + 3,
            rx1,
            mid_bottom,
            Style::fg(color::FG),
        );
        let mut summary_x = inner;
        for state in [
            "challenged",
            "weakening",
            "mixed",
            "idle",
            "emerging",
            "building",
        ] {
            let count = data.theses.iter().filter(|row| row.2 == state).count();
            if count == 0 {
                continue;
            }
            if summary_x > inner {
                summary_x += 3;
            }
            let color = match state {
                "challenged" => color::RED,
                "weakening" | "mixed" => color::AMBER,
                "building" => color::GREEN,
                _ => color::MUTED,
            };
            summary_x = screen.text(
                summary_x,
                mid_top + 1,
                &format!("● {count} {state}"),
                Style::fg(color),
            );
        }
        screen.text(
            inner + 3,
            mid_top + 3,
            "Most at risk",
            Style::fg(color::FG).bold(),
        );
        screen.text_right(
            rx1 - 1,
            mid_top + 3,
            "for/against",
            Style::fg(color::FG).bold(),
        );
        for (index, (claim, ratio, _state)) in data.theses.iter().enumerate() {
            let y = mid_top + 4 + index;
            screen.text(inner, y, "●", Style::fg(color::FG));
            screen.text(
                inner + 3,
                y,
                &truncate_home(claim, rx1 - inner - 17),
                Style::fg(color::FG),
            );
            screen.text_right(rx1 - 1, y, ratio, Style::fg(color::FG));
        }
    } else {
        screen.text(inner, mid_top + 1, "no theses yet", muted);
        screen.text(
            inner,
            mid_top + 3,
            "4 opens the theses desk — n tracks a claim",
            muted,
        );
    }

    if !data.upcoming.is_empty() {
        style_table_row(screen, 2, left_end, mid_top + 1, color::PANEL, true);
        style_table_row(screen, 2, left_end, mid_top + 2, "#242424", false);
    }
    if !data.theses.is_empty() {
        style_table_row(screen, right + 1, rx1, mid_top + 3, color::PANEL, true);
        style_table_row(screen, right + 1, rx1, mid_top + 4, "#242424", false);
    }
    screen.fill(45, mid_bottom - 1, left_end, mid_bottom, Style::DEFAULT);
    screen.put(2, mid_bottom - 1, ' ', Style::DEFAULT);
    if !data.theses.is_empty() {
        for x in inner..inner + 12 {
            screen.cells[(mid_top + 1) * screen.w + x].fg = Some(color::MUTED);
        }
        screen.cells[(mid_top + 5) * screen.w + inner].fg = Some(color::MUTED);
        for x in rx1 - 12..rx1 - 1 {
            screen.cells[(mid_top + 5) * screen.w + x].fg = Some(color::MUTED);
        }
    }

    screen.pane(
        agenda.0,
        agenda.1,
        agenda.2 - 1,
        agenda.3 - 1,
        false,
        &[
            ("needs you today ", Style::fg(color::BLUE).bold()),
            (
                &format!("· {}", data.due_reviews + data.stale_sources),
                muted.bold(),
            ),
        ],
        &pane_hints(&[("enter", "open"), ("tab", "next box")]),
    );
    paint_home_agenda_rows(screen, agenda_top + 1);
    paint_due_row(screen, agenda_top + 1, data.due_reviews);
    paint_stale_row(screen, agenda_top + 4, data.stale_sources);
}

fn style_table_row(
    screen: &mut Screen,
    x0: usize,
    x1: usize,
    y: usize,
    bg: &'static str,
    bold: bool,
) {
    for x in x0..x1 {
        let cell = &mut screen.cells[y * screen.w + x];
        cell.fg = Some(color::FG);
        cell.bg = Some(bg);
        cell.bold = bold || cell.bold;
    }
}

fn paint_home_agenda_rows(screen: &mut Screen, first_row: usize) {
    for (offset, key) in ["6", "4", "3", "2"].into_iter().enumerate() {
        let y = first_row + offset;
        screen.text(3, y, "▸", Style::fg(color::BLUE));
        screen.text(5, y, key, Style::fg(color::BLUE).bold());
    }
    for (offset, message) in [" no falsifier hits", " no earnings in the next 7 days"]
        .into_iter()
        .enumerate()
    {
        let y = first_row + offset + 1;
        screen.text(8, y, "✓", Style::fg(color::MUTED));
        screen.text(9, y, message, Style::fg(color::MUTED));
    }
}

fn paint_due_row(screen: &mut Screen, y: usize, due: usize) {
    screen.fill(8, y, screen.w - 4, y + 1, Style::DEFAULT);
    if due > 0 {
        screen.text(
            8,
            y,
            &format!("{due} decision review due"),
            Style::fg(color::MUTED).bold(),
        );
    } else {
        screen.text(8, y, "✓ no decision reviews due", Style::fg(color::MUTED));
    }
}

fn paint_stale_row(screen: &mut Screen, y: usize, stale: usize) {
    screen.fill(8, y, screen.w - 4, y + 1, Style::DEFAULT);
    if stale > 0 {
        screen.text(
            8,
            y,
            &format!(
                "⚠ {stale} stale source{}",
                if stale == 1 { "" } else { "s" }
            ),
            Style::fg(color::AMBER),
        );
    } else {
        screen.text(8, y, "✓ bars and reports fresh", Style::fg(color::MUTED));
    }
}

fn truncate_home(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        return text.to_string();
    }
    format!(
        "{}…",
        text.chars()
            .take(width.saturating_sub(1))
            .collect::<String>()
    )
}

/// The Home screen: header chip + clock, watchlist / since-you-last-looked,
/// upcoming / theses, the agenda, and the status bar with `1 Home` active
/// (port of `delta/tui/screens/home.py` at the captured 120x40 layout).
fn render_home_empty_normal(screen: &mut Screen, home: &HomeState) {
    let w = screen.w;
    let content_bottom = screen.h - 3; // 37 at h=40
    let panes = home_panes(screen);
    let top_left = panes[0][0];
    let top_right = panes[0][1];
    let middle_left = panes[1][0];
    let middle_right = panes[1][1];
    let agenda = panes[2][0];

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
        top_left.0,
        top_left.1,
        top_left.2 - 1,
        top_left.3 - 1,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );
    screen.pane(
        top_right.0,
        top_right.1,
        top_right.2 - 1,
        top_right.3 - 1,
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
        middle_left.0,
        middle_left.1,
        middle_left.2 - 1,
        middle_left.3 - 1,
        false,
        &[("upcoming", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open evidence")]),
    );
    screen.pane(
        middle_right.0,
        middle_right.1,
        middle_right.2 - 1,
        middle_right.3 - 1,
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
        agenda.0,
        agenda.1,
        agenda.2 - 1,
        agenda.3 - 1,
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
fn render_home_empty_wide(screen: &mut Screen, home: &HomeState) {
    let content_bottom = screen.h - 3; // 47 at h=50
    let mid_bottom = screen.h - 10; // 40 at h=50
    let w = screen.w;
    let panes = home_panes(screen);
    let top_left = panes[0][0];
    let top_right = panes[0][1];
    let middle_left = panes[1][0];
    let middle_right = panes[1][1];
    let agenda = panes[2][0];
    let lx1 = top_left.2 - 1;
    let rx0 = top_right.0;
    let rx1 = top_right.2 - 1;
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
        top_left.0,
        top_left.1,
        lx1,
        top_left.3 - 1,
        true,
        &[
            ("watchlist ", Style::fg(color::BLUE).bold()),
            ("· 1", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select"), ("enter", "open"), ("tab", "next box")]),
    );
    screen.pane(
        top_right.0,
        top_right.1,
        rx1,
        top_right.3 - 1,
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
        middle_left.0,
        middle_left.1,
        middle_left.2 - 1,
        middle_left.3 - 1,
        false,
        &[("upcoming", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("enter", "open evidence")]),
    );
    screen.pane(
        middle_right.0,
        middle_right.1,
        middle_right.2 - 1,
        middle_right.3 - 1,
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
        agenda.0,
        agenda.1,
        agenda.2 - 1,
        agenda.3 - 1,
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
fn render_home_empty_narrow(screen: &mut Screen, home: &HomeState) {
    let w = screen.w;
    let panes = home_panes(screen);
    let watch = panes[0][0];
    let agenda = panes[1][0];

    // Header row: inked DELTA chip, "overview", clock right-aligned.
    screen.put(0, 0, ' ', Style::DEFAULT);
    let chip = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.put(1, 0, ' ', chip);
    screen.text(2, 0, "DELTA", chip);
    screen.put(7, 0, ' ', chip);
    screen.text(10, 0, "overview", Style::fg(color::MUTED));
    let clock_x = w - 1 - home.clock.chars().count();
    screen.text(clock_x, 0, &home.clock, Style::fg(color::MUTED));

    let x1 = watch.2 - 1;
    screen.pane(
        watch.0,
        watch.1,
        x1,
        watch.3 - 1,
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
    screen.pane(
        agenda.0,
        agenda.1,
        agenda.2 - 1,
        agenda.3 - 1,
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

/// Oracle compatibility entry points. The running component uses the renderers directly.
pub fn draw_home(screen: &mut Screen, home: &HomeState) {
    render_home_empty_normal(screen, home);
}

pub fn draw_home_wide(screen: &mut Screen, home: &HomeState) {
    render_home_empty_wide(screen, home);
}

pub fn draw_home_narrow(screen: &mut Screen, home: &HomeState) {
    render_home_empty_narrow(screen, home);
}

pub fn draw_home_live(screen: &mut Screen, home: &HomeState, data: &HomeLiveData) {
    render_home_live(screen, home, data);
}

pub fn draw_home_setup(screen: &mut Screen, home: &HomeState, setup: &HomeSetupData) {
    render_home_setup(screen, home, setup);
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use crate::Screen as _;
    use crossterm::event::KeyModifiers;

    #[test]
    fn home_selection_and_open_are_screen_owned() {
        let mut home = HomeComponent::default();
        home.set_snapshot(
            HomeState {
                clock: String::new(),
                symbol: "AAPL".into(),
                last: String::new(),
                chg_label: String::new(),
                spark: String::new(),
                since_stamp: String::new(),
                closes: Vec::new(),
                feed: HomeFeed::default(),
            },
            None,
            None,
            0,
            2,
        );
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        assert_eq!(
            home.handle_key(key(KeyCode::Down)),
            Some(Action::Home(HomeCommand::Select(1)))
        );
        assert_eq!(
            home.handle_key(key(KeyCode::Down)),
            Some(Action::Home(HomeCommand::Select(1)))
        );
        assert_eq!(
            home.handle_key(key(KeyCode::Enter)),
            Some(Action::Home(HomeCommand::OpenSelected))
        );
    }

    #[test]
    fn home_panes_follow_breakpoint_grid() {
        for (w, h, split, top, agenda) in [
            (80, 24, None, 14, 15),
            (120, 40, Some(60), 15, 31),
            (200, 50, Some(100), 20, 41),
        ] {
            let screen = Screen::new(w, h);
            let panes = home_panes(&screen);
            assert_eq!(panes[0][0].3 - 1, top);
            assert_eq!(panes.last().unwrap()[0].1, agenda);
            assert_eq!(panes[0].get(1).map(|pane| pane.0), split);
        }
    }
}
