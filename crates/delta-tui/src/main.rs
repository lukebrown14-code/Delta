//! Delta TUI binary: the real screens over the terminal, desk data from
//! `config.toml` + the configured DB, background quote/ingest workers over
//! the action bus. The app library lives in `delta_tui` (see `lib.rs`).

use std::io::Stdout;
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use delta_tui::desk::Desk;
use delta_tui::screen::{color, Screen, Style};
use delta_tui::screens::{
    draw_ask, draw_ask_narrow, draw_ask_wide, draw_decisions, draw_decisions_narrow,
    draw_decisions_wide, draw_glossary_overlay, draw_home, draw_home_narrow, draw_home_wide,
    draw_research, draw_research_narrow, draw_research_wide, draw_settings, draw_settings_narrow,
    draw_settings_wide, draw_theses, draw_theses_narrow, draw_theses_wide, draw_watchlist,
    draw_watchlist_narrow, draw_watchlist_wide,
};
use delta_tui::{is_quit_key, workers, Action, Component};

/// The seven panes: 1-6 plus `c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Home,
    Watchlist,
    Research,
    Theses,
    Ask,
    Decisions,
    Settings,
}

/// Frame-time counters, printed on exit (R4 instrumentation hook).
#[derive(Debug, Default)]
struct FrameStats {
    count: u64,
    total_us: u128,
    max_us: u128,
}

impl FrameStats {
    fn record(&mut self, us: u128) {
        self.count += 1;
        self.total_us += us;
        self.max_us = self.max_us.max(us);
    }

    fn report(&self) -> String {
        let avg = if self.count > 0 {
            self.total_us / self.count as u128
        } else {
            0
        };
        format!(
            "frames {} · avg {avg:.0}µs · max {}µs",
            self.count, self.max_us
        )
    }
}

/// The app: desk data + active pane, painting through the golden `Screen`
/// model so the live frame is cell-for-cell what the goldens capture.
struct App {
    desk: Desk,
    tab: Tab,
    glossary: bool,
    status: String,
    frame_stats: FrameStats,
    quit: bool,
}

impl App {
    fn paint(&mut self, screen: &mut Screen) {
        let w = screen.w;
        let wide = w >= 160;
        let narrow = w < delta_tui::NARROW_WIDTH as usize;
        macro_rules! route {
            ($wide:expr, $normal:expr, $narrow:expr) => {
                if wide {
                    $wide
                } else if narrow {
                    $narrow
                } else {
                    $normal
                }
            };
        }
        match self.tab {
            Tab::Home => route!(
                draw_home_wide(screen, &self.desk.home_state()),
                draw_home(screen, &self.desk.home_state()),
                draw_home_narrow(screen, &self.desk.home_state())
            ),
            Tab::Watchlist => route!(
                draw_watchlist_wide(screen, &self.desk.watch_state()),
                draw_watchlist(screen, &self.desk.watch_state()),
                draw_watchlist_narrow(screen, &self.desk.watch_state())
            ),
            Tab::Research => route!(
                draw_research_wide(screen),
                draw_research(screen),
                draw_research_narrow(screen)
            ),
            Tab::Theses => route!(
                draw_theses_wide(screen),
                draw_theses(screen),
                draw_theses_narrow(screen)
            ),
            Tab::Ask => route!(
                draw_ask_wide(screen),
                draw_ask(screen),
                draw_ask_narrow(screen)
            ),
            Tab::Decisions => route!(
                draw_decisions_wide(screen),
                draw_decisions(screen),
                draw_decisions_narrow(screen)
            ),
            Tab::Settings => route!(
                draw_settings_wide(screen),
                draw_settings(screen),
                draw_settings_narrow(screen)
            ),
        }
        if self.glossary && self.tab == Tab::Watchlist {
            draw_glossary_overlay(screen, &self.desk.watch_state());
        }
        self.paint_status_overlay(screen);
    }

    /// A right-aligned live badge on the last row: quotes, gather status and
    /// the data source. Painted after the status bar, so it wins the cells.
    fn paint_status_overlay(&mut self, screen: &mut Screen) {
        let y = screen.h.saturating_sub(1);
        let mut label = String::new();
        if !self.desk.live.is_empty() {
            label.push_str(&format!("● live {}  ", self.desk.live.len()));
        }
        match &self.desk.source {
            delta_tui::desk::Source::Real(db) => {
                label.push_str(&format!("db {}", db.display()));
            }
            delta_tui::desk::Source::Offline => label.push_str("offline seed"),
        }
        if !self.status.is_empty() {
            label.push_str(&format!("  ·  {}", self.status));
        }
        let style = Style::fg(color::GREEN);
        let start = screen.w.saturating_sub(label.chars().count());
        screen.text(start, y, &label, style);
    }
}

impl Component for App {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if is_quit_key(key) {
            return Some(Action::Quit);
        }
        match key.code {
            KeyCode::Esc => self.glossary = false,
            KeyCode::Char('1') => self.tab = Tab::Home,
            KeyCode::Char('2') => self.tab = Tab::Watchlist,
            KeyCode::Char('3') => self.tab = Tab::Research,
            KeyCode::Char('4') => self.tab = Tab::Theses,
            KeyCode::Char('5') => self.tab = Tab::Ask,
            KeyCode::Char('6') => self.tab = Tab::Decisions,
            KeyCode::Char('c') => self.tab = Tab::Settings,
            KeyCode::Char('g') if self.tab == Tab::Watchlist => self.glossary = !self.glossary,
            KeyCode::Char('h') | KeyCode::Left if self.tab == Tab::Watchlist => {
                self.desk.cycle_range(-1);
            }
            KeyCode::Char('l') | KeyCode::Right if self.tab == Tab::Watchlist => {
                self.desk.cycle_range(1);
            }
            KeyCode::Char(',') if !self.desk.instruments.is_empty() => {
                self.desk.cycle_instrument(-1);
            }
            KeyCode::Char('.') if !self.desk.instruments.is_empty() => {
                self.desk.cycle_instrument(1);
            }
            KeyCode::Char('U') => return Some(Action::Gather),
            _ => {}
        }
        None
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::Quotes(prices) => self.desk.live = prices,
            Action::Metrics { instrument, rows } => {
                self.desk.metrics.insert(instrument, rows);
            }
            Action::Ingested(counts) => {
                // Per-source counts, RSS/SEC/bars each visible.
                let parts: Vec<String> = counts
                    .iter()
                    .filter(|(_, n)| **n > 0)
                    .map(|(source, n)| format!("{source} {n}"))
                    .collect();
                self.status = if parts.is_empty() {
                    "ingest: nothing new".to_string()
                } else {
                    format!("ingest: {}", parts.join(" · "))
                };
                self.desk.last_ingest = Some(counts);
                if let delta_tui::desk::Source::Real(db) = &self.desk.source {
                    let db = db.clone();
                    self.desk.reload_from(&db);
                }
            }
            Action::Status(msg) => self.status = msg,
            Action::HomeRefresh(feed) => self.desk.feed = feed,
            Action::Noop
            | Action::OpenDialog(_)
            | Action::CloseDialog
            | Action::Goto(_)
            | Action::Gather => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        if area.width < 4 || area.height < 4 {
            return; // degenerate terminal; the painters assume a status bar
        }
        let mut screen = Screen::new(area.width as usize, area.height as usize);
        self.paint(&mut screen);
        blit(frame, &screen, area);
    }
}

/// Blit the golden `Screen` cell grid into the ratatui buffer.
fn blit(frame: &mut Frame, screen: &Screen, area: ratatui::layout::Rect) {
    for y in 0..screen.h.min(area.height as usize) {
        for x in 0..screen.w.min(area.width as usize) {
            let cell = &screen.cells[y * screen.w + x];
            let mut style = RStyle::default();
            if let Some(fg) = cell.fg {
                style = style.fg(hex_color(fg));
            }
            if let Some(bg) = cell.bg {
                style = style.bg(hex_color(bg));
            }
            if cell.bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            frame
                .buffer_mut()
                .cell_mut((x as u16, y as u16))
                .expect("cell in bounds")
                .set_char(cell.ch)
                .set_style(style);
        }
    }
}

/// `#rrggbb` (the exporter's resolved tokens) to a ratatui RGB colour.
fn hex_color(hex: &str) -> Color {
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0);
    Color::Rgb(byte(1), byte(3), byte(5))
}

fn main() -> std::io::Result<()> {
    // R4 benchmark entry: `delta --version` never touches the terminal.
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("delta {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut terminal = setup()?;
    let runtime = tokio::runtime::Runtime::new()?;
    let res = runtime.block_on(run(&mut terminal));
    teardown(&mut terminal)?;
    res
}

async fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    let (bus_tx, mut bus_rx) = mpsc::unbounded_channel::<Action>();
    let desk = Desk::open();
    let universe: Vec<delta_core::models::Instrument> = desk
        .instruments
        .iter()
        .map(|d| d.instrument.clone())
        .collect();
    let db_path = match &desk.source {
        delta_tui::desk::Source::Real(db) => Some(db.clone()),
        delta_tui::desk::Source::Offline => None,
    };
    // Quotes hit the network; opt in with DELTA_QUOTES=1.
    let quotes_enabled = std::env::var("DELTA_QUOTES").as_deref() == Ok("1");
    let gather_tx = workers::spawn(bus_tx.clone(), universe, db_path, quotes_enabled);

    let mut app = App {
        desk,
        tab: Tab::Watchlist,
        glossary: false,
        status: String::new(),
        frame_stats: FrameStats::default(),
        quit: false,
    };

    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut clock = tokio::time::interval(Duration::from_secs(1)); // the home clock
    let mut dirty = true; // first frame
    loop {
        if dirty {
            let started = Instant::now();
            let size = terminal.size()?;
            let area = ratatui::layout::Rect::new(0, 0, size.width, size.height);
            app.draw_frame(terminal, area)?;
            app.frame_stats.record(started.elapsed().as_micros());
            dirty = false;
        }
        tokio::select! {
            maybe_event = events.next() => {
                match maybe_event {
                    Some(Ok(Event::Key(key))) => {
                        dirty = true;
                        if let Some(action) = app.handle_key(key) {
                            if action == Action::Gather {
                                let _ = gather_tx.send(());
                            } else {
                                app.update(action);
                            }
                        }
                    }
                    Some(Ok(_)) => {
                        dirty = true;
                    }
                    Some(Err(e)) => return Err(std::io::Error::other(e.to_string())),
                    None => {}
                }
            }
            Some(action) = bus_rx.recv() => {
                dirty = true;
                app.update(action);
            }
            _ = clock.tick() => {
                // The home clock reads the wall clock; repaint once a second.
                if app.tab == Tab::Home {
                    dirty = true;
                }
            }
            _ = tick.tick() => {}
        }
        if app.quit {
            eprintln!("{}", app.frame_stats.report());
            if let Ok(path) = std::env::var("DELTA_FRAME_LOG") {
                let _ = std::fs::write(path, app.frame_stats.report());
            }
            return Ok(());
        }
    }
}

impl App {
    fn draw_frame(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<Stdout>>,
        area: ratatui::layout::Rect,
    ) -> std::io::Result<()> {
        terminal.draw(|frame| self.draw(frame, area))?;
        Ok(())
    }
}

fn setup() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    queue!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout))
}

fn teardown(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use std::collections::BTreeMap;

    use ratatui::backend::TestBackend;

    pub(super) fn app() -> App {
        App {
            desk: Desk::offline(),
            tab: Tab::Watchlist,
            glossary: false,
            status: String::new(),
            frame_stats: FrameStats::default(),
            quit: false,
        }
    }

    #[test]
    fn tab_keys_navigate_all_seven_panes() {
        let mut a = app();
        for (key, tab) in [
            (KeyCode::Char('1'), Tab::Home),
            (KeyCode::Char('2'), Tab::Watchlist),
            (KeyCode::Char('3'), Tab::Research),
            (KeyCode::Char('4'), Tab::Theses),
            (KeyCode::Char('5'), Tab::Ask),
            (KeyCode::Char('6'), Tab::Decisions),
        ] {
            a.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
            assert_eq!(a.tab, tab);
        }
        a.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));
        assert_eq!(a.tab, Tab::Settings);
    }

    #[test]
    fn glossary_toggles_on_watchlist_and_esc_closes() {
        let mut a = app();
        a.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert!(a.glossary);
        a.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!a.glossary);
        // `g` on another pane does not open it.
        a.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        a.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        assert!(!a.glossary);
    }

    #[test]
    fn range_cycling_walks_the_ranges() {
        let mut a = app();
        assert_eq!(a.desk.range(), "1m");
        a.handle_key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::NONE));
        assert_eq!(a.desk.range(), "6m");
        a.handle_key(KeyEvent::new(KeyCode::Char('h'), KeyModifiers::NONE));
        assert_eq!(a.desk.range(), "1m");
    }

    #[test]
    fn quit_keys_produce_quit_action() {
        let mut a = app();
        assert_eq!(
            a.handle_key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE)),
            Some(Action::Quit)
        );
        assert_eq!(
            a.handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Action::Quit)
        );
    }

    #[test]
    fn gather_is_requested_and_statuses_update() {
        let mut a = app();
        assert_eq!(
            a.handle_key(KeyEvent::new(KeyCode::Char('U'), KeyModifiers::NONE)),
            Some(Action::Gather)
        );
        let mut counts = BTreeMap::new();
        counts.insert("bar".to_string(), 5);
        a.update(Action::Ingested(counts));
        assert_eq!(a.status, "ingest: bar 5");
        // Metrics flow into the desk and out through the watch state.
        a.update(Action::Metrics {
            instrument: "US:AAPL".to_string(),
            rows: vec![("Market cap".to_string(), "$3.40T".to_string())],
        });
        let metric = a.desk.watch_state().metric.unwrap();
        assert!(metric
            .values
            .iter()
            .any(|(l, v)| l == "Market cap" && v == "$3.40T"));
    }

    #[test]
    fn live_quotes_flow_into_the_desk() {
        let mut a = app();
        let mut prices = BTreeMap::new();
        prices.insert("US:AAPL".to_string(), 250.0);
        a.update(Action::Quotes(prices));
        assert_eq!(a.desk.live_price(), Some(250.0));
        // And the watchlist paints with the live price as the current value.
        let state = a.desk.watch_state();
        let metric = state.metric.unwrap();
        assert_eq!(metric.current, "250.00");
        assert_eq!(metric.source, "live quote");
    }

    #[test]
    fn every_pane_paints_into_the_terminal_buffer_at_all_breakpoints() {
        for &(w, h) in &[(80u16, 24u16), (120, 40), (200, 50)] {
            for tab in [
                Tab::Home,
                Tab::Watchlist,
                Tab::Research,
                Tab::Theses,
                Tab::Ask,
                Tab::Decisions,
                Tab::Settings,
            ] {
                let mut a = app();
                a.tab = tab;
                let backend = TestBackend::new(w, h);
                let mut terminal = Terminal::new(backend).unwrap();
                terminal.draw(|frame| a.draw(frame, frame.area())).unwrap();
            }
        }
    }

    #[test]
    fn every_tab_fits_non_canonical_terminal_sizes() {
        // The painters must adapt to any terminal, not just the three
        // canonical matrix sizes: the status bar stays on the last row and
        // pane bottoms land on the content row, never past it.
        let sizes = [
            (70u16, 20u16),
            (80, 24),
            (90, 28),
            (100, 30),
            (120, 40),
            (140, 35),
            (159, 45),
            (160, 40),
            (180, 35),
            (200, 50),
            (200, 38),
        ];
        for &(w, h) in &sizes {
            for tab in [
                Tab::Home,
                Tab::Watchlist,
                Tab::Research,
                Tab::Theses,
                Tab::Ask,
                Tab::Decisions,
                Tab::Settings,
            ] {
                let mut a = app();
                a.tab = tab;
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                terminal.draw(|f| a.draw(f, f.area())).unwrap();
                let buf = terminal.backend().buffer();
                let row_text = |y: u16| -> String {
                    (0..w)
                        .map(|x| buf[(x, y)].symbol())
                        .collect::<String>()
                        .trim_end()
                        .to_string()
                };
                let status = row_text(h - 1);
                assert!(
                    status.contains("offline seed"),
                    "status bar missing at {w}x{h} tab {tab:?}: {status:?}"
                );
                // The watchlist panes bottom out on the content row.
                if tab == Tab::Watchlist {
                    let content = row_text(h - 3);
                    assert!(
                        content.contains('\u{2500}') || content.contains('\u{2504}'),
                        "watchlist pane bottom missing at {w}x{h}: {content:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn metrics_rows_render_on_the_watchlist_screen() {
        let mut a = app();
        let rows = vec![
            ("Market cap".to_string(), "$4.86T".to_string()),
            ("P/E".to_string(), "31.2x".to_string()),
            ("Gross margin".to_string(), "48.7%".to_string()),
            ("Free cash flow".to_string(), "$107.72B".to_string()),
            ("Dividend yield".to_string(), "0.3%".to_string()),
        ];
        let mut metrics = BTreeMap::new();
        metrics.insert("US:AAPL".to_string(), rows);
        a.desk.metrics = metrics;
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| a.draw(f, f.area())).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect();
        for probe in [
            "Market cap",
            "$4.86T",
            "P/E",
            "Gross margin",
            "Free cash flow",
            "Dividend yield",
        ] {
            assert!(text.contains(probe), "missing {probe} on screen");
        }
    }
}

#[cfg(test)]
mod home_feed_tests {
    use super::tests::app;
    use super::*;
    use delta_tui::screens::HomeFeed;
    use ratatui::backend::TestBackend;

    #[test]
    fn home_refresh_flows_from_analytics_output_into_the_home_state() {
        let mut a = app();
        // The offline desk starts seeded…
        assert_eq!(a.desk.home_state().feed, HomeFeed::seed());
        // …then the refresh worker's analytics output lands on the bus.
        let mut feed = HomeFeed::seed();
        feed.since_line = Some("3 new items since your last visit".to_string());
        feed.since_brief = Some("3 new".to_string());
        feed.newest_line = Some("newest   Apple announces new chip".to_string());
        feed.stale_age = Some("0d old".to_string());
        a.update(Action::HomeRefresh(feed));
        let state = a.desk.home_state();
        assert_eq!(
            state.feed.since_line.as_deref(),
            Some("3 new items since your last visit")
        );
        assert_eq!(state.feed.since_brief.as_deref(), Some("3 new"));
        assert_eq!(
            state.feed.newest_line.as_deref(),
            Some("newest   Apple announces new chip")
        );
        assert_eq!(state.feed.stale_age.as_deref(), Some("0d old"));
    }

    #[test]
    fn live_feed_values_render_on_the_home_screen() {
        let mut a = app();
        let mut feed = HomeFeed::seed();
        feed.since_line = Some("3 new items since your last visit".to_string());
        feed.since_brief = Some("3 new".to_string());
        feed.newest_line = Some("newest   Apple announces new chip".to_string());
        feed.stale_age = Some("0d old".to_string());
        feed.upcoming_line = Some("Thu 01 Jan  earnings · Q4 results".to_string());
        feed.upcoming_brief = Some("Thu 01 Jan earnings".to_string());
        a.update(Action::HomeRefresh(feed));
        a.tab = Tab::Home;
        // The narrow breakpoint folds the since pane into briefs, not lines.
        for &(w, h) in &[(80u16, 24u16), (120, 40), (200, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| a.draw(f, f.area())).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(ratatui::buffer::Cell::symbol)
                .collect();
            let mut probes = vec!["0d old"];
            if w == 80 {
                probes.push("  3 new   ");
                probes.push("next  Thu 01 Jan earnings");
            } else {
                probes.push("3 new items since your last visit");
                probes.push("newest   Apple announces new chip");
                probes.push("Thu 01 Jan  earnings · Q4 results");
            }
            for probe in probes {
                assert!(text.contains(probe), "missing {probe} at {w}x{h}");
            }
        }
    }
}
