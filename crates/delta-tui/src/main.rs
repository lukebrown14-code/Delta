//! Delta TUI binary: the real screens over the terminal, desk data from
//! `config.toml` + the configured DB, background quote/ingest workers over
//! the action bus. The app library lives in `delta_tui` (see `lib.rs`).

use std::collections::BTreeMap;
use std::io::Stdout;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{execute, queue};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style as RStyle};
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use delta_services::{Decision, Thesis};
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

#[derive(Debug, Clone, Copy)]
enum FormKind {
    Thesis,
    ThesisEdit,
    Decision,
    DecisionEdit,
    Review,
    Evidence,
    Llm,
    Target,
    RemoveTarget,
    Source,
    Market,
    RemoveMarket,
    Provider,
}

struct Form {
    kind: FormKind,
    labels: Vec<&'static str>,
    answers: Vec<String>,
    field: usize,
    input: String,
    defaults: Vec<String>,
    citations: Vec<String>,
    locked_id: Option<String>,
}

#[derive(Default)]
struct JournalFilter {
    text: String,
    editing: bool,
}

impl Form {
    fn new(kind: FormKind) -> Self {
        let mut labels = match kind {
            FormKind::Thesis | FormKind::ThesisEdit => vec![
                "Claim",
                "Scope",
                "Assumptions (semicolon separated)",
                "Falsifiers (semicolon separated)",
                "Targets (comma separated ids)",
                "Time horizon",
            ],
            FormKind::Decision | FormKind::DecisionEdit => vec![
                "Instrument id",
                "Rationale",
                "Valuation context",
                "Time horizon",
                "Review date (YYYY-MM-DD)",
                "Invalidation criteria",
                "Thesis id (optional)",
            ],
            FormKind::Review => vec![
                "Review note",
                "Status (open, reviewed, retired; blank keeps current)",
            ],
            FormKind::Evidence => vec![
                "Evidence id",
                "Side (support, against, neutral)",
                "Why it matters",
            ],
            FormKind::Llm => vec!["Provider", "Default model id"],
            FormKind::Target => vec![
                "Name",
                "Kind (company, sector, industry, theme, market)",
                "Market",
                "Tickers (comma separated)",
                "Asset class",
                "Tags (comma separated)",
                "Notes",
            ],
            FormKind::RemoveTarget => vec!["Target name", "Type REMOVE to confirm"],
            FormKind::Source => vec![
                "SEC contact email",
                "Markets (comma separated; blank for all)",
            ],
            FormKind::Market => vec!["Market id", "Label", "Currency", "Yahoo suffix"],
            FormKind::RemoveMarket => vec!["Market id", "Type REMOVE to confirm"],
            FormKind::Provider => vec![
                "Provider",
                "Base URL (custom only)",
                "Key environment variable (custom only)",
                "API key (blank keeps saved key)",
            ],
        };
        if matches!(kind, FormKind::ThesisEdit) {
            labels.push("Status (active, paused, concluded)");
        }
        Self {
            kind,
            labels,
            answers: Vec::new(),
            field: 0,
            input: String::new(),
            defaults: Vec::new(),
            citations: Vec::new(),
            locked_id: None,
        }
    }

    fn with_defaults(mut self, defaults: Vec<String>) -> Self {
        self.input = defaults.first().cloned().unwrap_or_default();
        self.defaults = defaults;
        self
    }

    fn lock_id(mut self) -> Self {
        self.locked_id = self.defaults.first().cloned();
        self.answers = vec![self.locked_id.clone().unwrap_or_default()];
        self.field = 1;
        self.input = self.defaults.get(1).cloned().unwrap_or_default();
        self
    }

    fn retry(mut self) -> Self {
        self.defaults = std::mem::take(&mut self.answers);
        self.input = self.defaults.first().cloned().unwrap_or_default();
        self.field = 0;
        if self.locked_id.is_some() {
            self = self.lock_id();
        }
        self
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
struct App {
    desk: Desk,
    tab: Tab,
    glossary: bool,
    status: String,
    reports: BTreeMap<String, String>,
    research_scroll: usize,
    research: delta_tui::research::ResearchBrowser,
    watchlist: delta_tui::watchlist::WatchlistBrowser,
    metrics_scroll: usize,
    palette: Option<delta_tui::components::CommandPalette>,
    palette_model: bool,
    help_open: bool,
    footer: delta_tui::footer::FooterState,
    settings_state: delta_tui::settings_view::SettingsState,
    settings_diagnostics: Result<delta_tui::settings_view::SettingsDiagnostics, String>,
    settings_sources: Vec<delta_tui::settings_view::SettingsSource>,
    provider_connected: bool,
    viewport_width: usize,
    pending_action: Option<Action>,
    ask_input: String,
    ask_editing: bool,
    gather_busy: bool,
    report_busy: bool,
    ask_busy: bool,
    ask_generation: u64,
    ask_scroll: usize,
    ask_history: Vec<delta_services::ChatMessage>,
    ask: delta_tui::ask_view::AskState,
    db_path: Option<PathBuf>,
    config_path: PathBuf,
    configuration_changed: bool,
    theses: Vec<Thesis>,
    decisions: Vec<Decision>,
    decision_reviews: Vec<delta_services::DecisionReview>,
    thesis_selected: usize,
    decision_selected: usize,
    decision_detail_open: bool,
    decision_scroll: usize,
    journal_filter: JournalFilter,
    pending_delete: Option<String>,
    form: Option<Form>,
    thesis_links: Vec<delta_services::ThesisEvidence>,
    thesis_items: BTreeMap<String, delta_services::EvidenceItem>,
    thesis_health: BTreeMap<String, delta_services::HealthResult>,
    thesis_focus: delta_tui::theses_view::ThesisFocus,
    evidence_selected: usize,
    framing_scroll: usize,
    note_scroll: usize,
    thesis_summary: Option<String>,
    summary_scroll: usize,
    settings: Option<delta_core::config::AppConfig>,

    frame_stats: FrameStats,
    quit: bool,
}

impl App {
    fn paint(&mut self, screen: &mut Screen) {
        self.viewport_width = screen.w;
        if let delta_tui::desk::Source::Unavailable(reason) = &self.desk.source {
            if !matches!(
                self.tab,
                Tab::Settings | Tab::Theses | Tab::Decisions | Tab::Ask | Tab::Watchlist
            ) {
                screen.text(2, 1, "DELTA", Style::fg(color::BLUE).bold());
                screen.text(
                    2,
                    3,
                    "Watch data unavailable",
                    Style::fg(color::AMBER).bold(),
                );
                screen.text(2, 5, reason, Style::fg(color::FG));
                screen.text(
                    2,
                    7,
                    "Press a to add a watch target, or c to open Settings.",
                    Style::fg(color::MUTED),
                );
                if self.form.is_some() {
                    self.paint_form(screen);
                }
                if !matches!(self.desk.source, delta_tui::desk::Source::Offline) {
                    self.footer.paint(
                        screen,
                        match self.tab {
                            Tab::Home => "Home",
                            Tab::Watchlist => "Watchlist",
                            Tab::Research => "Research",
                            Tab::Theses => "Theses",
                            Tab::Ask => "Ask",
                            Tab::Decisions => "Decisions",
                            Tab::Settings => "Settings",
                        },
                        &self.status,
                    );
                } else {
                    self.paint_status_overlay(screen);
                }
                return;
            }
        }
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
        if self.tab == Tab::Research && matches!(self.desk.source, delta_tui::desk::Source::Real(_))
        {
            self.paint_research_live(screen);
        }
        if self.tab == Tab::Ask {
            self.paint_ask_live(screen);
        }
        if self.tab == Tab::Theses {
            self.paint_theses_live(screen);
        }
        if self.tab == Tab::Decisions {
            self.paint_decisions_live(screen);
        }
        if self.tab == Tab::Settings {
            self.paint_settings_live(screen);
        }
        if !matches!(self.desk.source, delta_tui::desk::Source::Offline) {
            if self.tab == Tab::Watchlist {
                self.paint_watchlist_rows(screen);
            }
            if self.tab == Tab::Home {
                self.paint_home_live(screen);
            }
        }
        if self.form.is_some() {
            self.paint_form(screen);
        }
        if self.tab == Tab::Theses {
            self.paint_thesis_summary(screen);
        }
        if self.glossary && self.tab == Tab::Watchlist {
            draw_glossary_overlay(screen, &self.desk.watch_state());
        }
        if self.form.is_none()
            && self.tab == Tab::Home
            && self
                .desk
                .instruments
                .get(self.desk.selected)
                .is_some_and(|item| item.bars.is_empty())
        {
            screen.fill(0, 0, screen.w, screen.h.saturating_sub(1), Style::DEFAULT);
            screen.text(2, 1, "DELTA", Style::fg(color::BLUE).bold());
            screen.text(
                2,
                3,
                &format!(
                    "No price history for {}",
                    self.desk.current().instrument.symbol
                ),
                Style::fg(color::AMBER),
            );
            screen.text(
                2,
                5,
                "Press U to gather data, or , / . to select another instrument.",
                Style::fg(color::MUTED),
            );
        }
        if !matches!(self.desk.source, delta_tui::desk::Source::Offline) {
            self.footer.paint(
                screen,
                match self.tab {
                    Tab::Home => "Home",
                    Tab::Watchlist => "Watchlist",
                    Tab::Research => "Research",
                    Tab::Theses => "Theses",
                    Tab::Ask => "Ask",
                    Tab::Decisions => "Decisions",
                    Tab::Settings => "Settings",
                },
                &self.status,
            );
        } else {
            self.paint_status_overlay(screen);
        }
    }

    /// A right-aligned live badge for worker progress and quote state.
    fn paint_status_overlay(&mut self, screen: &mut Screen) {
        let y = screen.h.saturating_sub(1);
        let mut label = String::new();
        if !self.desk.live.is_empty() {
            label.push_str(&format!("● live {}  ", self.desk.live.len()));
        }
        if !self.status.is_empty() {
            label.push_str(&self.status);
        }
        if label.is_empty() {
            return;
        }
        let style = Style::fg(color::GREEN);
        let start = screen.w.saturating_sub(label.chars().count());
        screen.text(start, y, &label, style);
    }

    fn inspect_evidence(&mut self, id: &str) {
        let Some(path) = &self.db_path else {
            self.status = "No configured database".into();
            return;
        };
        let result = delta_core::db::Db::open(path)
            .map_err(|error| error.to_string())
            .and_then(|db| {
                delta_services::evidence_by_ids(&db, &[id.to_string()])
                    .map_err(|error| error.to_string())
            });
        match result {
            Ok(items) if !items.is_empty() => {
                let item = &items[0];
                if let Some(index) = self
                    .desk
                    .instruments
                    .iter()
                    .position(|desk| item.target_ids.contains(&desk.instrument.id))
                {
                    self.desk.selected = index;
                }
                self.tab = Tab::Research;
                self.reload_research();
                self.research.selected = self
                    .research
                    .items
                    .iter()
                    .position(|item| item.id == id)
                    .unwrap_or_else(|| {
                        self.research.items.push(items[0].clone());
                        self.research.items.len() - 1
                    });
                self.research.company_open = false;
                self.research.evidence_open = true;
                self.research.detail_open = true;
                self.research.preview_scroll = 0;
            }
            Ok(_) => self.status = format!("Evidence unavailable: {id}"),
            Err(error) => self.status = format!("Evidence: {error}"),
        }
    }

    fn paint_research_live(&self, screen: &mut Screen) {
        let bottom = screen.h.saturating_sub(3);
        if screen.w < delta_tui::NARROW_WIDTH as usize || self.research.zoomed {
            if self.research.company_open {
                self.paint_research_companies(screen);
            } else if self.research.evidence_open {
                let target = self
                    .desk
                    .instruments
                    .get(self.desk.selected)
                    .map(|item| item.instrument.symbol.as_str())
                    .unwrap_or_default();
                self.research.paint(screen, target);
            } else {
                self.paint_report_live(screen);
            }
            return;
        }
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        let report_right = screen.w - 42;
        let evidence_left = screen.w - 41;
        let title = Style::fg(color::BLUE).bold();
        screen.pane(
            1,
            0,
            36,
            bottom,
            self.research.company_open,
            &[("t company", title)],
            &[],
        );
        screen.pane(
            37,
            0,
            report_right,
            bottom,
            !self.research.company_open && !self.research.evidence_open,
            &[("r report", title)],
            &[],
        );
        screen.pane(
            evidence_left,
            0,
            screen.w - 2,
            bottom,
            self.research.evidence_open,
            &[("e evidence", title)],
            &[],
        );
        let mut companies = Screen::new(34, bottom.saturating_sub(1));
        self.paint_research_companies(&mut companies);
        screen.blit_at(&companies, 2, 1);
        let mut report = Screen::new(report_right - 38, bottom.saturating_sub(1));
        self.paint_report_live(&mut report);
        screen.blit_at(&report, 38, 1);
        let mut evidence = Screen::new(38, bottom.saturating_sub(1));
        let target = self
            .desk
            .instruments
            .get(self.desk.selected)
            .map(|item| item.instrument.symbol.as_str())
            .unwrap_or_default();
        self.research.paint(&mut evidence, target);
        screen.blit_at(&evidence, evidence_left + 1, 1);
    }

    fn paint_research_companies(&self, screen: &mut Screen) {
        let bottom = screen.h.saturating_sub(2);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        screen.text(2, 0, "Company · ↑↓ select", Style::fg(color::BLUE).bold());
        screen.text(
            2,
            1,
            "r report · e evidence · n generate",
            Style::fg(color::MUTED),
        );
        let count = bottom.saturating_sub(4).max(1);
        let start = self.desk.selected.saturating_sub(count - 1);
        for (row, (index, item)) in self
            .desk
            .instruments
            .iter()
            .enumerate()
            .skip(start)
            .take(count)
            .enumerate()
        {
            screen.text(
                2,
                3 + row,
                &delta_tui::research::truncate(
                    &format!(
                        "{} {}",
                        if index == self.desk.selected {
                            "›"
                        } else {
                            " "
                        },
                        item.instrument.id
                    ),
                    screen.w.saturating_sub(4),
                ),
                if index == self.desk.selected {
                    Style::fg(color::WHITE).bg(color::BLUE_BG)
                } else {
                    Style::fg(color::FG)
                },
            );
        }
    }

    fn paint_report_live(&self, screen: &mut Screen) {
        let bottom = screen.h.saturating_sub(2);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        let Some(current) = self.desk.instruments.get(self.desk.selected) else {
            return;
        };
        screen.text(
            2,
            0,
            &format!("Research · {}", current.instrument.symbol),
            Style::fg(color::BLUE).bold(),
        );
        screen.text(
            2,
            1,
            "n generate · b brief · [ ] history · e evidence · ↑↓ scroll",
            Style::fg(color::MUTED),
        );
        let Some(markdown) = self.reports.get(&current.instrument.id) else {
            screen.text(
                2,
                4,
                "No report yet. Press n to build one from gathered evidence.",
                Style::fg(color::MUTED),
            );
            return;
        };
        let width = screen.w.saturating_sub(4).max(1);
        let lines = delta_tui::markdown::render(markdown, width);
        for (row, line) in lines
            .iter()
            .skip(self.research_scroll)
            .take(bottom.saturating_sub(3))
            .enumerate()
        {
            screen.text(2, row + 3, &line.text, line.style);
        }
    }

    fn paint_ask_live(&self, screen: &mut Screen) {
        if (screen.w < delta_tui::NARROW_WIDTH as usize || self.ask.zoomed)
            && self.ask.targets_focus
        {
            screen.fill(0, 0, screen.w, screen.h.saturating_sub(3), Style::DEFAULT);
            self.ask.paint_sidebar(screen);
        } else if screen.w >= delta_tui::NARROW_WIDTH as usize && !self.ask.zoomed {
            let bottom = screen.h.saturating_sub(3);
            let split = screen.w - 38;
            let title = Style::fg(color::BLUE).bold();
            screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
            screen.pane(
                1,
                0,
                split,
                bottom,
                !self.ask.targets_focus,
                &[("5 ask", title)],
                &[],
            );
            screen.pane(
                split + 1,
                0,
                screen.w - 2,
                bottom,
                self.ask.targets_focus,
                &[("t targets / citations", title)],
                &[],
            );
            let mut transcript = Screen::new(split - 2, bottom.saturating_sub(1));
            self.paint_ask_transcript(&mut transcript);
            screen.blit_at(&transcript, 2, 1);
            let mut sidebar = Screen::new(34, bottom.saturating_sub(1));
            self.ask.paint_sidebar(&mut sidebar);
            screen.blit_at(&sidebar, split + 2, 1);
        } else {
            self.paint_ask_transcript(screen);
        }
    }

    fn paint_ask_transcript(&self, screen: &mut Screen) {
        let bottom = screen.h.saturating_sub(2);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        screen.text(
            2,
            0,
            "Ask · grounded answers",
            Style::fg(color::BLUE).bold(),
        );
        screen.text(
            2,
            1,
            &format!(
                "scope: {} · i ask · t targets · s save",
                self.ask
                    .scope
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Style::fg(color::MUTED),
        );
        let marker = if self.ask_editing { "●" } else { ">" };
        screen.text(
            2,
            2,
            &format!("{marker} {}", self.ask_input),
            Style::fg(color::FG),
        );
        if self.ask_busy {
            screen.text(2, 3, "answering…", Style::fg(color::AMBER));
        }
        let mut lines = Vec::new();
        for turn in &self.ask_history {
            let label = if turn.role == "user" { "You" } else { "Delta" };
            lines.push(format!("{label} [{}]", turn.source));
            lines.extend(turn.text.lines().map(str::to_string));
            if !turn.citations.is_empty() {
                lines.push(format!("Sources: {}", turn.citations.join(" · ")));
            }
            lines.push(String::new());
        }
        let width = screen.w.saturating_sub(4).max(1);
        let wrapped = delta_tui::markdown::render(&lines.join("\n"), width);
        for (row, line) in wrapped
            .iter()
            .skip(self.ask_scroll)
            .take(bottom.saturating_sub(4))
            .enumerate()
        {
            screen.text(2, row + 4, &line.text, line.style);
        }
    }

    fn paint_theses_live(&self, screen: &mut Screen) {
        if self.theses.is_empty() && !self.journal_filter.editing {
            return;
        }
        let indices = self.journal_indices(Tab::Theses);
        delta_tui::theses_view::ThesisView {
            theses: &self.theses,
            indices: &indices,
            selected: self.thesis_selected,
            links: &self.thesis_links,
            evidence_selected: self.evidence_selected,
            framing_scroll: self.framing_scroll,
            note_scroll: self.note_scroll,
            items: &self.thesis_items,
            health: &self.thesis_health,
            filter: &self.journal_filter.text,
            focus: self.thesis_focus,
        }
        .paint(screen);
    }

    fn paint_thesis_summary(&self, screen: &mut Screen) {
        let Some(text) = &self.thesis_summary else {
            return;
        };
        let bottom = screen.h.saturating_sub(2);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        screen.text(
            2,
            0,
            "Thesis summary · Esc close · ↑↓ scroll",
            Style::fg(color::BLUE).bold(),
        );
        let width = screen.w.saturating_sub(4).max(1);
        let mut lines = Vec::new();
        for line in text.lines() {
            let chars = line.chars().collect::<Vec<_>>();
            if chars.is_empty() {
                lines.push(String::new());
            }
            for chunk in chars.chunks(width) {
                lines.push(chunk.iter().collect::<String>());
            }
        }
        for (row, line) in lines
            .iter()
            .skip(self.summary_scroll)
            .take(bottom.saturating_sub(2))
            .enumerate()
        {
            screen.text(2, row + 2, line, Style::fg(color::FG));
        }
    }

    fn paint_watchlist_rows(&self, screen: &mut Screen) {
        let narrow = screen.w < delta_tui::NARROW_WIDTH as usize;
        let bottom = screen.h.saturating_sub(4);
        let edge = if narrow {
            screen.w.saturating_sub(2)
        } else {
            45
        };
        if narrow && self.watchlist.detail_open {
            screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
            if !self.paint_live_metrics(screen, 0, bottom) {
                screen.text(
                    2,
                    1,
                    "Loading metrics… Enter retry · Esc back",
                    Style::fg(color::MUTED),
                );
            }
            return;
        }
        if narrow {
            screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
            screen.text(
                2,
                0,
                "Watchlist · Enter inspect",
                Style::fg(color::BLUE).bold(),
            );
        }
        screen.fill(3, 1, edge, bottom, Style::DEFAULT);
        screen.text(
            3,
            1,
            &delta_tui::research::truncate(
                &format!("↑↓ select · r range · i help · / {}", self.watchlist.search),
                edge.saturating_sub(3),
            ),
            Style::fg(color::MUTED),
        );
        let start = self
            .watchlist
            .selected
            .saturating_sub(bottom.saturating_sub(5));
        for (row, (index, item)) in self
            .watchlist
            .rows
            .iter()
            .enumerate()
            .skip(start)
            .take(bottom.saturating_sub(3))
            .enumerate()
        {
            if let delta_tui::watchlist::WatchRow::Group { asset_class, count } = item {
                let marker = if self.watchlist.is_collapsed(asset_class) {
                    "▸"
                } else {
                    "▾"
                };
                screen.text(
                    3,
                    row + 3,
                    &format!("{marker} {asset_class} · {count}"),
                    Style::fg(color::BLUE).bold(),
                );
                continue;
            }
            let delta_tui::watchlist::WatchRow::Target(target) = item else {
                continue;
            };
            let id = target.instruments().first().map(|i| i.id.clone());
            let price = self
                .desk
                .live
                .get(id.as_deref().unwrap_or_default())
                .copied()
                .or_else(|| {
                    self.desk
                        .instruments
                        .iter()
                        .find(|i| Some(&i.instrument.id) == id.as_ref())
                        .and_then(|i| i.bars.last().map(|b| b.0))
                });
            let label = price.map_or_else(|| "no data".to_string(), |p| format!("{p:.2}"));
            let marker = if index == self.watchlist.selected {
                "›"
            } else {
                " "
            };
            screen.text(
                3,
                row + 3,
                &delta_tui::research::truncate(
                    &format!("{marker} {:<14} {:>12}", target.name, label),
                    edge.saturating_sub(3),
                ),
                if index == self.watchlist.selected {
                    Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
                } else {
                    Style::fg(color::FG)
                },
            );
        }
        if self.watchlist.rows.is_empty() {
            screen.text(
                3,
                4,
                "No targets match. Esc clear · a add",
                Style::fg(color::MUTED),
            );
        }
        if !narrow
            && (self.watchlist.instrument_id().is_none()
                || self.desk.watch_state().metric.is_none())
        {
            screen.fill(48, 0, screen.w, bottom, Style::DEFAULT);
            screen.text(
                50,
                2,
                "Select a target with price history to inspect.",
                Style::fg(color::MUTED),
            );
            screen.text(50, 4, "U gather · a add target", Style::fg(color::MUTED));
        }
        if !narrow {
            let _ = self.paint_live_metrics(screen, 48, bottom);
        }
    }

    fn paint_live_metrics(&self, screen: &mut Screen, x: usize, bottom: usize) -> bool {
        let Some(id) = self.watchlist.instrument_id() else {
            return false;
        };
        let Some(instrument) = self
            .desk
            .instruments
            .iter()
            .find(|item| item.instrument.id == id)
            .map(|item| &item.instrument)
        else {
            return false;
        };
        let range = self.desk.range();
        let Some(metrics) = self
            .desk
            .asset_metrics
            .get(&(id.clone(), range.to_string()))
        else {
            return false;
        };
        if x >= screen.w {
            return false;
        }
        let width = screen.w - x;
        let mut view = Screen::new(width, bottom);
        let state = delta_tui::metrics_view::MetricsView {
            instrument,
            metrics,
            range,
            live_price: self.desk.live.get(&id).copied(),
            scroll: self.metrics_scroll,
        };
        state.paint(&mut view);
        screen.blit_at(&view, x, 0);
        true
    }

    fn paint_home_live(&self, screen: &mut Screen) {
        let narrow = screen.w < delta_tui::NARROW_WIDTH as usize;
        let wide = screen.w >= 160;
        let edge = if narrow {
            screen.w - 2
        } else if wide {
            screen.w / 2 - 1
        } else {
            59
        };
        let list_bottom = if narrow {
            5
        } else if wide {
            18
        } else {
            screen.h.saturating_sub(27)
        };
        screen.fill(2, 3, edge, list_bottom, Style::fg(color::FG));
        let visible = list_bottom.saturating_sub(3).max(1);
        let start = self.desk.selected.saturating_sub(visible - 1);
        for (row, (index, item)) in self
            .desk
            .instruments
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let price = self
                .desk
                .live
                .get(&item.instrument.id)
                .copied()
                .or_else(|| item.bars.last().map(|bar| bar.0));
            let previous = item.bars.iter().rev().nth(1).map(|bar| bar.0);
            let change = price
                .zip(previous)
                .filter(|(_, prev)| *prev != 0.0)
                .map(|(last, prev)| (last / prev - 1.0) * 100.0);
            let value =
                price.map_or_else(|| "—".into(), |value| delta_core::format::grouped(value, 2));
            let change = change.map_or_else(|| "—".into(), |value| format!("{value:+.1}%"));
            let style = if index == self.desk.selected {
                Style::fg(color::WHITE).bg(color::BLUE_BG)
            } else {
                Style::fg(color::FG)
            };
            screen.fill(2, 3 + row, edge, 4 + row, style);
            let label = format!(" {:<11}{:>11}{:>9}", item.instrument.symbol, value, change);
            screen.text(2, 3 + row, &label, style);
            let closes = item
                .bars
                .iter()
                .rev()
                .take(40)
                .map(|bar| bar.0)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let spark = delta_tui::braille::BrailleGraph::filled(closes)
                .rows(edge.saturating_sub(42).max(1), 1);
            if let Some(spark) = spark.first() {
                screen.text(41, 3 + row, spark, style);
            }
        }
        if !narrow {
            let inner = if wide { screen.w / 2 + 2 } else { 62 };
            screen.fill(inner, 8, screen.w - 2, 9, Style::DEFAULT);
            let top = if wide {
                22
            } else {
                screen.h.saturating_sub(23)
            };
            let bottom = screen.h.saturating_sub(11);
            screen.fill(inner, top, screen.w - 2, bottom, Style::DEFAULT);
            if self.desk.feed.fleet.is_empty() {
                screen.text(inner, top, "no theses yet", Style::fg(color::MUTED));
            } else {
                for (row, (claim, state)) in self
                    .desk
                    .feed
                    .fleet
                    .iter()
                    .take(bottom.saturating_sub(top))
                    .enumerate()
                {
                    screen.text(
                        inner,
                        top + row,
                        &delta_tui::research::truncate(
                            &format!("{state} · {claim}"),
                            screen.w.saturating_sub(inner + 2),
                        ),
                        Style::fg(color::FG),
                    );
                }
            }
        }
        if let Some(agenda) = &self.desk.feed.agenda {
            let top = if narrow {
                16
            } else {
                screen.h.saturating_sub(8)
            };
            for (row, line) in agenda.iter().enumerate() {
                if top + row >= screen.h.saturating_sub(3) {
                    break;
                }
                screen.fill(8, top + row, screen.w - 2, top + row + 1, Style::DEFAULT);
                screen.text(
                    8,
                    top + row,
                    line,
                    Style::fg(if line.starts_with('⚠') {
                        color::AMBER
                    } else {
                        color::MUTED
                    }),
                );
            }
        }
    }

    fn paint_decisions_live(&self, screen: &mut Screen) {
        if self.decisions.is_empty() && !self.journal_filter.editing {
            return;
        }
        let indices = self.journal_indices(Tab::Decisions);
        delta_tui::decisions_view::DecisionView {
            decisions: &self.decisions,
            indices: &indices,
            selected: self.decision_selected,
            reviews: &self.decision_reviews,
            filter: &self.journal_filter.text,
            detail_open: self.decision_detail_open,
            scroll: self.decision_scroll,
        }
        .paint(screen);
    }

    fn paint_form(&self, screen: &mut Screen) {
        let Some(form) = &self.form else {
            return;
        };
        screen.fill(0, 0, screen.w, screen.h.saturating_sub(1), Style::DEFAULT);
        let heading = match form.kind {
            FormKind::Thesis => "New thesis",
            FormKind::ThesisEdit => "Edit thesis",
            FormKind::Decision => "New decision",
            FormKind::DecisionEdit => "Edit decision",
            FormKind::Review => "New review",
            FormKind::Evidence => "Link evidence",
            FormKind::Llm => "Provider and model",
            FormKind::Target => "Add watch target",
            FormKind::RemoveTarget => "Remove watch target",
            FormKind::Source => "Configure SEC EDGAR",
            FormKind::Market => "Add or edit market",
            FormKind::RemoveMarket => "Remove market",
            FormKind::Provider => "Connect AI provider",
        };
        screen.text(2, 1, heading, Style::fg(color::BLUE).bold());
        screen.text(
            2,
            3,
            "Enter next · Ctrl+S save · Shift+Tab previous · Esc cancel",
            Style::fg(color::MUTED),
        );
        let visible = screen
            .h
            .saturating_sub(6)
            .checked_div(2)
            .unwrap_or(0)
            .max(1);
        let start = form.field.saturating_sub(visible - 1);
        for (i, label) in form.labels.iter().enumerate().skip(start).take(visible) {
            let value = if i == form.field {
                &form.input
            } else {
                form.answers
                    .get(i)
                    .or_else(|| form.defaults.get(i))
                    .map_or("", String::as_str)
            };
            let marker = if i == form.field { "›" } else { " " };
            let masked;
            let value = if matches!(form.kind, FormKind::Provider) && i == 3 {
                masked = "•".repeat(value.chars().count());
                &masked
            } else {
                value
            };
            screen.text(
                2,
                5 + (i - start) * 2,
                &format!("{marker} {label}: {value}"),
                if i == form.field {
                    Style::fg(color::BLUE)
                } else {
                    Style::fg(color::FG)
                },
            );
        }
    }

    fn paint_settings_live(&self, screen: &mut Screen) {
        if let Some(cfg) = &self.settings {
            delta_tui::settings_view::SettingsView {
                config: cfg,
                state: &self.settings_state,
                sources: &self.settings_sources,
                diagnostics: &self.settings_diagnostics,
                provider_connected: self.provider_connected,
            }
            .paint(screen);
        }
    }

    fn edit_selected_market(&mut self, remove: bool) {
        if let Some((id, market)) = self
            .settings
            .as_ref()
            .and_then(|cfg| cfg.markets.iter().nth(self.settings_state.market_selected))
        {
            self.form = Some(if remove {
                Form::new(FormKind::RemoveMarket).with_defaults(vec![id.clone(), String::new()])
            } else {
                Form::new(FormKind::Market)
                    .with_defaults(vec![
                        id.clone(),
                        market.label.clone(),
                        market.currency.clone(),
                        market.yahoo_suffix.clone(),
                    ])
                    .lock_id()
            });
        } else {
            self.status = "Select a market first".into();
        }
    }

    fn reload_footer(&mut self) {
        let provider = self
            .settings
            .as_ref()
            .map(|cfg| cfg.llm_provider.clone())
            .unwrap_or_default();
        self.footer = self
            .db_path
            .as_ref()
            .and_then(|path| delta_core::db::Db::open(path).ok())
            .and_then(|db| delta_tui::footer::FooterState::load(&db, provider.clone()).ok())
            .unwrap_or(delta_tui::footer::FooterState {
                provider,
                unavailable: true,
                ..Default::default()
            });
    }

    fn reload_settings(&mut self) {
        match delta_core::config::load_config(&self.config_path) {
            Ok((_, cfg)) => {
                let mut cfg = cfg;
                for plugin in delta_plugins::default_plugins() {
                    cfg.plugins
                        .entry(plugin.name().into())
                        .or_insert_with(|| serde_json::json!({"enabled":true}));
                }
                cfg.plugins.retain(|_, table| table.is_object());
                self.settings = Some(cfg);
                if let Some(cfg) = &self.settings {
                    let sec = cfg.plugins.get("sec_edgar");
                    let enabled = sec
                        .and_then(|spec| spec.get("enabled"))
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    let contact = sec
                        .and_then(|spec| spec.get("contact"))
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or_default();
                    self.settings_sources = vec![delta_tui::settings_view::SettingsSource {
                        id: "sec_edgar".into(),
                        label: "SEC EDGAR".into(),
                        status: if !enabled {
                            "disabled"
                        } else if contact.contains('@') {
                            "ready"
                        } else {
                            "needs contact"
                        }
                        .into(),
                    }];
                    self.settings_state
                        .reconcile(cfg, self.settings_sources.len());
                    self.provider_connected = delta_services::config_ops::provider_connected(
                        cfg,
                        &self.config_path.with_file_name(".env"),
                    );
                }
            }
            Err(e) => {
                self.status = format!("settings: {e}");
                self.settings = None;
            }
        }
        self.reload_footer();
    }

    fn reload_journal(&mut self) {
        let Some(path) = &self.db_path else {
            return;
        };
        match delta_core::db::Db::open(path) {
            Ok(db) => {
                match delta_services::list_theses(&db) {
                    Ok(items) => self.theses = items,
                    Err(e) => self.status = format!("theses: {e}"),
                }
                if let Ok(fleet) = delta_services::thesis_fleet(&db, None) {
                    self.thesis_health = fleet
                        .into_iter()
                        .filter_map(|row| row.result.map(|health| (row.thesis.id, health)))
                        .collect();
                }
                match delta_services::list_decisions(&db, None, true) {
                    Ok(items) => self.decisions = items,
                    Err(e) => self.status = format!("decisions: {e}"),
                }
            }
            Err(e) => self.status = format!("journal database: {e}"),
        }
        self.thesis_selected = self
            .thesis_selected
            .min(self.theses.len().saturating_sub(1));
        self.decision_selected = self
            .decision_selected
            .min(self.decisions.len().saturating_sub(1));
        self.reload_thesis_links();
        self.reload_decision_reviews();
    }

    fn reload_thesis_links(&mut self) {
        self.thesis_links.clear();
        self.thesis_items.clear();
        if let (Some(path), Some(thesis)) = (&self.db_path, self.theses.get(self.thesis_selected)) {
            if let Ok(db) = delta_core::db::Db::open(path) {
                match delta_services::thesis_evidence(&db, &thesis.id, false) {
                    Ok(links) => self.thesis_links = links,
                    Err(e) => self.status = format!("thesis evidence: {e}"),
                }
                let ids = self
                    .thesis_links
                    .iter()
                    .map(|link| link.evidence_id.clone())
                    .collect::<Vec<_>>();
                if let Ok(items) = delta_services::evidence_by_ids(&db, &ids) {
                    self.thesis_items = items
                        .into_iter()
                        .map(|item| (item.id.clone(), item))
                        .collect();
                }
            }
        }
        self.evidence_selected = self
            .evidence_selected
            .min(self.thesis_links.len().saturating_sub(1));
    }

    fn reload_decision_reviews(&mut self) {
        self.decision_reviews.clear();
        if let (Some(path), Some(decision)) =
            (&self.db_path, self.decisions.get(self.decision_selected))
        {
            if let Ok(db) = delta_core::db::Db::open(path) {
                match delta_services::review_history(&db, &decision.id) {
                    Ok(history) => self.decision_reviews = history,
                    Err(e) => self.status = format!("decision reviews: {e}"),
                }
            }
        }
    }

    fn submit_form(&mut self, form: Form) {
        if matches!(form.kind, FormKind::Provider) {
            self.pending_action = Some(Action::ConnectProvider(delta_services::ProviderSetup {
                name: form.answers[0].clone(),
                base_url: form.answers[1].clone(),
                api_key_env: form.answers[2].clone(),
                key: form.answers[3].clone(),
            }));
            self.status = "connecting provider…".into();
            return;
        }
        if matches!(
            form.kind,
            FormKind::Source | FormKind::Market | FormKind::RemoveMarket
        ) {
            let a = &form.answers;
            let result = match form.kind {
                FormKind::Source => {
                    let markets = a[1]
                        .split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_lowercase)
                        .collect::<Vec<_>>();
                    delta_services::configure_data_provider(
                        &self.config_path,
                        "sec_edgar",
                        &std::collections::BTreeMap::from([("contact".into(), a[0].clone())]),
                        Some(&markets),
                    )
                }
                FormKind::Market => {
                    delta_services::save_market(&self.config_path, &a[0], &a[1], &a[2], &a[3])
                }
                FormKind::RemoveMarket if a[1] == "REMOVE" => {
                    delta_services::remove_market(&self.config_path, &a[0])
                }
                _ => Err(delta_services::ServiceError::invalid(
                    "confirmation must be REMOVE",
                )),
            };
            match result {
                Ok(()) => {
                    self.reload_watchlist();
                    self.status = "settings saved".into();
                }
                Err(error) => {
                    self.status = format!("settings: {error}");
                    self.form = Some(form.retry());
                }
            }
            return;
        }
        if matches!(form.kind, FormKind::Target | FormKind::RemoveTarget) {
            let a = &form.answers;
            let result = if matches!(form.kind, FormKind::Target) {
                let split = |raw: &str| {
                    raw.split(',')
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                };
                delta_services::add_target(
                    &self.config_path,
                    &a[0],
                    &a[1],
                    &a[2],
                    &split(&a[3]),
                    &split(&a[5]),
                    &a[6],
                    None,
                    &a[4],
                )
            } else if a[1] == "REMOVE" {
                delta_services::remove_target(&self.config_path, &a[0])
            } else {
                Err(delta_services::ServiceError::invalid(
                    "removal cancelled: confirmation must be REMOVE",
                ))
            };
            match result {
                Ok(()) => {
                    self.reload_watchlist();
                    self.status = if matches!(form.kind, FormKind::Target) {
                        "watch target added"
                    } else {
                        "watch target removed"
                    }
                    .into();
                }
                Err(error) => {
                    self.status = format!("watchlist: {error}");
                    self.form = Some(form.retry());
                }
            }
            return;
        }
        if matches!(form.kind, FormKind::Llm) {
            let result = delta_services::save_llm_settings(
                &self.config_path,
                &form.answers[0],
                &form.answers[1],
            );
            match result {
                Ok(()) => self.status = "model settings saved".into(),
                Err(error) => {
                    self.status = format!("settings: {error}");
                    self.form = Some(form.retry());
                }
            }
            self.reload_settings();
            return;
        }
        let Some(path) = &self.db_path else {
            self.status = "no configured database".into();
            self.form = Some(form.retry());
            return;
        };
        let mut db = match delta_core::db::Db::open(path) {
            Ok(db) => db,
            Err(e) => {
                self.status = format!("database: {e}");
                self.form = Some(form.retry());
                return;
            }
        };
        let a = &form.answers;
        let result = match form.kind {
            FormKind::Thesis | FormKind::ThesisEdit => {
                let split_items = |raw: &str, separator: char| {
                    raw.split(separator)
                        .map(str::trim)
                        .filter(|v| !v.is_empty())
                        .map(str::to_string)
                        .collect::<Vec<_>>()
                };
                let assumptions = split_items(&a[2], ';');
                let falsifiers = split_items(&a[3], ';');
                let targets = a[4]
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_string)
                    .collect::<Vec<_>>();
                if matches!(form.kind, FormKind::ThesisEdit) {
                    let Some(old) = self.theses.get(self.thesis_selected) else {
                        return;
                    };
                    delta_services::update_thesis(
                        &mut db,
                        &old.id,
                        &delta_services::ThesisEdit {
                            claim: a[0].clone(),
                            scope: a[1].clone(),
                            assumptions,
                            falsifiers,
                            targets,
                            time_horizon: a[5].clone(),
                            status: a.get(6).cloned().unwrap_or_else(|| old.status.clone()),
                        },
                    )
                    .map(|v| format!("thesis {} updated", &v.id[..8]))
                } else {
                    delta_services::create_thesis_with_fields(
                        &db,
                        &a[0],
                        &a[1],
                        &assumptions,
                        &falsifiers,
                        &targets,
                        &a[5],
                    )
                    .and_then(|v| {
                        for id in &form.citations {
                            delta_services::add_thesis_evidence(
                                &db,
                                &v.id,
                                id,
                                delta_services::EvidenceSide::Support,
                                "from ask",
                                Some(true),
                            )?;
                        }
                        Ok(format!("thesis {} created", &v.id[..8]))
                    })
                }
            }
            FormKind::Decision | FormKind::DecisionEdit => {
                let date = match chrono::NaiveDate::parse_from_str(&a[4], "%Y-%m-%d") {
                    Ok(d) => d,
                    Err(_) => {
                        self.status = "review date must be YYYY-MM-DD".into();
                        self.form = Some(form.retry());
                        return;
                    }
                };
                let input = delta_services::DecisionInput {
                    instrument_id: a[0].clone(),
                    rationale: a[1].clone(),
                    valuation_context: a[2].clone(),
                    time_horizon: a[3].clone(),
                    review_date: date,
                    invalidation_criteria: a[5].clone(),
                    thesis_id: if a[6].trim().is_empty() {
                        None
                    } else {
                        Some(a[6].trim().to_string())
                    },
                };
                if matches!(form.kind, FormKind::DecisionEdit) {
                    let Some(old) = self.decisions.get(self.decision_selected) else {
                        return;
                    };
                    delta_services::update_decision(&db, &old.id, &input)
                        .map(|v| format!("decision {} updated", &v.id[..8]))
                } else {
                    delta_services::create_decision(&db, &input)
                        .map(|v| format!("decision {} created", &v.id[..8]))
                }
            }
            FormKind::Review => {
                let Some(decision) = self.decisions.get(self.decision_selected) else {
                    return;
                };
                let status = if a[1].trim().is_empty() {
                    None
                } else {
                    Some(a[1].trim())
                };
                delta_services::append_review(&mut db, &decision.id, &a[0], status)
                    .map(|_| "review saved".into())
            }
            FormKind::Evidence => {
                let Some(thesis) = self.theses.get(self.thesis_selected) else {
                    return;
                };
                let Some(side) = delta_services::EvidenceSide::parse(a[1].trim()) else {
                    self.status = "side must be support, against, or neutral".into();
                    self.form = Some(form.retry());
                    return;
                };
                delta_services::add_thesis_evidence(&db, &thesis.id, &a[0], side, &a[2], None)
                    .map(|_| "evidence candidate linked".into())
            }
            FormKind::Llm
            | FormKind::Target
            | FormKind::RemoveTarget
            | FormKind::Source
            | FormKind::Market
            | FormKind::RemoveMarket
            | FormKind::Provider => unreachable!(),
        };
        match result {
            Ok(message) => self.status = message,
            Err(error) => {
                self.status = format!("journal: {error}");
                self.form = Some(form.retry());
            }
        }
        self.reload_journal();
    }

    fn reload_watchlist(&mut self) {
        let selected = self
            .desk
            .instruments
            .get(self.desk.selected)
            .map(|item| item.instrument.id.clone());
        let mut replacement = Desk::open_at(&self.config_path);
        replacement.last_seen = self.desk.last_seen;
        replacement.live = std::mem::take(&mut self.desk.live);
        replacement.metrics = std::mem::take(&mut self.desk.metrics);
        replacement.asset_metrics = std::mem::take(&mut self.desk.asset_metrics);
        let ids = replacement
            .instruments
            .iter()
            .map(|item| item.instrument.id.as_str())
            .collect::<std::collections::BTreeSet<_>>();
        replacement.live.retain(|id, _| ids.contains(id.as_str()));
        replacement
            .metrics
            .retain(|id, _| ids.contains(id.as_str()));
        replacement
            .asset_metrics
            .retain(|(id, _), _| ids.contains(id.as_str()));
        self.desk = replacement;
        if let Some(id) = selected {
            if let Some(index) = self
                .desk
                .instruments
                .iter()
                .position(|item| item.instrument.id == id)
            {
                self.desk.selected = index;
            }
        }
        self.reload_settings();
        self.db_path = self
            .settings
            .as_ref()
            .map(|cfg| PathBuf::from(&cfg.db_path));
        self.configuration_changed = true;
        self.reload_watchlist_browser();
        self.reload_journal();
    }

    fn reload_watchlist_browser(&mut self) {
        if let Err(error) = self.watchlist.reload(&self.config_path) {
            self.status = format!("watch targets: {error}");
        }
        self.sync_watchlist_selection();
        if let Err(error) = self.ask.reload(&self.config_path) {
            self.status = format!("ask scope: {error}");
        }
    }

    fn sync_watchlist_selection(&mut self) {
        if let Some(id) = self.watchlist.instrument_id() {
            if let Some(index) = self
                .desk
                .instruments
                .iter()
                .position(|item| item.instrument.id == id)
            {
                self.desk.selected = index;
            }
        }
    }

    fn reload_research(&mut self) {
        if let (Some(path), Some(current)) =
            (&self.db_path, self.desk.instruments.get(self.desk.selected))
        {
            if let Err(error) = self.research.reload(path, &current.instrument.id) {
                self.status = format!("evidence: {error}");
            }
        }
    }

    fn show_report_history(&mut self, step: isize) {
        if let (Some(cfg), Some(current)) = (
            &self.settings,
            self.desk.instruments.get(self.desk.selected),
        ) {
            let history = delta_services::report_history(
                std::path::Path::new(&cfg.reports_dir),
                &current.instrument.id,
            );
            if history.is_empty() {
                self.status = "No report history".into();
                return;
            }
            self.research.history_index = self
                .research
                .history_index
                .saturating_add_signed(step)
                .min(history.len() - 1);
            let report = &history[self.research.history_index];
            self.reports.insert(
                current.instrument.id.clone(),
                delta_services::render_markdown(report, true),
            );
            self.status = format!(
                "Report {} of {} · {}",
                self.research.history_index + 1,
                history.len(),
                report.as_of
            );
            self.research.company_open = false;
            self.research.evidence_open = false;
            self.research_scroll = 0;
        }
    }

    fn open_citation(&mut self, id: &str) {
        let Some(path) = &self.db_path else {
            self.status = "No configured evidence database".into();
            return;
        };
        let result = delta_core::db::Db::open(path)
            .map_err(delta_services::ServiceError::from)
            .and_then(|db| delta_services::source_url(&db, id));
        match result {
            Ok(Some(url)) => {
                let mut command = if cfg!(target_os = "macos") {
                    std::process::Command::new("open")
                } else if cfg!(target_os = "windows") {
                    let mut command = std::process::Command::new("rundll32");
                    command.arg("url.dll,FileProtocolHandler");
                    command
                } else {
                    std::process::Command::new("xdg-open")
                };
                self.status = command
                    .arg(url)
                    .spawn()
                    .map(|_| "source opened".into())
                    .unwrap_or_else(|error| format!("open source: {error}"));
            }
            Ok(None) => self.status = "This evidence has no browser URL".into(),
            Err(error) => self.status = error.to_string(),
        }
    }

    fn journal_indices(&self, tab: Tab) -> Vec<usize> {
        let query = self.journal_filter.text.to_lowercase();
        if tab == Tab::Theses {
            self.theses
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    format!("{} {} {}", row.claim, row.scope, row.targets.join(" "))
                        .to_lowercase()
                        .contains(&query)
                })
                .map(|(index, _)| index)
                .collect()
        } else {
            self.decisions
                .iter()
                .enumerate()
                .filter(|(_, row)| {
                    format!("{} {}", row.instrument_id, row.rationale)
                        .to_lowercase()
                        .contains(&query)
                })
                .map(|(index, _)| index)
                .collect()
        }
    }

    fn move_journal_selection(&mut self, step: isize) {
        let indices = self.journal_indices(self.tab);
        let current = if self.tab == Tab::Theses {
            self.thesis_selected
        } else {
            self.decision_selected
        };
        let position = indices.iter().position(|i| *i == current).unwrap_or(0);
        let next = position
            .saturating_add_signed(step)
            .min(indices.len().saturating_sub(1));
        if self.tab == Tab::Theses {
            self.framing_scroll = 0;
            self.note_scroll = 0;
            self.thesis_selected = indices.get(next).copied().unwrap_or(self.theses.len());
            self.reload_thesis_links();
        } else {
            self.decision_selected = indices.get(next).copied().unwrap_or(self.decisions.len());
            self.reload_decision_reviews();
        }
    }
}

impl Component for App {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Action::Quit);
        }
        if self.help_open {
            if is_quit_key(key) {
                return Some(Action::Quit);
            }
            if matches!(key.code, KeyCode::Esc | KeyCode::Char('?')) {
                self.help_open = false;
            }
            return None;
        }
        if let Some(palette) = &mut self.palette {
            let action = palette.handle_key(key);
            match action {
                Some(Action::CloseDialog) => {
                    self.palette = None;
                    self.palette_model = false;
                }
                Some(Action::Goto(name)) => {
                    self.palette = None;
                    if self.palette_model {
                        self.palette_model = false;
                        let provider = self
                            .settings
                            .as_ref()
                            .map(|cfg| cfg.llm_provider.as_str())
                            .unwrap_or("openrouter");
                        self.status =
                            delta_services::save_llm_settings(&self.config_path, provider, &name)
                                .map(|_| format!("model set to {name}"))
                                .unwrap_or_else(|error| error.to_string());
                        self.reload_settings();
                    } else {
                        self.update(Action::Goto(name));
                    }
                }
                _ => {}
            }
            return None;
        }
        if self.tab == Tab::Theses && self.thesis_summary.is_some() {
            match key.code {
                KeyCode::Esc => self.thesis_summary = None,
                KeyCode::Down => self.summary_scroll = self.summary_scroll.saturating_add(1),
                KeyCode::Up => self.summary_scroll = self.summary_scroll.saturating_sub(1),
                _ => {}
            }
            return None;
        }
        if let Some(form) = &mut self.form {
            match key.code {
                KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.answers.push(std::mem::take(&mut form.input));
                    for index in form.field + 1..form.labels.len() {
                        form.answers
                            .push(form.defaults.get(index).cloned().unwrap_or_default());
                    }
                    let completed = self.form.take().unwrap();
                    self.submit_form(completed);
                    return self.pending_action.take();
                }
                KeyCode::Esc => self.form = None,
                KeyCode::Enter => {
                    form.answers.push(std::mem::take(&mut form.input));
                    form.field += 1;
                    if form.field == form.labels.len() {
                        let completed = self.form.take().unwrap();
                        self.submit_form(completed);
                        return self.pending_action.take();
                    } else {
                        form.input = form.defaults.get(form.field).cloned().unwrap_or_default();
                    }
                }
                KeyCode::Backspace => {
                    form.input.pop();
                }
                KeyCode::BackTab | KeyCode::Up
                    if form.field > usize::from(form.locked_id.is_some()) =>
                {
                    if form.defaults.len() <= form.field {
                        form.defaults.resize(form.field + 1, String::new());
                    }
                    form.defaults[form.field] = std::mem::take(&mut form.input);
                    form.field -= 1;
                    form.input = form.answers.pop().unwrap_or_default();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.input.push(c)
                }
                _ => {}
            }
            return None;
        }
        if self.tab == Tab::Ask && self.ask_editing {
            match key.code {
                KeyCode::Esc => self.ask_editing = false,
                KeyCode::Enter if !self.ask_busy => {
                    let question = self.ask_input.trim().to_string();
                    if !question.is_empty() {
                        self.ask_input.clear();
                        self.ask_editing = false;
                        return Some(Action::AskQuestion(question));
                    }
                }
                KeyCode::Backspace => {
                    self.ask_input.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.ask_input.push(c);
                }
                _ => {}
            }
            return None;
        }
        if self.tab == Tab::Research && self.research.search_editing {
            match key.code {
                KeyCode::Esc => self.research.search_editing = false,
                KeyCode::Enter => {
                    self.research.search_editing = false;
                    self.reload_research();
                }
                KeyCode::Backspace => {
                    self.research.search.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.research.search.push(c)
                }
                _ => {}
            }
            return None;
        }
        if self.tab == Tab::Watchlist && self.watchlist.editing {
            match key.code {
                KeyCode::Esc => {
                    self.watchlist.detail_open = false;
                    self.watchlist.search.clear();
                    self.watchlist.editing = false;
                }
                KeyCode::Enter => self.watchlist.editing = false,
                KeyCode::Backspace => {
                    self.watchlist.search.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.watchlist.search.push(c)
                }
                _ => {}
            }
            self.watchlist.rebuild();
            self.sync_watchlist_selection();
            return None;
        }
        if matches!(self.tab, Tab::Theses | Tab::Decisions) && self.journal_filter.editing {
            match key.code {
                KeyCode::Esc => {
                    self.journal_filter.text.clear();
                    self.journal_filter.editing = false;
                }
                KeyCode::Enter => self.journal_filter.editing = false,
                KeyCode::Backspace => {
                    self.journal_filter.text.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    self.journal_filter.text.push(c)
                }
                _ => {}
            }
            self.move_journal_selection(0);
            return None;
        }
        if is_quit_key(key) {
            return Some(Action::Quit);
        }
        match key.code {
            KeyCode::Esc => {
                if self.tab == Tab::Research && (self.report_busy || self.gather_busy) {
                    let action = if self.report_busy {
                        Action::CancelReport
                    } else {
                        Action::CancelGather
                    };
                    self.report_busy = false;
                    self.gather_busy = false;
                    return Some(action);
                }
                self.pending_delete = None;
                self.ask.clear_pending = false;
                self.ask.targets_focus = false;
                self.ask.zoomed = false;
                self.decision_detail_open = false;
                self.thesis_focus = delta_tui::theses_view::ThesisFocus::Claims;
                self.journal_filter.text.clear();
                self.palette_model = false;
                self.glossary = false;
                if self.research.evidence_open && self.research.detail_open {
                    self.research.detail_open = false;
                    self.research.preview_scroll = 0;
                } else {
                    self.research.evidence_open = false;
                    self.research.company_open = true;
                }
                self.research.zoomed = false;
                self.watchlist.detail_open = false;
                self.metrics_scroll = 0;
                self.watchlist.search.clear();
                self.watchlist.rebuild();
                self.settings_state.back();
            }
            KeyCode::Char('1') | KeyCode::Char('h') => self.tab = Tab::Home,
            KeyCode::Char('?') => self.help_open = true,
            KeyCode::Char('g') => {
                self.palette_model = false;
                self.palette = Some(delta_tui::components::CommandPalette::new(
                    [
                        "Home",
                        "Watchlist",
                        "Research",
                        "Theses",
                        "Ask",
                        "Decisions",
                        "Settings",
                    ]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
                ))
            }
            KeyCode::Char('2') => self.tab = Tab::Watchlist,
            KeyCode::Char('3') => {
                self.tab = Tab::Research;
                self.reload_research();
            }
            KeyCode::Char('4') => self.update(Action::Goto("Theses".into())),
            KeyCode::Char('5') => self.tab = Tab::Ask,
            KeyCode::Char('6') => self.update(Action::Goto("Decisions".into())),
            KeyCode::Char('c') => {
                self.tab = Tab::Settings;
                self.reload_settings();
                return Some(Action::LoadDiagnostics);
            }
            KeyCode::Down if self.tab == Tab::Home => self.desk.cycle_instrument(1),
            KeyCode::Up if self.tab == Tab::Home => self.desk.cycle_instrument(-1),
            KeyCode::Enter if self.tab == Tab::Home => {
                if let Some(instrument) = self
                    .desk
                    .instruments
                    .get(self.desk.selected)
                    .map(|item| &item.instrument)
                {
                    if let Some(index) = self.watchlist.rows.iter().position(|row| matches!(row, delta_tui::watchlist::WatchRow::Target(target) if target.instruments().iter().any(|member| member.id == instrument.id))) {
                        self.watchlist.selected = index;
                        self.watchlist.member = self.watchlist.target().and_then(|target| target.tickers.iter().position(|symbol| symbol == &instrument.symbol)).unwrap_or(0);
                    }
                }
                self.tab = Tab::Watchlist;
            }
            KeyCode::Char('a') if matches!(self.tab, Tab::Watchlist | Tab::Home) => {
                self.form = Some(Form::new(FormKind::Target).with_defaults(vec![
                    String::new(),
                    "company".into(),
                    "us".into(),
                    String::new(),
                    "equity".into(),
                    String::new(),
                    String::new(),
                ]))
            }
            KeyCode::Char('d') if self.tab == Tab::Watchlist => {
                let name = self
                    .watchlist
                    .target()
                    .map(|target| target.id.clone())
                    .unwrap_or_default();
                self.form = Some(
                    Form::new(FormKind::RemoveTarget).with_defaults(vec![name, String::new()]),
                );
            }
            KeyCode::Char('r') if self.tab == Tab::Settings => {
                self.reload_settings();
                return Some(Action::LoadDiagnostics);
            }
            KeyCode::Char('s') if self.tab == Tab::Settings => {
                let table = self
                    .settings
                    .as_ref()
                    .and_then(|cfg| cfg.plugins.get("sec_edgar"));
                let contact = table
                    .and_then(|v| v.get("contact"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string();
                self.form =
                    Some(Form::new(FormKind::Source).with_defaults(vec![contact, "us".into()]));
            }
            KeyCode::Char('a') if self.tab == Tab::Settings => {
                self.form = Some(Form::new(FormKind::Market))
            }
            KeyCode::Char('e') if self.tab == Tab::Settings => self.edit_selected_market(false),
            KeyCode::Char('x') if self.tab == Tab::Settings => self.edit_selected_market(true),
            KeyCode::Char('d') if self.tab == Tab::Settings => {
                self.settings_state.toggle_diagnostics(self.viewport_width)
            }
            KeyCode::Tab | KeyCode::BackTab if self.tab == Tab::Settings => self
                .settings_state
                .cycle_focus(key.code == KeyCode::BackTab),
            KeyCode::Char('l') if self.tab == Tab::Settings => {
                self.settings_state.focus = delta_tui::settings_view::SettingsFocus::Plugins
            }
            KeyCode::Enter if self.tab == Tab::Settings => {
                use delta_tui::settings_view::SettingsFocus;
                match self.settings_state.focus {
                    SettingsFocus::Provider => {
                        return self.handle_key(KeyEvent::new(
                            KeyCode::Char(if self.settings_state.provider_selected == 0 {
                                'p'
                            } else {
                                'm'
                            }),
                            KeyModifiers::NONE,
                        ));
                    }
                    SettingsFocus::Sources => {
                        return self
                            .handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
                    }
                    SettingsFocus::Markets => self.edit_selected_market(false),
                    SettingsFocus::Plugins => {
                        if let Some((name, spec)) = self.settings.as_ref().and_then(|cfg| {
                            cfg.plugins.iter().nth(self.settings_state.plugin_selected)
                        }) {
                            self.status = format!("{name}: {spec} · t toggle");
                        }
                    }
                    SettingsFocus::Diagnostics => {}
                }
            }
            KeyCode::Char('p') => {
                let defaults = self
                    .settings
                    .as_ref()
                    .map(|cfg| {
                        vec![
                            cfg.llm_provider.clone(),
                            cfg.llm_base_url.clone(),
                            cfg.llm_api_key_env.clone(),
                            String::new(),
                        ]
                    })
                    .unwrap_or_default();
                self.form = Some(Form::new(FormKind::Provider).with_defaults(defaults));
            }
            KeyCode::Char('m') => {
                self.palette_model = true;
                self.status = "loading models…".into();
                return Some(Action::LoadModels);
            }
            KeyCode::Char('M') => {
                let defaults = self
                    .settings
                    .as_ref()
                    .map(|cfg| vec![cfg.llm_provider.clone(), cfg.llm_model.clone()])
                    .unwrap_or_default();
                self.form = Some(Form::new(FormKind::Llm).with_defaults(defaults));
            }
            KeyCode::Down | KeyCode::Up if self.tab == Tab::Settings => {
                if let Some(cfg) = &self.settings {
                    self.settings_state.move_selection(
                        if key.code == KeyCode::Down { 1 } else { -1 },
                        cfg.plugins.len(),
                        self.settings_sources.len(),
                        cfg.markets.len(),
                    );
                }
            }
            KeyCode::Char('t') if self.tab == Tab::Settings => {
                if let Some((name, spec)) = self
                    .settings
                    .as_ref()
                    .and_then(|cfg| cfg.plugins.iter().nth(self.settings_state.plugin_selected))
                {
                    let enabled = spec
                        .get("enabled")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true);
                    self.status =
                        delta_services::set_plugin_enabled(&self.config_path, name, !enabled)
                            .map(|_| {
                                format!("{name}: {}", if enabled { "disabled" } else { "enabled" })
                            })
                            .unwrap_or_else(|e| format!("settings: {e}"));
                    self.reload_settings();
                }
            }
            KeyCode::Enter if self.tab == Tab::Watchlist => {
                self.watchlist.detail_open = true;
                self.metrics_scroll = 0;
                if let Some(item) = self.desk.instruments.get(self.desk.selected) {
                    if self.watchlist.instrument_id().is_some() {
                        return Some(Action::RefreshMetrics(item.instrument.id.clone()));
                    }
                }
            }
            KeyCode::Char('i') if self.tab == Tab::Watchlist => self.glossary = !self.glossary,
            KeyCode::Char('R') if self.tab == Tab::Watchlist => {
                self.desk.cycle_range(-1);
            }
            KeyCode::Char('l') | KeyCode::Char('r') if self.tab == Tab::Watchlist => {
                self.desk.cycle_range(1);
            }
            KeyCode::Char('/') if self.tab == Tab::Watchlist => self.watchlist.editing = true,
            KeyCode::Char(' ') if self.tab == Tab::Watchlist => self.watchlist.toggle_group(),
            KeyCode::Down if self.tab == Tab::Watchlist => {
                if self.viewport_width < delta_tui::NARROW_WIDTH as usize
                    && self.watchlist.detail_open
                {
                    self.metrics_scroll = self.metrics_scroll.saturating_add(1);
                } else {
                    self.watchlist.move_selection(1);
                    self.sync_watchlist_selection();
                }
            }
            KeyCode::Up if self.tab == Tab::Watchlist => {
                if self.viewport_width < delta_tui::NARROW_WIDTH as usize
                    && self.watchlist.detail_open
                {
                    self.metrics_scroll = self.metrics_scroll.saturating_sub(1);
                } else {
                    self.watchlist.move_selection(-1);
                    self.sync_watchlist_selection();
                }
            }
            KeyCode::Left if self.tab == Tab::Watchlist => {
                self.watchlist.cycle_member(-1);
                self.sync_watchlist_selection();
            }
            KeyCode::Right if self.tab == Tab::Watchlist => {
                self.watchlist.cycle_member(1);
                self.sync_watchlist_selection();
            }
            KeyCode::Char(',') if !self.desk.instruments.is_empty() => {
                self.desk.cycle_instrument(-1);
                self.reload_research();
                self.research_scroll = 0;
            }
            KeyCode::Char('.') if !self.desk.instruments.is_empty() => {
                self.desk.cycle_instrument(1);
                self.reload_research();
                self.research_scroll = 0;
            }
            KeyCode::Char('u') if self.tab == Tab::Research => {
                if !self.gather_busy {
                    if let Some(item) = self.desk.instruments.get(self.desk.selected) {
                        self.gather_busy = true;
                        return Some(Action::GatherTargets(vec![item.instrument.id.clone()]));
                    }
                }
            }
            KeyCode::Char('U') if !self.gather_busy => {
                self.gather_busy = true;
                return Some(Action::Gather);
            }
            KeyCode::Char('e') if self.tab == Tab::Research => {
                self.research.evidence_open = true;
                self.research.company_open = false;
                self.reload_research();
            }
            KeyCode::Char('r') if self.tab == Tab::Research => {
                self.research.evidence_open = false;
                self.research.company_open = false;
            }
            KeyCode::Char('t') if self.tab == Tab::Research => {
                self.research.company_open = true;
                self.research.evidence_open = false;
            }
            KeyCode::Char('z') if self.tab == Tab::Research => {
                self.research.zoomed = !self.research.zoomed
            }
            KeyCode::Char('[') if self.tab == Tab::Research => self.show_report_history(1),
            KeyCode::Char(']') if self.tab == Tab::Research => self.show_report_history(-1),
            KeyCode::Char('b') if self.tab == Tab::Research => {
                if let (Some(path), Some(current)) =
                    (&self.db_path, self.desk.instruments.get(self.desk.selected))
                {
                    if let Ok(db) = delta_core::db::Db::open(path) {
                        let universe = self
                            .desk
                            .instruments
                            .iter()
                            .map(|item| item.instrument.clone())
                            .collect::<Vec<_>>();
                        if let Some(brief) = delta_services::brief_for(
                            &db,
                            &universe,
                            &current.instrument.id,
                            chrono::Utc::now().naive_utc(),
                        ) {
                            self.reports.insert(current.instrument.id.clone(), brief);
                            self.research.company_open = false;
                            self.research.evidence_open = false;
                            self.research_scroll = 0;
                        } else {
                            self.status = "No brief evidence available".into();
                        }
                    }
                }
            }
            KeyCode::Char('/') if self.tab == Tab::Research => {
                self.research.evidence_open = true;
                self.research.company_open = false;
                self.research.search_editing = true;
            }
            KeyCode::Char('k') if self.tab == Tab::Research => {
                self.research.cycle_kind();
                self.reload_research();
            }
            KeyCode::Char('o') if self.tab == Tab::Research => {
                if let Some(id) = self
                    .research
                    .items
                    .get(self.research.selected)
                    .map(|item| item.id.clone())
                {
                    self.open_citation(&id);
                }
            }
            KeyCode::Char('l') if self.tab == Tab::Research => {
                self.research.limit = self.research.limit.max(80) + 80;
                self.reload_research();
            }
            KeyCode::Enter if self.tab == Tab::Research && self.research.evidence_open => {
                self.research.detail_open = true;
                self.research.preview_scroll = 0;
            }
            KeyCode::Down
                if self.tab == Tab::Research
                    && self.research.evidence_open
                    && self.research.detail_open =>
            {
                self.research.preview_scroll = self.research.preview_scroll.saturating_add(1);
            }
            KeyCode::Up
                if self.tab == Tab::Research
                    && self.research.evidence_open
                    && self.research.detail_open =>
            {
                self.research.preview_scroll = self.research.preview_scroll.saturating_sub(1);
            }
            KeyCode::Down if self.tab == Tab::Research && self.research.evidence_open => {
                self.research.selected =
                    (self.research.selected + 1).min(self.research.items.len().saturating_sub(1));
            }
            KeyCode::Down if self.tab == Tab::Research && self.research.company_open => {
                self.desk.cycle_instrument(1);
                self.reload_research();
                self.research_scroll = 0;
            }
            KeyCode::Up if self.tab == Tab::Research && self.research.company_open => {
                self.desk.cycle_instrument(-1);
                self.reload_research();
                self.research_scroll = 0;
            }
            KeyCode::Up if self.tab == Tab::Research && self.research.evidence_open => {
                self.research.selected = self.research.selected.saturating_sub(1);
            }
            KeyCode::Char('n') if self.tab == Tab::Research && !self.report_busy => {
                self.report_busy = true;
                if let Some(current) = self.desk.instruments.get(self.desk.selected) {
                    return Some(Action::GenerateReport(current.instrument.id.clone()));
                }
            }
            KeyCode::Char('i') | KeyCode::Enter if self.tab == Tab::Ask => {
                self.ask_editing = true;
                self.ask.targets_focus = false;
            }
            KeyCode::Char('t') if self.tab == Tab::Ask => self.ask.targets_focus = true,
            KeyCode::Char(' ') if self.tab == Tab::Ask => self.ask.toggle_target(),
            KeyCode::Char('a') if self.tab == Tab::Ask => self.ask.toggle_all(),
            KeyCode::Char('z') if self.tab == Tab::Ask => self.ask.zoomed = !self.ask.zoomed,
            KeyCode::Char('s') if self.tab == Tab::Ask => {
                if let Some(answer) = self
                    .ask_history
                    .iter()
                    .rev()
                    .find(|turn| turn.role == "assistant")
                {
                    let claim = answer
                        .text
                        .split("\n\n")
                        .next()
                        .unwrap_or_default()
                        .to_string();
                    let mut form = Form::new(FormKind::Thesis).with_defaults(vec![
                        claim,
                        String::new(),
                        String::new(),
                        String::new(),
                        self.ask.instrument_ids().join(", "),
                        String::new(),
                    ]);
                    form.citations = answer
                        .citations
                        .iter()
                        .filter(|id| !id.starts_with("http://") && !id.starts_with("https://"))
                        .cloned()
                        .collect();
                    self.form = Some(form);
                } else {
                    self.status = "No answer to save yet".into();
                }
            }
            KeyCode::Char('o') if self.tab == Tab::Ask => {
                if let Some(id) = self.ask.citations.get(self.ask.citation_selected).cloned() {
                    if id.starts_with("http://") || id.starts_with("https://") {
                        self.open_citation(&id);
                    } else {
                        self.inspect_evidence(&id);
                    }
                }
            }
            KeyCode::Char('x') if self.tab == Tab::Ask => {
                self.ask.clear_pending = true;
                self.status = "Clear transcript? y confirm · Esc cancel".into();
            }
            KeyCode::Char('y') if self.tab == Tab::Ask && self.ask.clear_pending => {
                self.ask_generation = self.ask_generation.wrapping_add(1);
                self.ask_busy = false;
                self.ask_history.clear();
                self.ask.citations.clear();
                self.ask.items.clear();
                self.ask.clear_pending = false;
                self.ask_scroll = 0;
                self.status = "transcript cleared".into();
                return Some(Action::CancelChat);
            }
            KeyCode::Left if self.tab == Tab::Ask => {
                self.ask.citation_selected = (self.ask.citation_selected
                    + self.ask.citations.len().saturating_sub(1))
                    % self.ask.citations.len().max(1)
            }
            KeyCode::Right if self.tab == Tab::Ask => {
                self.ask.citation_selected =
                    (self.ask.citation_selected + 1) % self.ask.citations.len().max(1)
            }
            KeyCode::Down if self.tab == Tab::Ask && self.ask.targets_focus => {
                self.ask.target_selected =
                    (self.ask.target_selected + 1).min(self.ask.targets.len().saturating_sub(1))
            }
            KeyCode::Up if self.tab == Tab::Ask && self.ask.targets_focus => {
                self.ask.target_selected = self.ask.target_selected.saturating_sub(1)
            }
            KeyCode::Char('n') if self.tab == Tab::Theses => {
                self.form = Some(Form::new(FormKind::Thesis))
            }
            KeyCode::Char('E') | KeyCode::Char('d') if self.tab == Tab::Theses => {
                if let Some(old) = self.theses.get(self.thesis_selected) {
                    self.form = Some(Form::new(FormKind::ThesisEdit).with_defaults(vec![
                        old.claim.clone(),
                        old.scope.clone(),
                        old.assumptions.join("; "),
                        old.falsifiers.join("; "),
                        old.targets.join(", "),
                        old.time_horizon.clone(),
                        old.status.clone(),
                    ]));
                }
            }
            KeyCode::Char('n') if self.tab == Tab::Decisions => {
                self.form = Some(Form::new(FormKind::Decision))
            }
            KeyCode::Enter if self.tab == Tab::Decisions => self.decision_detail_open = true,
            KeyCode::Char('/') if matches!(self.tab, Tab::Theses | Tab::Decisions) => {
                self.journal_filter.editing = true
            }
            KeyCode::Char('d') if self.tab == Tab::Decisions => {
                if let Some(decision) = self.decisions.get(self.decision_selected) {
                    self.pending_delete = Some(decision.id.clone());
                    self.status = "Delete decision and its reviews? y confirm · Esc cancel".into();
                }
            }
            KeyCode::Char('y') if self.tab == Tab::Decisions => {
                if let (Some(id), Some(path)) = (self.pending_delete.take(), &self.db_path) {
                    match delta_core::db::Db::open(path) {
                        Ok(mut db) => {
                            self.status = delta_services::delete_decision(&mut db, &id)
                                .map(|_| "decision deleted".into())
                                .unwrap_or_else(|error| error.to_string());
                            self.reload_journal();
                        }
                        Err(error) => self.status = error.to_string(),
                    }
                }
            }
            KeyCode::Char('o') if self.tab == Tab::Decisions => {
                if let Some(decision) = self.decisions.get(self.decision_selected) {
                    if let Some(index) = self
                        .desk
                        .instruments
                        .iter()
                        .position(|item| item.instrument.id == decision.instrument_id)
                    {
                        self.desk.selected = index;
                        self.tab = Tab::Research;
                        self.research.evidence_open = true;
                        self.research.company_open = false;
                        self.reload_research();
                    } else {
                        self.status = "Decision instrument is no longer watched".into();
                    }
                }
            }
            KeyCode::Char('E') | KeyCode::Char('e') if self.tab == Tab::Decisions => {
                if let Some(old) = self.decisions.get(self.decision_selected) {
                    self.form = Some(Form::new(FormKind::DecisionEdit).with_defaults(vec![
                        old.instrument_id.clone(),
                        old.rationale.clone(),
                        old.valuation_context.clone(),
                        old.time_horizon.clone(),
                        old.review_date.to_string(),
                        old.invalidation_criteria.clone(),
                        old.thesis_id.clone().unwrap_or_default(),
                    ]));
                }
            }
            KeyCode::Char('r') if self.tab == Tab::Decisions && !self.decisions.is_empty() => {
                self.form = Some(Form::new(FormKind::Review))
            }
            KeyCode::Char('L') if self.tab == Tab::Theses && !self.theses.is_empty() => {
                self.form = Some(Form::new(FormKind::Evidence))
            }
            KeyCode::Char('t') if self.tab == Tab::Theses => {
                self.thesis_focus = delta_tui::theses_view::ThesisFocus::Framing
            }
            KeyCode::Char('e') if self.tab == Tab::Theses => {
                self.thesis_focus = delta_tui::theses_view::ThesisFocus::Evidence
            }
            KeyCode::Char('f')
                if self.tab == Tab::Theses && self.theses.get(self.thesis_selected).is_some() =>
            {
                return Some(Action::ProposeThesis(
                    self.theses[self.thesis_selected].id.clone(),
                ));
            }
            KeyCode::Char('s')
                if self.tab == Tab::Theses && self.theses.get(self.thesis_selected).is_some() =>
            {
                self.status = "writing thesis summary…".into();
                return Some(Action::SummarizeThesis(
                    self.theses[self.thesis_selected].id.clone(),
                ));
            }
            KeyCode::Char('j') if self.tab == Tab::Theses => {
                self.evidence_selected =
                    (self.evidence_selected + 1).min(self.thesis_links.len().saturating_sub(1))
            }
            KeyCode::Char('k') if self.tab == Tab::Theses => {
                self.evidence_selected = self.evidence_selected.saturating_sub(1)
            }
            KeyCode::Char('a' | 'u' | 'x') if self.tab == Tab::Theses => {
                if let (Some(path), Some(thesis), Some(link)) = (
                    &self.db_path,
                    self.theses.get(self.thesis_selected),
                    self.thesis_links.get(self.evidence_selected),
                ) {
                    if let Ok(db) = delta_core::db::Db::open(path) {
                        let result = match key.code {
                            KeyCode::Char('x') => delta_services::remove_thesis_evidence(
                                &db,
                                &thesis.id,
                                &link.evidence_id,
                            )
                            .map(|_| "evidence removed".to_string()),
                            KeyCode::Char('a') => delta_services::set_thesis_evidence_accepted(
                                &db,
                                &thesis.id,
                                &link.evidence_id,
                                true,
                            )
                            .map(|_| "evidence accepted".to_string()),
                            _ => delta_services::set_thesis_evidence_accepted(
                                &db,
                                &thesis.id,
                                &link.evidence_id,
                                false,
                            )
                            .map(|_| "evidence unaccepted".to_string()),
                        };
                        self.status = result.unwrap_or_else(|e| e.to_string());
                        self.reload_thesis_links();
                    }
                }
            }
            KeyCode::Down
                if self.tab == Tab::Theses
                    && self.thesis_focus == delta_tui::theses_view::ThesisFocus::Evidence
                    && key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.note_scroll = self.note_scroll.saturating_add(1);
            }
            KeyCode::Up
                if self.tab == Tab::Theses
                    && self.thesis_focus == delta_tui::theses_view::ThesisFocus::Evidence
                    && key.modifiers.contains(KeyModifiers::SHIFT) =>
            {
                self.note_scroll = self.note_scroll.saturating_sub(1);
            }
            KeyCode::Down if self.tab == Tab::Theses => {
                if self.thesis_focus == delta_tui::theses_view::ThesisFocus::Evidence {
                    self.note_scroll = 0;
                    self.evidence_selected =
                        (self.evidence_selected + 1).min(self.thesis_links.len().saturating_sub(1));
                } else if self.thesis_focus == delta_tui::theses_view::ThesisFocus::Framing {
                    self.framing_scroll = self.framing_scroll.saturating_add(1);
                } else {
                    self.move_journal_selection(1);
                }
            }
            KeyCode::Up if self.tab == Tab::Theses => {
                if self.thesis_focus == delta_tui::theses_view::ThesisFocus::Evidence {
                    self.note_scroll = 0;
                    self.evidence_selected = self.evidence_selected.saturating_sub(1);
                } else if self.thesis_focus == delta_tui::theses_view::ThesisFocus::Framing {
                    self.framing_scroll = self.framing_scroll.saturating_sub(1);
                } else {
                    self.move_journal_selection(-1);
                }
            }
            KeyCode::Down if self.tab == Tab::Decisions => {
                if self.decision_detail_open {
                    self.decision_scroll = self.decision_scroll.saturating_add(1);
                } else {
                    self.move_journal_selection(1);
                    self.decision_scroll = 0;
                }
            }
            KeyCode::Up if self.tab == Tab::Decisions => {
                if self.decision_detail_open {
                    self.decision_scroll = self.decision_scroll.saturating_sub(1);
                } else {
                    self.move_journal_selection(-1);
                    self.decision_scroll = 0;
                }
            }
            KeyCode::Char('P') if self.tab == Tab::Theses => {
                if let (Some(path), Some(thesis)) =
                    (&self.db_path, self.theses.get(self.thesis_selected))
                {
                    if let Ok(db) = delta_core::db::Db::open(path) {
                        let status = if thesis.status == "active" {
                            "paused"
                        } else {
                            "active"
                        };
                        self.status = delta_services::set_thesis_status(&db, &thesis.id, status)
                            .map(|_| format!("thesis {status}"))
                            .unwrap_or_else(|e| e.to_string());
                        self.reload_journal();
                    }
                }
            }
            KeyCode::Char('X') if self.tab == Tab::Theses => {
                if let (Some(path), Some(thesis)) =
                    (&self.db_path, self.theses.get(self.thesis_selected))
                {
                    if let Ok(db) = delta_core::db::Db::open(path) {
                        self.status =
                            delta_services::set_thesis_status(&db, &thesis.id, "concluded")
                                .map(|_| "thesis concluded".into())
                                .unwrap_or_else(|e| e.to_string());
                        self.reload_journal();
                    }
                }
            }
            KeyCode::Char('x') if self.tab == Tab::Decisions => {
                if let (Some(path), Some(decision)) =
                    (&self.db_path, self.decisions.get(self.decision_selected))
                {
                    if let Ok(mut db) = delta_core::db::Db::open(path) {
                        self.status = delta_services::append_review(
                            &mut db,
                            &decision.id,
                            "Retired from TUI",
                            Some("retired"),
                        )
                        .map(|_| "decision retired".into())
                        .unwrap_or_else(|e| e.to_string());
                        self.reload_journal();
                    }
                }
            }
            KeyCode::Down if self.tab == Tab::Ask => {
                self.ask_scroll = self.ask_scroll.saturating_add(1);
            }
            KeyCode::Up if self.tab == Tab::Ask => {
                self.ask_scroll = self.ask_scroll.saturating_sub(1);
            }
            KeyCode::Down if self.tab == Tab::Research => {
                self.research_scroll = self.research_scroll.saturating_add(1);
            }
            KeyCode::Up if self.tab == Tab::Research => {
                self.research_scroll = self.research_scroll.saturating_sub(1);
            }
            _ => {}
        }
        None
    }

    fn update(&mut self, action: Action) {
        match action {
            Action::SettingsDiagnostics(result) => self.settings_diagnostics = result,
            Action::ModelsReady(models) => {
                if self.palette_model {
                    if models.is_empty() {
                        self.palette_model = false;
                        self.form = Some(
                            Form::new(FormKind::Llm).with_defaults(
                                self.settings
                                    .as_ref()
                                    .map(|cfg| {
                                        vec![cfg.llm_provider.clone(), cfg.llm_model.clone()]
                                    })
                                    .unwrap_or_default(),
                            ),
                        );
                        self.status = "Catalog unavailable; enter a model id".into();
                    } else {
                        self.palette = Some(delta_tui::components::CommandPalette::new(models));
                        self.status = "Type to filter models · Enter select · Esc cancel".into();
                    }
                }
            }
            Action::ProviderConnected { name, verified } => {
                self.reload_settings();
                self.status = format!(
                    "{name}: {}",
                    if verified {
                        "connected"
                    } else {
                        "saved, key unverified"
                    }
                );
            }
            Action::Goto(name) => {
                let previous_tab = self.tab;
                self.tab = match name.to_lowercase().as_str() {
                    "home" => Tab::Home,
                    "watchlist" => Tab::Watchlist,
                    "research" => Tab::Research,
                    "theses" => Tab::Theses,
                    "ask" => Tab::Ask,
                    "decisions" => Tab::Decisions,
                    "settings" => Tab::Settings,
                    _ => return,
                };
                if self.tab != previous_tab {
                    self.journal_filter = JournalFilter::default();
                    self.framing_scroll = 0;
                    self.note_scroll = 0;
                    self.decision_scroll = 0;
                    self.decision_detail_open = false;
                    self.pending_delete = None;
                    if matches!(self.tab, Tab::Theses | Tab::Decisions) {
                        self.move_journal_selection(0);
                    }
                }
                if self.tab == Tab::Research {
                    self.reload_research();
                }
            }
            Action::Quit => self.quit = true,
            Action::Quotes(prices) => {
                let ids = self
                    .desk
                    .instruments
                    .iter()
                    .map(|item| item.instrument.id.as_str())
                    .collect::<std::collections::BTreeSet<_>>();
                self.desk.live.extend(
                    prices
                        .into_iter()
                        .filter(|(id, _)| ids.contains(id.as_str())),
                );
            }
            Action::AssetMetrics { range, metrics } => {
                if self
                    .desk
                    .instruments
                    .iter()
                    .any(|item| item.instrument.id == metrics.instrument_id)
                {
                    self.desk
                        .asset_metrics
                        .insert((metrics.instrument_id.clone(), range), metrics);
                }
            }
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
            Action::Gathered {
                counts,
                events,
                sentiment,
                warnings,
            } => {
                let stored: usize = counts.values().sum();
                self.status =
                    format!("gather: {stored} stored · {events} events · {sentiment} stances");
                if !warnings.is_empty() {
                    self.status.push_str(&format!(" · {}", warnings.join("; ")));
                }
                self.desk.last_ingest = Some(counts);
                if let delta_tui::desk::Source::Real(db) = &self.desk.source {
                    let db = db.clone();
                    self.desk.reload_from(&db);
                }
            }
            Action::Status(msg) => self.status = msg,
            Action::GatherBusy(busy) => self.gather_busy = busy,
            Action::AskBusy(busy) => self.ask_busy = busy,
            Action::ReportBusy(busy) => self.report_busy = busy,
            Action::HomeRefresh(feed) => self.desk.feed = feed,
            Action::FooterRefresh(state) => self.footer = state,
            Action::ReportReady {
                target_id,
                markdown,
            } => {
                self.reports.insert(target_id, markdown);
                self.research_scroll = 0;
                self.research.company_open = false;
                self.research.evidence_open = false;
            }
            Action::AskQuestion(text) => {
                self.ask_generation = self.ask_generation.wrapping_add(1);
                self.ask_history.push(delta_services::ChatMessage {
                    role: "user".to_string(),
                    text,
                    citations: Vec::new(),
                    source: "user".to_string(),
                });
                self.ask_busy = true;
                self.ask_scroll = 0;
            }
            Action::ChatFinished { generation, result } => {
                if generation == self.ask_generation && self.ask_busy {
                    match result {
                        Ok(answer) => self.update(Action::ChatReady(answer)),
                        Err(message) => self.update(Action::ChatFailed(message)),
                    }
                }
            }
            Action::ChatReady(answer) => {
                self.ask.citations = answer.citations.clone();
                self.ask.citation_selected = 0;
                self.ask.items.clear();
                if let Some(path) = &self.db_path {
                    if let Ok(db) = delta_core::db::Db::open(path) {
                        if let Ok(items) = delta_services::evidence_by_ids(&db, &self.ask.citations)
                        {
                            self.ask.items = items
                                .into_iter()
                                .map(|item| (item.id.clone(), item))
                                .collect();
                        }
                    }
                }
                self.ask_history.push(answer);
                self.ask_busy = false;
                self.status = "answer ready".to_string();
            }
            Action::ChatFailed(message) => {
                self.ask_busy = false;
                self.status = message;
            }
            Action::ThesisProposed(count) => {
                self.status = format!("{count} thesis evidence candidates");
                self.reload_thesis_links();
            }
            Action::ThesisSummaryReady(text) => {
                self.thesis_summary = Some(text);
                self.summary_scroll = 0;
                self.status = "thesis summary ready".into();
            }
            Action::GatherTargets(_)
            | Action::CancelGather
            | Action::CancelChat
            | Action::CancelReport
            | Action::RefreshMetrics(_)
            | Action::LoadDiagnostics
            | Action::LoadModels
            | Action::ConnectProvider(_)
            | Action::Noop
            | Action::OpenDialog(_)
            | Action::CloseDialog
            | Action::Gather
            | Action::GenerateReport(_)
            | Action::ProposeThesis(_) => {}
            Action::SummarizeThesis(_) => {}
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: ratatui::layout::Rect) {
        if area.width < 4 || area.height < 4 {
            return; // degenerate terminal; the painters assume a status bar
        }
        let mut screen = Screen::new(area.width as usize, area.height as usize);
        self.paint(&mut screen);
        if self.help_open {
            screen.fill(0, 0, screen.w, screen.h.saturating_sub(1), Style::DEFAULT);
            let lines = [
                "Delta keymap",
                "",
                "1 Home · 2 Watchlist · 3 Research · 4 Theses",
                "5 Ask · 6 Decisions · c Settings",
                "h Home · g Go · ? Help · q Quit",
                "",
                "Watchlist: a add · d remove · / filter · space fold",
                "↑↓ target · ←→ member · r/R range · i glossary",
                "Research: n report · e evidence · r report · / search",
                "k kind · l load more · ↑↓ scroll or select",
                "Theses: n new · d edit · t framing · e ledger · s summary",
                "j/k evidence · a accept · u unaccept · x reject",
                "Decisions: n new · e edit · d delete · r review · o research",
                "Ask: i type · Enter send · Esc cancel · ↑↓ scroll",
                "Settings: m model · s source · a market · x remove",
                "U gather all · Esc back",
            ];
            for (row, line) in lines.iter().enumerate().take(screen.h.saturating_sub(2)) {
                screen.text(2, row + 1, line, Style::fg(color::FG));
            }
        }
        blit(frame, &screen, area);
        if let Some(palette) = &mut self.palette {
            palette.draw(frame, area);
        }
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
            if cell.reverse {
                style = style.add_modifier(Modifier::REVERSED);
            }
            if cell.italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if cell.underline {
                style = style.add_modifier(Modifier::UNDERLINED);
            }
            frame
                .buffer_mut()
                .cell_mut((x as u16, y as u16))
                .expect("cell in bounds")
                .set_symbol(&cell.symbol)
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
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("delta {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if args.iter().any(|a| a == "--help" || a == "-h") {
        println!("delta [gather | report TARGET_ID | review-due [YYYY-MM-DD]]\nRun without a command to open the terminal app.");
        return Ok(());
    }
    if !args.is_empty() {
        let runtime = tokio::runtime::Runtime::new()?;
        return runtime.block_on(run_headless(&args));
    }
    let mut terminal = setup()?;
    let runtime = tokio::runtime::Runtime::new()?;
    let res = runtime.block_on(run(&mut terminal));
    teardown(&mut terminal)?;
    res
}

async fn run_headless(args: &[String]) -> std::io::Result<()> {
    let config_path = std::path::Path::new("config.toml");
    let (_, cfg) = delta_core::config::load_config(config_path)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    let mut db = delta_core::db::Db::open(std::path::Path::new(&cfg.db_path))
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    match args[0].as_str() {
        "gather" if args.len() == 1 => {
            let universe = delta_services::configured_universe(config_path)
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            let result = delta_services::gather_configured(&mut db, &cfg, &universe, |message| {
                println!("{message}")
            })
            .await
            .map_err(|e| std::io::Error::other(e.to_string()))?;
            println!(
                "{} rows stored; {} events extracted; {} stances classified",
                result.ingested.counts.values().sum::<usize>(),
                result.extracted.events,
                result.sentiment
            );
            for warning in result.warnings {
                eprintln!("warning: {warning}");
            }
        }
        "report" if args.len() == 2 => {
            let (_, path) = delta_services::generate_report_configured(&mut db, &cfg, &args[1])
                .await
                .map_err(|e| std::io::Error::other(e.to_string()))?;
            println!("{}", path.display());
        }
        "review-due" if args.len() <= 2 => {
            let date = if let Some(value) = args.get(1) {
                chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
                    .map_err(|e| std::io::Error::other(e.to_string()))?
            } else {
                chrono::Utc::now().date_naive()
            };
            for decision in delta_services::due_reviews(&db, date)
                .map_err(|e| std::io::Error::other(e.to_string()))?
            {
                println!(
                    "{}\t{}\t{}\t{}",
                    decision.id, decision.review_date, decision.instrument_id, decision.rationale
                );
            }
        }
        _ => {
            return Err(std::io::Error::other(
                "usage: delta [gather | report TARGET_ID | review-due [YYYY-MM-DD]]",
            ))
        }
    }
    Ok(())
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
        delta_tui::desk::Source::Unavailable(_) => {
            delta_core::config::load_config(std::path::Path::new("config.toml"))
                .ok()
                .map(|(_, cfg)| PathBuf::from(cfg.db_path))
        }
    };
    // Match the Python app's live feeds; allow explicit offline operation.
    let quotes_enabled = std::env::var("DELTA_QUOTES").as_deref() != Ok("0");
    let mut data_workers =
        workers::spawn(bus_tx.clone(), universe, db_path.clone(), quotes_enabled);
    let report_tx = workers::spawn_reports(bus_tx.clone(), db_path.clone());
    let diagnostics_tx = workers::spawn_settings(bus_tx.clone(), db_path.clone());
    let ask_tx = workers::spawn_chat(bus_tx.clone(), db_path.clone());
    let thesis_tx = workers::spawn_thesis_proposals(bus_tx.clone(), db_path.clone());
    let summary_tx = workers::spawn_thesis_summaries(bus_tx.clone(), db_path.clone());
    let provider_tx = workers::spawn_provider_setup(bus_tx.clone());
    let models_tx = workers::spawn_models(bus_tx.clone());

    let mut reports = BTreeMap::new();
    if let Ok((_, cfg)) = delta_core::config::load_config(std::path::Path::new("config.toml")) {
        for instrument in &desk.instruments {
            if let Some(stamp) = delta_services::latest_report(
                std::path::Path::new(&cfg.reports_dir),
                &instrument.instrument.id,
            ) {
                if let Ok(markdown) = std::fs::read_to_string(stamp.path) {
                    reports.insert(instrument.instrument.id.clone(), markdown);
                }
            }
        }
    }

    let mut app = App {
        desk,
        tab: Tab::Home,
        glossary: false,
        status: String::new(),
        reports,
        research_scroll: 0,
        research: delta_tui::research::ResearchBrowser::default(),
        watchlist: delta_tui::watchlist::WatchlistBrowser::default(),
        metrics_scroll: 0,
        palette: None,
        palette_model: false,
        help_open: false,
        footer: delta_tui::footer::FooterState::default(),
        settings_state: delta_tui::settings_view::SettingsState::default(),
        settings_diagnostics: Err("Diagnostics have not been loaded".into()),
        settings_sources: Vec::new(),
        provider_connected: false,
        viewport_width: 120,
        pending_action: None,
        ask_input: String::new(),
        ask_editing: false,
        gather_busy: false,
        report_busy: false,
        ask_busy: false,
        ask_generation: 0,
        ask_scroll: 0,
        ask_history: Vec::new(),
        ask: delta_tui::ask_view::AskState::default(),
        db_path,
        config_path: PathBuf::from("config.toml"),
        configuration_changed: false,
        theses: Vec::new(),
        decisions: Vec::new(),
        decision_reviews: Vec::new(),
        thesis_selected: 0,
        decision_selected: 0,
        decision_detail_open: false,
        decision_scroll: 0,
        journal_filter: JournalFilter::default(),
        pending_delete: None,
        form: None,
        thesis_links: Vec::new(),
        thesis_items: BTreeMap::new(),
        thesis_health: BTreeMap::new(),
        thesis_focus: delta_tui::theses_view::ThesisFocus::Claims,
        evidence_selected: 0,
        framing_scroll: 0,
        note_scroll: 0,
        thesis_summary: None,
        summary_scroll: 0,
        settings: None,

        frame_stats: FrameStats::default(),
        quit: false,
    };
    app.reload_journal();
    app.reload_settings();
    app.reload_watchlist_browser();
    if let Some(path) = &app.db_path {
        delta_core::state::write_last_seen(&path.to_string_lossy(), None);
    }

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
                                let _ = data_workers.gather.send(workers::WorkRequest::Run(Vec::new()));
                            } else if let Action::GatherTargets(ids) = action {
                                let _ = data_workers.gather.send(workers::WorkRequest::Run(ids));
                            } else if action == Action::CancelGather {
                                let _ = data_workers.gather.send(workers::WorkRequest::Cancel);
                            } else if action == Action::CancelReport {
                                let _ = report_tx.send(workers::WorkRequest::Cancel);
                            } else if let Action::GenerateReport(target_id) = action {
                                let _ = report_tx.send(workers::WorkRequest::Run(target_id));
                            } else if let Action::RefreshMetrics(id) = action {
                                if let Some(item) = app.desk.instruments.iter().find(|item| item.instrument.id == id) {
                                    let _ = data_workers.metrics.send(workers::MetricsRequest { instrument: item.instrument.clone(), range: app.desk.range().into() });
                                }
                            } else if action == Action::CancelChat {
                                let _ = ask_tx.send(workers::WorkRequest::Cancel);
                            } else if let Action::AskQuestion(question) = action {
                                app.update(Action::AskQuestion(question));
                                let _ = ask_tx.send(workers::WorkRequest::Run(workers::AskRequest {
                                    generation: app.ask_generation,
                                    history: app.ask_history.clone(),
                                    targets: app.ask.instrument_ids(),
                                }));
                            } else if let Action::ProposeThesis(thesis_id)=action {
                                let _=thesis_tx.send(thesis_id);
                            } else if let Action::LoadDiagnostics = action {
                                let _ = diagnostics_tx.send(());
                            } else if let Action::LoadModels = action {
                                let _ = models_tx.send(());
                            } else if let Action::ConnectProvider(setup) = action {
                                let _ = provider_tx.send(setup);
                            } else if let Action::SummarizeThesis(thesis_id)=action {
                                let _=summary_tx.send(thesis_id);
                            } else {
                                app.update(action);
                            }
                        }
                        if app.tab == Tab::Watchlist && matches!(key.code, KeyCode::Up | KeyCode::Down | KeyCode::Left | KeyCode::Right | KeyCode::Char('r' | 'R')) {
                            if let Some(item) = app.desk.instruments.get(app.desk.selected) {
                                if app.watchlist.instrument_id().is_some() {
                                    let _ = data_workers.metrics.send(workers::MetricsRequest { instrument: item.instrument.clone(), range: app.desk.range().into() });
                                }
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
        if app.configuration_changed {
            data_workers.refresh(
                app.desk
                    .instruments
                    .iter()
                    .map(|item| item.instrument.clone())
                    .collect(),
            );
            app.configuration_changed = false;
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
            reports: BTreeMap::new(),
            research_scroll: 0,
            research: delta_tui::research::ResearchBrowser::default(),
            watchlist: delta_tui::watchlist::WatchlistBrowser::default(),
            metrics_scroll: 0,
            palette: None,
            palette_model: false,
            help_open: false,
            footer: delta_tui::footer::FooterState::default(),
            settings_state: delta_tui::settings_view::SettingsState::default(),
            settings_diagnostics: Err("Diagnostics have not been loaded".into()),
            settings_sources: Vec::new(),
            provider_connected: false,
            viewport_width: 120,
            pending_action: None,
            ask_input: String::new(),
            ask_editing: false,
            gather_busy: false,
            report_busy: false,
            ask_busy: false,
            ask_generation: 0,
            ask_scroll: 0,
            ask_history: Vec::new(),
            ask: delta_tui::ask_view::AskState::default(),
            db_path: None,
            config_path: PathBuf::from("config.toml"),
            configuration_changed: false,
            theses: Vec::new(),
            decisions: Vec::new(),
            decision_reviews: Vec::new(),
            thesis_selected: 0,
            decision_selected: 0,
            decision_detail_open: false,
            decision_scroll: 0,
            journal_filter: JournalFilter::default(),
            pending_delete: None,
            form: None,
            thesis_links: Vec::new(),
            thesis_items: BTreeMap::new(),
            thesis_health: BTreeMap::new(),
            thesis_focus: delta_tui::theses_view::ThesisFocus::Claims,
            evidence_selected: 0,
            framing_scroll: 0,
            note_scroll: 0,
            thesis_summary: None,
            summary_scroll: 0,
            settings: None,

            frame_stats: FrameStats::default(),
            quit: false,
        }
    }

    #[test]
    fn forms_keep_edits_when_moving_back_and_after_validation_failure() {
        let mut a = app();
        a.form =
            Some(Form::new(FormKind::Llm).with_defaults(vec!["openai".into(), "model".into()]));
        a.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        a.form.as_mut().unwrap().input = "edited-model".into();
        a.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(a.form.as_ref().unwrap().input, "openai");
        a.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(a.form.as_ref().unwrap().input, "edited-model");
        let mut invalid = Form::new(FormKind::Llm);
        invalid.answers = vec!["invalid-provider".into(), "keep-this-model".into()];
        a.submit_form(invalid);
        assert_eq!(a.form.as_ref().unwrap().defaults[1], "keep-this-model");
        assert!(a.status.contains("settings:"));
    }

    #[test]
    fn unicode_text_keeps_wide_and_combining_symbols_in_terminal_cells() {
        let mut screen = Screen::new(8, 3);
        assert_eq!(screen.text(0, 0, "東京e\u{301}X", Style::fg(color::FG)), 6);
        assert_eq!(screen.cells[4].symbol, "e\u{301}");
        assert_eq!(screen.cells[5].ch, 'X');
        let mut terminal = Terminal::new(TestBackend::new(8, 3)).unwrap();
        terminal
            .draw(|frame| blit(frame, &screen, frame.area()))
            .unwrap();
        assert_eq!(terminal.backend().buffer()[(0, 0)].symbol(), "東");
        assert_eq!(terminal.backend().buffer()[(2, 0)].symbol(), "京");
        assert_eq!(terminal.backend().buffer()[(4, 0)].symbol(), "e\u{301}");
    }

    #[test]
    fn go_picker_navigates_and_cancelled_model_load_stays_closed() {
        let mut a = app();
        a.handle_key(KeyEvent::new(KeyCode::Char('g'), KeyModifiers::NONE));
        for ch in "dec".chars() {
            a.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        a.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(a.tab, Tab::Decisions);
        assert!(a.palette.is_none());
        assert_eq!(
            a.handle_key(KeyEvent::new(KeyCode::Char('m'), KeyModifiers::NONE)),
            Some(Action::LoadModels)
        );
        a.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        a.update(Action::ModelsReady(vec!["model".into()]));
        assert!(a.palette.is_none());
    }

    #[test]
    fn evidence_search_consumes_navigation_keys_and_escape_returns_to_report() {
        let mut a = app();
        a.tab = Tab::Research;
        a.handle_key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
        for ch in ['q', 'c', '3'] {
            assert!(a
                .handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE))
                .is_none());
        }
        assert_eq!(a.research.search, "qc3");
        assert_eq!(a.tab, Tab::Research);
        a.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!a.research.search_editing);
        a.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!a.research.evidence_open);
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
    fn journal_forms_persist_and_review_records() {
        let path = std::env::temp_dir().join(format!(
            "delta-journal-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        delta_core::db::Db::open(&path).unwrap();
        let mut a = app();
        a.db_path = Some(path.clone());
        let mut thesis = Form::new(FormKind::Thesis);
        thesis.answers = vec![
            "Growth continues".into(),
            "US".into(),
            "Demand grows".into(),
            "Revenue falls".into(),
            "US:AAPL".into(),
            "3 years".into(),
        ];
        a.submit_form(thesis);
        assert_eq!(a.theses.len(), 1);
        a.tab = Tab::Theses;
        assert_eq!(
            a.handle_key(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::NONE)),
            Some(Action::ProposeThesis(a.theses[0].id.clone()))
        );
        let mut decision = Form::new(FormKind::Decision);
        decision.answers = vec![
            "US:AAPL".into(),
            "Product growth".into(),
            "20x earnings".into(),
            "3 years".into(),
            "2026-10-01".into(),
            "Revenue declines".into(),
            a.theses[0].id.clone(),
        ];
        a.submit_form(decision);
        assert_eq!(a.decisions.len(), 1);
        let mut review = Form::new(FormKind::Review);
        review.answers = vec!["Still valid".into(), "reviewed".into()];
        a.submit_form(review);
        assert_eq!(a.decisions[0].status, "reviewed");
        assert_eq!(a.decision_reviews.len(), 1);
        let mut db = delta_core::db::Db::open(&path).unwrap();
        db.store_items(&[delta_core::db::StoreItem::News(
            delta_core::models::NewsItem {
                id: "journal-source".into(),
                instrument_ids: vec!["US:AAPL".into()],
                published: chrono::Utc::now().naive_utc(),
                title: "Launch evidence".into(),
                url: "https://example.test/launch".into(),
                body: Some("Product demand increased".into()),
                source: "rss".into(),
            },
        )])
        .unwrap();
        let mut link = Form::new(FormKind::Evidence);
        link.answers = vec![
            "news:journal-source".into(),
            "support".into(),
            "Tracks product demand".into(),
        ];
        a.submit_form(link);
        a.handle_key(KeyEvent::new(KeyCode::Char('e'), KeyModifiers::NONE));
        a.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(a.thesis_links[0].accepted);
        for (w, h) in [(80, 24), (120, 40), (200, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            a.tab = Tab::Theses;
            terminal.draw(|frame| a.draw(frame, frame.area())).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                text.contains("Launch evidence"),
                "source missing at {w}x{h}"
            );
            a.tab = Tab::Decisions;
            a.decision_detail_open = true;
            terminal.draw(|frame| a.draw(frame, frame.area())).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(
                text.contains("Product growth"),
                "rationale missing at {w}x{h}"
            );
        }
        a.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert_eq!(a.decisions.len(), 1);
        a.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        a.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert_eq!(a.decisions.len(), 1);
        a.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        a.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(a.decisions.is_empty());
        assert_eq!(db.table_count("decision_review").unwrap(), 0);
        a.tab = Tab::Ask;
        a.ask.targets = vec![delta_services::target_from_spec(
            "apple",
            &serde_json::json!({"market":"us", "tickers":["AAPL"]}),
            false,
        )
        .unwrap()];
        a.ask.toggle_all();
        a.update(Action::ChatReady(delta_services::ChatMessage {
            role: "assistant".into(),
            text: "Demand expands\n\nFurther explanation".into(),
            citations: vec![
                "news:journal-source".into(),
                "https://example.test/web".into(),
            ],
            source: "stored".into(),
        }));
        a.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(a.form.as_ref().unwrap().citations, ["news:journal-source"]);
        a.handle_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        let saved = a
            .theses
            .iter()
            .find(|thesis| thesis.claim == "Demand expands")
            .unwrap();
        let evidence = delta_services::thesis_evidence(&db, &saved.id, true).unwrap();
        assert_eq!(evidence.len(), 1);
        assert!(evidence[0].accepted);
        assert_eq!(saved.targets, ["US:AAPL"]);
        drop(db);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn watchlist_forms_support_first_target_and_confirmed_removal() {
        let dir = std::env::temp_dir().join(format!(
            "delta-watchlist-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&dir).unwrap();
        let config_path = dir.join("config.toml");
        let db_path = dir.join("test.db");
        std::fs::write(
            &config_path,
            format!(
                "db_path = {}\n",
                serde_json::to_string(&db_path.to_string_lossy()).unwrap()
            ),
        )
        .unwrap();
        let mut a = app();
        a.config_path = config_path.clone();
        let mut form = Form::new(FormKind::Target);
        form.answers = vec![
            "Apple".into(),
            "company".into(),
            "us".into(),
            "AAPL".into(),
            "equity".into(),
            "technology".into(),
            "Research".into(),
        ];
        a.submit_form(form);
        assert_eq!(a.desk.instruments.len(), 1);
        assert_eq!(a.desk.instruments[0].instrument.id, "US:AAPL");
        assert!(a.configuration_changed);
        let mut remove = Form::new(FormKind::RemoveTarget);
        remove.answers = vec!["Apple".into(), "no".into()];
        a.submit_form(remove);
        assert_eq!(a.desk.instruments.len(), 1);
        let mut remove = Form::new(FormKind::RemoveTarget);
        remove.answers = vec!["Apple".into(), "REMOVE".into()];
        a.submit_form(remove);
        assert!(a.desk.instruments.is_empty());
        for tab in [Tab::Settings, Tab::Theses, Tab::Decisions, Tab::Ask] {
            a.tab = tab;
            a.paint(&mut Screen::new(80, 24));
        }
        std::fs::remove_file(config_path).unwrap();
        std::fs::remove_file(db_path).unwrap();
        std::fs::remove_dir(dir).unwrap();
    }

    #[test]
    fn glossary_toggles_on_watchlist_and_esc_closes() {
        let mut a = app();
        a.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert!(a.glossary);
        a.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!a.glossary);
        // `i` on another pane does not open the glossary.
        a.handle_key(KeyEvent::new(KeyCode::Char('3'), KeyModifiers::NONE));
        a.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert!(!a.glossary);
    }

    #[test]
    fn range_cycling_walks_the_ranges() {
        let mut a = app();
        assert_eq!(a.desk.range(), "1m");
        a.handle_key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
        assert_eq!(a.desk.range(), "6m");
        a.handle_key(KeyEvent::new(KeyCode::Char('R'), KeyModifiers::NONE));
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
    fn ask_input_accepts_navigation_letters_and_submits_a_turn() {
        let mut a = app();
        a.tab = Tab::Ask;
        a.handle_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        assert!(a.ask_editing);
        for c in ['q', '1', '?'] {
            assert_eq!(
                a.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE)),
                None
            );
        }
        assert_eq!(a.ask_input, "q1?");
        let action = a.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(action, Some(Action::AskQuestion("q1?".to_string())));
        a.update(action.unwrap());
        assert_eq!(a.ask_history[0].text, "q1?");
        assert!(a.ask_busy);
    }

    #[test]
    fn cleared_chat_ignores_late_answers_and_errors() {
        let mut a = app();
        a.tab = Tab::Ask;
        a.update(Action::AskQuestion("first".into()));
        let old = a.ask_generation;
        a.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        a.handle_key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::NONE));
        assert!(!a.ask_busy);
        a.update(Action::AskQuestion("second".into()));
        a.update(Action::ChatFinished {
            generation: old,
            result: Err("old error".into()),
        });
        assert!(a.ask_busy);
        a.update(Action::ChatFinished {
            generation: old,
            result: Ok(delta_services::ChatMessage {
                role: "assistant".into(),
                text: "old answer".into(),
                citations: Vec::new(),
                source: "stored".into(),
            }),
        });
        assert_eq!(a.ask_history.len(), 1);
        assert_eq!(a.ask_history[0].text, "second");
        a.update(Action::ChatFinished {
            generation: a.ask_generation,
            result: Err("current error".into()),
        });
        assert!(!a.ask_busy);
        assert_eq!(a.status, "current error");
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
                let active = match tab {
                    Tab::Home => "1 Home",
                    Tab::Watchlist => "2 Watchlist",
                    Tab::Research => "3 Research",
                    Tab::Theses => "4 Theses",
                    Tab::Ask => "5 Ask",
                    Tab::Decisions => "6 Decisions",
                    Tab::Settings => "c Settings",
                };
                assert!(
                    status.contains(active),
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
