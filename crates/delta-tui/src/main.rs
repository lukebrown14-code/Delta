//! Delta TUI binary: the real screens over the terminal, desk data from
//! `config.toml` + the configured DB, background quote/ingest workers over
//! the action bus. The app library lives in `delta_tui` (see `lib.rs`); the
//! app struct and event loop live in `app.rs`.

mod app;

use std::io::Stdout;

use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

fn main() -> std::io::Result<()> {
    // R4 benchmark entry: `delta --version` never touches the terminal.
    if std::env::args().any(|a| a == "--version" || a == "-V") {
        println!("delta {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    let mut terminal = setup()?;
    let runtime = tokio::runtime::Runtime::new()?;
    let res = runtime.block_on(app::run(&mut terminal));
    teardown(&mut terminal)?;
    res
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
