//! The app: desk data + active pane, painting through the golden `Screen`
//! model so the live frame is cell-for-cell what the goldens capture. Owns
//! the event loop, worker startup, the Python-matching bindings, the shell
//! modals (Go, help, palette) and the theme switch.

use std::io::Stdout;
use std::path::Path;
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use delta_tui::components::CommandPalette;
use delta_tui::desk::Desk;
use delta_tui::dialog::{GoPicker, HelpDialog};
use delta_tui::screen::{color, Screen, Style};
use delta_tui::screens::{
    draw_ask, draw_ask_narrow, draw_ask_wide, draw_decisions, draw_decisions_narrow,
    draw_decisions_wide, draw_glossary_overlay, draw_home, draw_home_narrow, draw_home_wide,
    draw_research, draw_research_narrow, draw_research_wide, draw_settings, draw_settings_narrow,
    draw_settings_wide, draw_theses, draw_theses_narrow, draw_theses_wide, draw_watchlist,
    draw_watchlist_narrow, draw_watchlist_wide,
};
use delta_tui::theme::Palette;
use delta_tui::{is_quit_key, workers, Action, Breakpoint, Component};

/// The shell modals the app can float over a pane.
enum Overlay {
    /// `g`: the Go picker (`shell.py::GoPicker`).
    Go,
    /// `?`: the help modal (`screens/help.py::HelpScreen`).
    Help(HelpDialog),
    /// ctrl+k / ctrl+p: the command palette (`app.py::DeltaCommands`).
    Palette(CommandPalette),
    AddTarget(String),
}

/// The seven panes: 1-6 plus `c`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Tab {
    Home,
    Watchlist,
    Research,
    Theses,
    Ask,
    Decisions,
    Settings,
}

impl Tab {
    fn from_screen_name(name: &str) -> Option<Tab> {
        match name {
            "home" => Some(Tab::Home),
            "targets" => Some(Tab::Watchlist),
            "data" => Some(Tab::Research),
            "theses" => Some(Tab::Theses),
            "chat" => Some(Tab::Ask),
            "decisions" => Some(Tab::Decisions),
            "config" => Some(Tab::Settings),
            _ => None,
        }
    }
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
pub(crate) struct App {
    desk: Desk,
    tab: Tab,
    glossary: bool,
    status: String,
    frame_stats: FrameStats,
    quit: bool,
    /// The active theme (`f2` toggles; Python boots `delta-dark`).
    palette: Palette,
    /// The floating shell modal, if any.
    overlay: Option<Overlay>,
}

impl App {
    fn paint(&mut self, screen: &mut Screen) {
        let w = screen.w;
        // The shell breakpoints (`shell.py::NARROW_WIDTH` plus the wide
        // layout the goldens capture).
        let wide = Breakpoint::from_width(w as u16) == Breakpoint::Wide;
        let narrow = Breakpoint::from_width(w as u16) == Breakpoint::Narrow;
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
        match &mut self.overlay {
            Some(Overlay::Go) => GoPicker::draw_screen(screen),
            Some(Overlay::Help(help)) => help.draw_screen(screen),
            Some(Overlay::Palette(palette)) => palette.draw_screen(screen),
            Some(Overlay::AddTarget(query)) => {
                let left = screen.w.saturating_sub(62) / 2;
                let top = screen.h.saturating_sub(12) / 2;
                screen.pane(
                    left,
                    top,
                    left + 61,
                    top + 11,
                    true,
                    &[("add to watchlist", Style::fg(color::BLUE).bold())],
                    &[],
                );
                screen.text(left + 3, top + 2, "Instrument", Style::fg(color::MUTED));
                screen.text(
                    left + 3,
                    top + 3,
                    &format!("> {query}_"),
                    Style::fg(color::FG),
                );
                screen.text(
                    left + 3,
                    top + 6,
                    "Enter US:AAPL or ASX:BHP",
                    Style::fg(color::MUTED),
                );
            }
            None => {}
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

impl App {
    /// Persist a watched instrument using the config service, then reload the
    /// desk from the resulting config and database.
    fn add_instrument_at(&mut self, config: &Path, input: &str) -> Result<(), String> {
        let input = input.trim().to_uppercase();
        let (market, symbol) = input.split_once(':').unwrap_or(("US", input.as_str()));
        if symbol.is_empty()
            || !symbol
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ".-^".contains(ch))
        {
            return Err("enter a market and instrument, for example US:AAPL".into());
        }
        delta_services::config_ops::add_target(
            config,
            symbol,
            "company",
            &market.to_lowercase(),
            &[symbol.to_string()],
            &[],
            "",
            None,
            "equity",
        )
        .map_err(|error| error.to_string())?;
        self.desk.reload_targets_from(config)?;
        self.desk.selected = self
            .desk
            .instruments
            .iter()
            .position(|row| row.instrument.symbol == symbol)
            .unwrap_or(0);
        self.status = format!("{market}:{symbol} added");
        Ok(())
    }

    fn remove_instrument_at(&mut self, config: &Path) -> Result<(), String> {
        let current = self
            .desk
            .current()
            .ok_or_else(|| "nothing selected".to_string())?;
        let name = current
            .instrument
            .watchlists
            .first()
            .ok_or_else(|| "target unavailable".to_string())?
            .clone();
        delta_services::config_ops::remove_target(config, &name)
            .map_err(|error| error.to_string())?;
        self.desk.reload_targets_from(config)?;
        self.status = format!("{name} removed");
        Ok(())
    }

    /// The shell keys, in `DeltaApp.BINDINGS` order (plus the esc close and
    /// the palette keys Textual/the plan install). Per-screen keys (the
    /// watchlist's `r`/`R` range, `enter` inspect, ...) belong to their
    /// screens in R3.2.
    fn handle_shell_key(&mut self, key: KeyEvent) -> Option<Action> {
        if is_quit_key(key) {
            return Some(Action::Quit);
        }
        match key.code {
            KeyCode::Esc => {
                self.glossary = false;
                return Some(Action::Noop);
            }
            KeyCode::Char('1') => self.tab = Tab::Home,
            KeyCode::Char('2') => self.tab = Tab::Watchlist,
            KeyCode::Char('3') => self.tab = Tab::Research,
            KeyCode::Char('4') => self.tab = Tab::Theses,
            KeyCode::Char('5') => self.tab = Tab::Ask,
            KeyCode::Char('6') => self.tab = Tab::Decisions,
            KeyCode::Char('c') => self.tab = Tab::Settings,
            KeyCode::Char('h') => self.tab = Tab::Home,
            KeyCode::Char('m') => return Some(Action::ShowModelPicker),
            // Textual's palette key (`COMMAND_PALETTE_BINDING`) and the
            // plan's ctrl+k alias open the same palette.
            KeyCode::Char('p')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.overlay = Some(Overlay::Palette(CommandPalette::new()));
                return Some(Action::Noop);
            }
            KeyCode::Char('p') => return Some(Action::ShowProviderPicker),
            KeyCode::Char('g') => {
                self.overlay = Some(Overlay::Go);
                return Some(Action::Noop);
            }
            KeyCode::Char('?') => {
                self.overlay = match self.overlay.take() {
                    // `?` on the help toggles it off (action_show_help pops).
                    Some(Overlay::Help(_)) => None,
                    _ => Some(Overlay::Help(HelpDialog::default())),
                };
                return Some(Action::Noop);
            }
            KeyCode::Char('i') if self.tab == Tab::Watchlist => {
                // The watchlist's glossary (`i` metric_help) until its
                // screen owns its keys in R3.2.
                self.glossary = !self.glossary;
                return Some(Action::Noop);
            }
            KeyCode::Char('U') => return Some(Action::Gather),
            KeyCode::F(2) => return Some(Action::ToggleTheme),
            KeyCode::Char('k')
                if key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL) =>
            {
                self.overlay = Some(Overlay::Palette(CommandPalette::new()));
                return Some(Action::Noop);
            }
            _ => {}
        }
        None
    }
}

impl Component for App {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        // Modals take the key first (a Textual modal stops propagation).
        if let Some(Overlay::AddTarget(query)) = &mut self.overlay {
            match key.code {
                KeyCode::Esc => self.overlay = None,
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Enter => {
                    let input = query.clone();
                    match self.add_instrument_at(Path::new("config.toml"), &input) {
                        Ok(()) => self.overlay = None,
                        Err(error) => self.status = error,
                    }
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    query.push(ch)
                }
                _ => {}
            }
            return Some(Action::Noop);
        }
        if let Some(overlay) = &mut self.overlay {
            let action = match overlay {
                Overlay::Go => delta_tui::dialog::go_picker_key(key),
                Overlay::Help(help) => help.handle_key(key),
                Overlay::Palette(palette) => palette.handle_key(key),
                Overlay::AddTarget(_) => unreachable!(),
            };
            let action = action.unwrap_or(Action::Noop);
            match action {
                Action::CloseDialog => self.overlay = None,
                Action::Quit => return Some(Action::Quit),
                other => self.update(other),
            }
            return None;
        }
        if self.tab == Tab::Watchlist {
            match key.code {
                KeyCode::Char('a') => {
                    self.overlay = Some(Overlay::AddTarget(String::new()));
                    return Some(Action::Noop);
                }
                KeyCode::Char('d') => {
                    if let Err(error) = self.remove_instrument_at(Path::new("config.toml")) {
                        self.status = error;
                    }
                    return Some(Action::Noop);
                }
                KeyCode::Char(']') => {
                    self.desk.move_scrub(1);
                    return Some(Action::Noop);
                }
                KeyCode::Char('[') => {
                    self.desk.move_scrub(-1);
                    return Some(Action::Noop);
                }
                KeyCode::Char('r') => {
                    self.desk.cycle_range(1);
                    return Some(Action::Noop);
                }
                KeyCode::Char('R') => {
                    self.desk.cycle_range(-1);
                    return Some(Action::Noop);
                }
                KeyCode::Down => {
                    self.desk.cycle_instrument(1);
                    return Some(Action::Noop);
                }
                KeyCode::Up => {
                    self.desk.cycle_instrument(-1);
                    return Some(Action::Noop);
                }
                _ => {}
            }
        }
        if let Some(action) = self.handle_shell_key(key) {
            return Some(action);
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
            Action::AssetMetrics { range, data } => {
                if let Some(error) = &data.error {
                    self.status = format!("metrics unavailable: {error}");
                } else {
                    self.desk
                        .asset_metrics
                        .insert((data.instrument_id.clone(), range), *data);
                }
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
            Action::ToggleTheme => {
                // `action_toggle_theme`: swap the palette and notify.
                self.palette = match self.palette {
                    Palette::Dark => Palette::Light,
                    Palette::Light => Palette::Dark,
                };
                self.status = format!("Theme: {}", self.palette.name());
            }
            Action::GotoScreen(name) => {
                if let Some(tab) = Tab::from_screen_name(&name) {
                    self.tab = tab;
                    self.overlay = None;
                    self.glossary = false;
                }
            }
            Action::ShowHelp => self.overlay = Some(Overlay::Help(HelpDialog::default())),
            Action::ShowModelPicker => {
                // The picker screen lands with R3.2 (settings stream); the
                // binding is live now so the keymap cannot drift.
                self.status = "model picker: lands with R3.2 settings".to_string();
            }
            Action::ShowProviderPicker => {
                self.status = "provider picker: lands with R3.2 settings".to_string();
            }
            Action::FormSubmitted(title) => {
                self.overlay = None;
                self.status = format!("{title} saved");
            }
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
        let mut screen = Screen::themed(self.palette, area.width as usize, area.height as usize);
        self.paint(&mut screen);
        delta_tui::screen::blit(frame, &screen, area);
    }
}

fn start_workers(bus: mpsc::UnboundedSender<Action>, desk: &Desk) -> workers::Workers {
    let universe: Vec<delta_core::models::Instrument> = desk
        .instruments
        .iter()
        .map(|row| row.instrument.clone())
        .collect();
    let db_path = match &desk.source {
        delta_tui::desk::Source::Real(db) => Some(db.clone()),
        delta_tui::desk::Source::Offline => None,
    };
    let quotes_enabled = matches!(&desk.source, delta_tui::desk::Source::Real(_));
    workers::spawn(bus, universe, db_path, quotes_enabled)
}

pub(crate) async fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    let (bus_tx, mut bus_rx) = mpsc::unbounded_channel::<Action>();
    let desk = Desk::open();
    let mut active_workers = start_workers(bus_tx.clone(), &desk);

    let mut app = App {
        desk,
        tab: Tab::Watchlist,
        glossary: false,
        status: String::new(),
        frame_stats: FrameStats::default(),
        quit: false,
        palette: Palette::Dark,
        overlay: None,
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
                        let previous_ids: Vec<_> = app.desk.instruments.iter().map(|row| row.instrument.id.clone()).collect();
                        let previous_range = app.desk.range();
                        let previous_selected = app.desk.current().map(|row| row.instrument.id.clone());
                        if let Some(action) = app.handle_key(key) {
                            if action == Action::Gather {
                                let _ = active_workers.gather_tx.send(());
                            } else {
                                app.update(action);
                            }
                        }
                        let current_ids: Vec<_> = app.desk.instruments.iter().map(|row| row.instrument.id.clone()).collect();
                        if current_ids != previous_ids {
                            active_workers = start_workers(bus_tx.clone(), &app.desk);
                        }
                        let selected = app.desk.current();
                        let selection_changed = selected.map(|row| &row.instrument.id) != previous_selected.as_ref();
                        if (app.desk.range() != previous_range || selection_changed || key.code == KeyCode::Enter)
                            && app.tab == Tab::Watchlist && app.overlay.is_none() {
                            if let Some(row) = selected {
                                let _ = active_workers.metrics_tx.send((row.instrument.clone(), app.desk.range().to_string()));
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
            palette: Palette::Dark,
            overlay: None,
        }
    }

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn bindings_match_python_delta_app() {
        let mut a = app();
        for (key_code, tab) in [
            (KeyCode::Char('1'), Tab::Home),
            (KeyCode::Char('2'), Tab::Watchlist),
            (KeyCode::Char('3'), Tab::Research),
            (KeyCode::Char('4'), Tab::Theses),
            (KeyCode::Char('5'), Tab::Ask),
            (KeyCode::Char('6'), Tab::Decisions),
        ] {
            a.handle_key(key(key_code));
            assert_eq!(a.tab, tab);
        }
        a.handle_key(key(KeyCode::Char('c')));
        assert_eq!(a.tab, Tab::Settings);
        // `h` jumps Home from anywhere (Python binds it app-wide).
        a.handle_key(key(KeyCode::Char('3')));
        a.handle_key(key(KeyCode::Char('h')));
        assert_eq!(a.tab, Tab::Home);
    }

    #[test]
    fn g_opens_go_picker_and_esc_closes() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('g')));
        assert!(matches!(a.overlay, Some(Overlay::Go)));
        a.handle_key(key(KeyCode::Esc));
        assert!(a.overlay.is_none());
        // `4` on the Go picker jumps to Theses and closes it.
        a.handle_key(key(KeyCode::Char('g')));
        a.handle_key(key(KeyCode::Char('4')));
        assert_eq!(a.tab, Tab::Theses);
        assert!(a.overlay.is_none());
    }

    #[test]
    fn question_mark_toggles_the_help_modal() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('?')));
        assert!(matches!(a.overlay, Some(Overlay::Help(_))));
        a.handle_key(key(KeyCode::Char('?')));
        assert!(a.overlay.is_none(), "? on help pops it (action_show_help)");
        a.handle_key(key(KeyCode::Char('?')));
        a.handle_key(key(KeyCode::Esc));
        assert!(a.overlay.is_none());
    }

    #[test]
    fn palette_opens_on_ctrl_k_and_ctrl_p() {
        let mut a = app();
        a.handle_key(ctrl(KeyCode::Char('k')));
        assert!(matches!(a.overlay, Some(Overlay::Palette(_))));
        a.handle_key(key(KeyCode::Esc));
        a.handle_key(ctrl(KeyCode::Char('p')));
        assert!(matches!(a.overlay, Some(Overlay::Palette(_))));
        // Typing filters, esc closes.
        a.handle_key(key(KeyCode::Char('g')));
        a.handle_key(key(KeyCode::Esc));
        assert!(a.overlay.is_none());
    }

    #[test]
    fn model_and_provider_pickers_are_bound() {
        let mut a = app();
        assert_eq!(
            a.handle_key(key(KeyCode::Char('m'))),
            Some(Action::ShowModelPicker)
        );
        a.update(Action::ShowModelPicker);
        assert!(!a.status.is_empty());
        assert_eq!(
            a.handle_key(key(KeyCode::Char('p'))),
            Some(Action::ShowProviderPicker)
        );
        a.update(Action::ShowProviderPicker);
        assert!(!a.status.is_empty());
    }

    #[test]
    fn f2_toggles_the_theme_and_the_screen_remaps() {
        let mut a = app();
        a.tab = Tab::Home;
        assert_eq!(a.handle_key(key(KeyCode::F(2))), Some(Action::ToggleTheme));
        a.update(Action::ToggleTheme);
        assert_eq!(a.palette, Palette::Light);
        assert_eq!(a.status, "Theme: delta-light");
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|f| a.draw(f, f.area())).unwrap();
        // The light background shows on the terminal buffer's corner cell.
        let bg = terminal.backend().buffer()[(0, 0)].bg;
        assert_eq!(bg, ratatui::style::Color::Rgb(0xf4, 0xf4, 0xef));
        a.update(Action::ToggleTheme);
        assert_eq!(a.palette, Palette::Dark);
    }

    #[test]
    fn glossary_toggles_on_watchlist_with_i_and_esc_closes() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('i')));
        assert!(a.glossary);
        a.handle_key(key(KeyCode::Esc));
        assert!(!a.glossary);
        // `i` on another pane does not open it.
        a.handle_key(key(KeyCode::Char('3')));
        a.handle_key(key(KeyCode::Char('i')));
        assert!(!a.glossary);
    }

    #[test]
    fn quit_keys_produce_quit_action() {
        let mut a = app();
        assert_eq!(a.handle_key(key(KeyCode::Char('q'))), Some(Action::Quit));
        assert_eq!(a.handle_key(ctrl(KeyCode::Char('c'))), Some(Action::Quit));
    }

    #[test]
    fn gather_is_requested_and_statuses_update() {
        let mut a = app();
        assert_eq!(a.handle_key(key(KeyCode::Char('U'))), Some(Action::Gather));
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
    fn add_dialog_accepts_instrument_text_and_esc_cancels() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('a')));
        for ch in "ASX:BHP".chars() {
            a.handle_key(key(KeyCode::Char(ch)));
        }
        assert!(matches!(&a.overlay, Some(Overlay::AddTarget(query)) if query == "ASX:BHP"));
        a.handle_key(key(KeyCode::Esc));
        assert!(a.overlay.is_none());
    }

    #[test]
    fn watchlist_add_and_remove_persist_through_services() {
        let dir = std::env::temp_dir().join(format!(
            "delta-watchlist-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let config = dir.join("config.toml");
        let db_path = dir.join("delta.db");
        delta_core::db::Db::open(&db_path).unwrap();
        std::fs::write(
            &config,
            format!("db_path = {:?}\n", db_path.display().to_string()),
        )
        .unwrap();
        let mut a = app();
        a.add_instrument_at(&config, "ASX:BHP").unwrap();
        assert!(delta_services::config_ops::target_specs(&config)
            .unwrap()
            .contains_key("BHP"));
        assert_eq!(a.desk.watch_state().entries[0].symbol, "BHP");
        a.remove_instrument_at(&config).unwrap();
        assert!(delta_services::config_ops::target_specs(&config)
            .unwrap()
            .is_empty());
        assert!(a.desk.watch_state().entries.is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn empty_watchlist_renders_without_a_selected_instrument() {
        let mut a = app();
        a.desk.instruments.clear();
        for &(w, h) in &[(80, 24), (120, 40), (200, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| a.draw(frame, frame.area())).unwrap();
        }
        assert!(a.desk.watch_state().metric.is_none());
    }

    #[test]
    fn watchlist_scrub_moves_across_the_loaded_series() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char(']')));
        let state = a.desk.watch_state();
        let count = state.metric.unwrap().series.len();
        assert_eq!(state.scrub, Some(count - 1));
        a.handle_key(key(KeyCode::Char('[')));
        assert_eq!(a.desk.watch_state().scrub, Some(count - 2));
    }

    #[test]
    fn scrubbed_watchlist_paints_a_date_value_and_cursor() {
        let mut a = app();
        a.handle_key(key(KeyCode::Char(']')));
        let state = a.desk.watch_state();
        let metric = state.metric.unwrap();
        let stamp = delta_tui::screens::friendly_date(metric.series_times.last().unwrap());
        let price = delta_tui::screens::grouped(*metric.series.last().unwrap());
        let mut screen = Screen::new(120, 40);
        a.paint(&mut screen);
        let cells = &screen.cells;
        let width = screen.w;
        let chart_rows: String = (4..18)
            .flat_map(|y| (50..116).map(move |x| cells[y * width + x].ch))
            .collect();
        assert!(chart_rows.contains("┊"));
        assert!(chart_rows.contains(&stamp));
        assert!(chart_rows.contains(&price));
    }

    #[test]
    fn bond_metrics_from_service_reach_the_inspector() {
        let mut a = app();
        a.desk.instruments[0].instrument.asset_class = delta_core::models::AssetClass::Bond;
        let inst = a.desk.instruments[0].instrument.clone();
        let metrics = delta_services::asset_metrics::normalize_asset_metrics(
            &inst,
            &serde_json::json!({"couponRate": {"raw": 0.05}}),
            vec![4.0, 4.25],
            vec!["2026-10-08T00:00:00Z".into(), "2026-10-09T00:00:00Z".into()],
        );
        a.update(Action::AssetMetrics {
            range: "1m".into(),
            data: Box::new(metrics),
        });
        let inspector = a.desk.watch_state().metric.unwrap();
        assert_eq!(inspector.asset_class, "bond");
        assert_eq!(inspector.change_label.as_deref(), Some("+25.0 bps"));
        assert!(inspector.values.iter().any(|(label, _)| label == "Coupon"));
    }

    #[test]
    fn watchlist_metrics_use_the_instrument_asset_class() {
        let mut a = app();
        a.desk.instruments[0].instrument.asset_class = delta_core::models::AssetClass::Bond;
        let metric = a.desk.watch_state().metric.unwrap();
        assert_eq!(metric.asset_class, "bond");
        assert!(metric.change_label.unwrap().ends_with("bps"));
        assert_eq!(metric.values[0].0, "Current yield");
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
    fn every_shell_modal_paints_at_all_sizes() {
        for &(w, h) in &[(80u16, 24u16), (120, 40), (200, 50)] {
            let mut a = app();
            a.tab = Tab::Home;
            a.overlay = Some(Overlay::Go);
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| a.draw(f, f.area())).unwrap();
            a.overlay = Some(Overlay::Help(HelpDialog::default()));
            terminal.draw(|f| a.draw(f, f.area())).unwrap();
            a.overlay = Some(Overlay::Palette(CommandPalette::new()));
            terminal.draw(|f| a.draw(f, f.area())).unwrap();
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
