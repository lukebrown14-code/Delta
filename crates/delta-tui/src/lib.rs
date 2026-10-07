//! Delta TUI: ratatui component-based terminal app (R0 skeleton + R1d
//! widgets + R3.1a TUI foundations: Screen components, breakpoints, shell
//! bindings, palette/help/Go modals, inputs, forms, markdown, light theme).

pub mod axes;
pub mod braille;
pub mod chart;
pub mod components;
pub mod desk;
pub mod dialog;
pub mod form;
pub mod input;
pub mod keymap;
pub mod markdown;
pub mod screen;
pub mod screens;
pub mod table;
pub mod theme;
pub mod workers;
pub mod wrap;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use ratatui::Frame;

use std::collections::BTreeMap;

/// Shared breakpoint carried over from `delta/tui/shell.py` (`NARROW_WIDTH`).
pub const NARROW_WIDTH: u16 = 100;
/// The wide breakpoint the shell painters use (>= 160 shows the wide
/// layouts; the exported goldens pin this).
pub const WIDE_WIDTH: u16 = 160;

/// The shell breakpoints (`shell.py::NARROW_WIDTH` plus the wide layout the
/// goldens capture): screens style against these, never their own width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Breakpoint {
    /// `width < NARROW_WIDTH`: panes stack, briefs replace lines.
    Narrow,
    /// `NARROW_WIDTH..WIDE_WIDTH`: the canonical 120x40 layout.
    Normal,
    /// `width >= WIDE_WIDTH`: panes grow, labels return to the status bar.
    Wide,
}

impl Breakpoint {
    pub fn from_width(width: u16) -> Self {
        if width < NARROW_WIDTH {
            Breakpoint::Narrow
        } else if width >= WIDE_WIDTH {
            Breakpoint::Wide
        } else {
            Breakpoint::Normal
        }
    }
}

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
    /// Navigate to a screen by registry name (the Go picker and palette).
    GotoScreen(String),
    /// Show the help modal (`?`).
    ShowHelp,
    /// Toggle light/dark (`f2`).
    ToggleTheme,
    /// Open the model picker (`m`; the picker screen lands with R3.2).
    ShowModelPicker,
    /// Open the provider picker (`p`; the picker screen lands with R3.2).
    ShowProviderPicker,
    /// A form submitted successfully (carries the form title).
    FormSubmitted(String),
    /// Live quote prices keyed by instrument id (from the quotes worker).
    Quotes(BTreeMap<String, f64>),
    /// Inspector metric rows (label, formatted value) for one instrument.
    Metrics {
        instrument: String,
        rows: Vec<(String, String)>,
    },
    /// Ingest finished; carries per-source row counts.
    Ingested(BTreeMap<String, usize>),
    /// Refreshed Home overview values (headline, pulse, upcoming, health).
    HomeRefresh(crate::screens::HomeFeed),
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

/// The screen-level component the R3.1a framework builds on
/// (`docs/rewrite/tasks/r3-tui-foundations.md` item 1): a named screen with
/// its own state, painted data-driven into a rect, keys first answering
/// `Option<Action>` up to the app.
///
/// The golden `crate::screen::Screen` cell grid stays the painter's model:
/// screens paint into a `Screen` and blit (see `screen::blit`), which keeps
/// every frame cell-for-cell what the goldens capture.
pub trait Screen {
    /// The registry name (`home`, `targets`, ...); drives Go, the palette
    /// and the status-bar highlight.
    fn name(&self) -> &'static str;

    /// Handle a key before the app's own bindings; `None` falls through.
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let _ = key;
        None
    }

    /// Apply a bus action.
    fn update(&mut self, action: Action) {
        let _ = action;
    }

    /// Paint the screen into `area`.
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
