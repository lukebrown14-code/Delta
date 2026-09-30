//! Delta TUI binary: the real screens over the terminal, offline desk data.
//! The app library lives in `delta_tui` (see `lib.rs`).

use std::io::Stdout;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::{Frame, Terminal};

use delta_tui::desk::Desk;
use delta_tui::screen::Screen;
use delta_tui::screens::{
    draw_ask, draw_ask_narrow, draw_ask_wide, draw_decisions, draw_decisions_narrow,
    draw_decisions_wide, draw_glossary_overlay, draw_home, draw_home_narrow, draw_home_wide,
    draw_research, draw_research_narrow, draw_research_wide, draw_settings, draw_settings_narrow,
    draw_settings_wide, draw_theses, draw_theses_narrow, draw_theses_wide, draw_watchlist,
    draw_watchlist_narrow, draw_watchlist_wide,
};
use delta_tui::{is_quit_key, Action, Component};

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

/// The app: desk data + active pane, painting through the golden `Screen`
/// model so the live frame is cell-for-cell what the goldens capture.
struct App {
    desk: Desk,
    tab: Tab,
    glossary: bool,
    quit: bool,
}

impl App {
    fn paint(&self, screen: &mut Screen) {
        let w = screen.w;
        if w >= 160 {
            match self.tab {
                Tab::Home => draw_home_wide(screen, &self.desk.home_state()),
                Tab::Watchlist => draw_watchlist_wide(screen, &self.desk.watch_state()),
                Tab::Research => draw_research_wide(screen),
                Tab::Theses => draw_theses_wide(screen),
                Tab::Ask => draw_ask_wide(screen),
                Tab::Decisions => draw_decisions_wide(screen),
                Tab::Settings => draw_settings_wide(screen),
            }
        } else if w < delta_tui::NARROW_WIDTH as usize {
            match self.tab {
                Tab::Home => draw_home_narrow(screen, &self.desk.home_state()),
                Tab::Watchlist => draw_watchlist_narrow(screen, &self.desk.watch_state()),
                Tab::Research => draw_research_narrow(screen),
                Tab::Theses => draw_theses_narrow(screen),
                Tab::Ask => draw_ask_narrow(screen),
                Tab::Decisions => draw_decisions_narrow(screen),
                Tab::Settings => draw_settings_narrow(screen),
            }
        } else {
            match self.tab {
                Tab::Home => draw_home(screen, &self.desk.home_state()),
                Tab::Watchlist => draw_watchlist(screen, &self.desk.watch_state()),
                Tab::Research => draw_research(screen),
                Tab::Theses => draw_theses(screen),
                Tab::Ask => draw_ask(screen),
                Tab::Decisions => draw_decisions(screen),
                Tab::Settings => draw_settings(screen),
            }
        }
        if self.glossary && self.tab == Tab::Watchlist {
            draw_glossary_overlay(screen, &self.desk.watch_state());
        }
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
                self.desk.cycle_range(-1)
            }
            KeyCode::Char('l') | KeyCode::Right if self.tab == Tab::Watchlist => {
                self.desk.cycle_range(1)
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, action: Action) {
        if action == Action::Quit {
            self.quit = true;
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
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
    let res = run(&mut terminal);
    teardown(&mut terminal)?;
    res
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    let mut app = App {
        desk: Desk::offline(),
        tab: Tab::Watchlist,
        glossary: false,
        quit: false,
    };
    loop {
        terminal.draw(|frame| app.draw(frame, frame.area()))?;
        if !crossterm::event::poll(Duration::from_millis(100))? {
            continue;
        }
        if let Event::Key(key) = crossterm::event::read()? {
            if let Some(action) = app.handle_key(key) {
                app.update(action);
            }
        }
        if app.quit {
            return Ok(());
        }
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
    use ratatui::backend::TestBackend;

    fn app() -> App {
        App {
            desk: Desk::offline(),
            tab: Tab::Watchlist,
            glossary: false,
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
}
