//! Delta TUI skeleton: `Component`/`Action` template with a ratatui loop stub.
//!
//! Mirrors the Python TUI's shell structure: panes handle keys and return
//! [`Action`]s, the app updates panes with them, and the loop never blocks.

use std::io::Stdout;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use ratatui::{Frame, Terminal};

/// Shared breakpoint carried over from `delta/tui/shell.py` (`NARROW_WIDTH`).
pub const NARROW_WIDTH: u16 = 100;

/// A discrete app-level event flowing through the (future) action bus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Placeholder for future action variants.
    #[allow(dead_code)]
    Noop,
    /// Shut the app down.
    Quit,
}

/// Ratatui component template (see `docs/RUST_REWRITE_PLAN.md`).
pub trait Component {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action>;
    fn update(&mut self, action: Action);
    fn draw(&mut self, frame: &mut Frame, area: Rect);
}

/// A single TUI pane stub; will become one of the real screens/widgets.
pub struct Pane {
    #[allow(dead_code)]
    title: String,
}

impl Pane {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
        }
    }
}

impl Component for Pane {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Char('q') => Some(Action::Quit),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                Some(Action::Quit)
            }
            _ => None,
        }
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut Frame, area: Rect) {
        frame.render_widget(ratatui::widgets::Block::bordered(), area);
    }
}

fn main() -> std::io::Result<()> {
    let mut terminal = setup()?;
    let res = run(&mut terminal);
    teardown(&mut terminal)?;
    res
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    let mut pane = Pane::new("Delta");
    loop {
        terminal.draw(|frame| pane.draw(frame, frame.area()))?;
        if !crossterm::event::poll(Duration::from_millis(100))? {
            continue;
        }
        if let Event::Key(key) = crossterm::event::read()? {
            if let Some(Action::Quit) = pane.handle_key(key) {
                return Ok(());
            }
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

    #[test]
    fn quit_keys_produce_quit_action() {
        let mut pane = Pane::new("test");
        for key in [
            KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            assert_eq!(pane.handle_key(key), Some(Action::Quit));
        }
    }

    #[test]
    fn other_keys_do_nothing() {
        let mut pane = Pane::new("test");
        assert_eq!(pane.handle_key(KeyEvent::from(KeyCode::Enter)), None);
    }
}
