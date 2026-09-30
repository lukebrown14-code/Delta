//! Delta TUI: ratatui component-based terminal app (R0 skeleton + R1d widgets).

pub mod axes;
pub mod braille;
pub mod chart;
pub mod components;
pub mod desk;
pub mod dialog;
pub mod screen;
pub mod screens;
pub mod table;
pub mod theme;
pub mod workers;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::Frame;

use std::collections::BTreeMap;

/// Shared breakpoint carried over from `delta/tui/shell.py` (`NARROW_WIDTH`).
pub const NARROW_WIDTH: u16 = 100;

/// A discrete app-level event flowing through the action bus.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// No state change.
    Noop,
    /// Shut the app down.
    Quit,
    /// Open a modal dialog by name.
    OpenDialog(&'static str),
    /// Close the top modal.
    CloseDialog,
    /// Navigate to a screen by name.
    Goto(&'static str),
    /// Live quote prices keyed by instrument id (from the quotes worker).
    Quotes(BTreeMap<String, f64>),
    /// Ingest finished; carries per-source row counts.
    Ingested(BTreeMap<String, usize>),
    /// Request a gather run (UI -> ingest worker).
    Gather,
    /// One-line worker status for the status overlay.
    Status(String),
}

/// Ratatui component template (see `docs/RUST_REWRITE_PLAN.md`).
pub trait Component {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action>;
    fn update(&mut self, action: Action);
    fn draw(&mut self, frame: &mut Frame, area: Rect);
}

/// Global quit keys, shared by every pane (matches the Python bindings).
pub fn is_quit_key(key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Char('q') => true,
        KeyCode::Char('c') => key.modifiers.contains(KeyModifiers::CONTROL),
        _ => false,
    }
}
