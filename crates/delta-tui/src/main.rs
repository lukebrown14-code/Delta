//! Delta TUI binary: runs the component loop over the terminal.
//! The app library lives in `delta_tui` (see `lib.rs`).

use std::io::Stdout;
use std::time::Duration;

use crossterm::event::{Event, KeyEvent};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::{Frame, Terminal};

use delta_tui::components::WhichKey;
use delta_tui::dialog::ModalStack;
use delta_tui::{is_quit_key, Action, Component, NARROW_WIDTH};

/// The app shell: one pane over a which-key footer, with a modal stack.
struct App {
    footer: WhichKey,
    modals: ModalStack,
    quit: bool,
}

impl Component for App {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if self.modals.is_open() {
            return self.modals.handle_key(key);
        }
        if is_quit_key(key) {
            return Some(Action::Quit);
        }
        None
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.quit = true,
            Action::OpenDialog(name) => {
                self.modals
                    .push(delta_tui::dialog::Dialog::new(name, vec![]));
            }
            Action::CloseDialog => {
                self.modals.pop();
            }
            _ => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        let [content, footer] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
        // Placeholder pane shell until the R3 screens land; the footer and the
        // modal stack are the real R1d components.
        let narrow = area.width < NARROW_WIDTH;
        let title = if narrow { "delta (narrow)" } else { "delta" };
        frame.render_widget(
            ratatui::widgets::Paragraph::new(title).style(
                ratatui::style::Style::default().fg(delta_tui::theme::Theme::TEXT_PRIMARY.color()),
            ),
            content,
        );
        self.footer.draw(frame, footer);
        self.modals.draw(frame, area);
    }
}

fn main() -> std::io::Result<()> {
    let mut terminal = setup()?;
    let res = run(&mut terminal);
    teardown(&mut terminal)?;
    res
}

fn run(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> std::io::Result<()> {
    let mut app = App {
        footer: WhichKey {
            bindings: vec![("q".to_string(), "quit".to_string())],
        },
        modals: ModalStack::default(),
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

    #[test]
    fn quit_keys_produce_quit_action() {
        let mut app = App {
            footer: WhichKey { bindings: vec![] },
            modals: ModalStack::default(),
            quit: false,
        };
        for key in [
            KeyEvent::new(crossterm::event::KeyCode::Char('q'), KeyModifiers::NONE),
            KeyEvent::new(crossterm::event::KeyCode::Char('c'), KeyModifiers::CONTROL),
        ] {
            assert_eq!(app.handle_key(key), Some(Action::Quit));
        }
    }

    #[test]
    fn modal_intercepts_keys_before_the_shell() {
        let mut app = App {
            footer: WhichKey { bindings: vec![] },
            modals: ModalStack::default(),
            quit: false,
        };
        app.update(Action::OpenDialog("confirm"));
        // `q` inside a modal is swallowed by the dialog, not the shell.
        assert_eq!(
            app.handle_key(KeyEvent::new(
                crossterm::event::KeyCode::Char('q'),
                KeyModifiers::NONE
            )),
            Some(Action::Quit)
        );
        assert_eq!(
            app.handle_key(KeyEvent::from(crossterm::event::KeyCode::Esc)),
            Some(Action::CloseDialog)
        );
    }
}
