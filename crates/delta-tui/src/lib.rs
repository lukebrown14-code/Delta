//! Delta TUI: ratatui component-based terminal app (R0 skeleton + R1d widgets).

pub mod ask_view;
pub mod axes;
pub mod braille;
pub mod chart;
pub mod components;
pub mod decisions_view;
pub mod desk;
pub mod dialog;
pub mod footer;
pub mod markdown;
pub mod metrics_view;
pub mod research;
pub mod screen;
pub mod screens;
pub mod settings_view;
pub mod table;
pub mod theme;
pub mod theses_view;
pub mod watchlist;
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
    Goto(String),
    /// Live quote prices keyed by instrument id (from the quotes worker).
    Quotes(BTreeMap<String, f64>),
    /// Inspector metric rows (label, formatted value) for one instrument.
    Metrics {
        instrument: String,
        rows: Vec<(String, String)>,
    },
    RefreshMetrics(String),
    AssetMetrics {
        range: String,
        metrics: delta_services::asset_metrics::AssetMetrics,
    },
    /// Ingest finished; carries per-source row counts.
    Ingested(BTreeMap<String, usize>),
    /// Complete gather finished: rows, extracted events, and classified news.
    Gathered {
        counts: BTreeMap<String, usize>,
        events: usize,
        sentiment: usize,
        warnings: Vec<String>,
    },
    /// Refreshed Home overview values (headline, pulse, upcoming, health).
    HomeRefresh(crate::screens::HomeFeed),
    FooterRefresh(crate::footer::FooterState),
    /// Request a gather run (UI -> ingest worker).
    Gather,
    GatherTargets(Vec<String>),
    CancelGather,
    CancelReport,
    GatherBusy(bool),
    ReportBusy(bool),
    /// Request a cited report for one instrument.
    GenerateReport(String),
    /// Generated report markdown to show in Research.
    ReportReady {
        target_id: String,
        markdown: String,
    },
    /// Submit one Ask prompt from the input field.
    AskQuestion(String),
    /// Verified answer from the grounded chat service.
    ChatReady(delta_services::ChatMessage),
    ChatFailed(String),
    CancelChat,
    AskBusy(bool),
    ChatFinished {
        generation: u64,
        result: Result<delta_services::ChatMessage, String>,
    },
    ThesisProposed(usize),
    ProposeThesis(String),
    SummarizeThesis(String),
    ThesisSummaryReady(String),
    ConnectProvider(delta_services::ProviderSetup),
    LoadModels,
    LoadDiagnostics,
    SettingsDiagnostics(Result<crate::settings_view::SettingsDiagnostics, String>),
    ModelsReady(Vec<String>),
    ProviderConnected {
        name: String,
        verified: bool,
    },
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
