//! The app: desk data + active pane, painting through the golden `Screen`
//! model so the live frame is cell-for-cell what the goldens capture. Owns
//! the event loop, worker startup, the Python-matching bindings, the shell
//! modals (Go, help, palette) and the theme switch.

use std::io::Stdout;
use std::path::Path;
use std::time::{Duration, Instant};

use chrono::NaiveDate;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent};
use futures::StreamExt;
use ratatui::backend::CrosstermBackend;
use ratatui::{Frame, Terminal};
use tokio::sync::mpsc;

use delta_tui::components::CommandPalette;
use delta_tui::desk::Desk;
use delta_tui::dialog::{GoPicker, HelpDialog};
use delta_tui::form::Form;
use delta_tui::screen::{color, Screen, Style};
use delta_tui::screens::decisions::{draw_decisions_live, DecisionsData};
use delta_tui::screens::{
    draw_ask, draw_ask_narrow, draw_ask_wide, draw_decisions, draw_decisions_narrow,
    draw_decisions_wide, draw_glossary_overlay, draw_home, draw_home_narrow, draw_home_wide,
    draw_research, draw_research_narrow, draw_research_wide, draw_settings, draw_settings_narrow,
    draw_settings_wide, draw_theses, draw_theses_narrow, draw_theses_wide, draw_watchlist,
    draw_watchlist_narrow, draw_watchlist_wide, SettingsFocus, SettingsState, SettingsView,
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
    /// A Settings form modal (market add/edit, source setup).
    SettingsForm(FormModal),
    DecisionForm(DecisionModal),
    DecisionFilter(String),
    /// Provider choices, with key entry after a choice.
    ProviderPicker(usize),
    /// Names from the target service, shown on request from Settings.
    TargetPicker(Vec<String>, usize),
}

const PROVIDER_CHOICES: [&str; 4] = ["openrouter", "openai", "anthropic", "custom"];

/// One Settings form and what submitting it does.
struct FormModal {
    form: Form,
    mode: FormMode,
}

struct DecisionModal {
    form: Form,
    mode: DecisionMode,
}

enum DecisionMode {
    New,
    Edit(String),
    Review(String),
}

/// The services call a submitted Settings form performs.
#[derive(Clone, PartialEq)]
enum FormMode {
    /// `a`: `services.save_market` on a new id.
    AddMarket,
    /// `e`/enter: `services.save_market` on an existing id.
    EditMarket(String),
    /// `s`/enter: `services.configure_data_provider` for one source.
    Source(String),
    Provider(String),
    Model,
    AddTarget,
    EditTarget(String),
    RemoveTarget,
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
    /// The Settings screen's interaction state.
    settings: SettingsState,
    decisions: Option<DecisionsData>,
    confirm_decision_delete: bool,
    /// The terminal size the last frame painted (breakpoint decisions).
    viewport: (u16, u16),
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
            Tab::Decisions => {
                if let Some(data) = self
                    .decisions
                    .as_ref()
                    .filter(|data| !data.decisions.is_empty())
                {
                    draw_decisions_live(screen, data);
                } else {
                    route!(
                        draw_decisions_wide(screen),
                        draw_decisions(screen),
                        draw_decisions_narrow(screen)
                    );
                }
            }
            Tab::Settings => {
                let view = SettingsView {
                    data: &self.desk.settings,
                    state: &self.settings,
                    footer: &self.desk.settings_footer,
                };
                route!(
                    draw_settings_wide(screen, &view),
                    draw_settings(screen, &view),
                    draw_settings_narrow(screen, &view)
                )
            }
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
            Some(Overlay::SettingsForm(modal)) => {
                let width = delta_tui::dialog::MODAL_WIDTH.min(screen.w.saturating_sub(4));
                let height = (modal.form.height(width) + 5).min(screen.h.saturating_sub(2));
                let (x, y, w, _h) = delta_tui::dialog::dialog_frame(screen, width, height);
                screen.text(x, y, &modal.form.title, Style::fg(color::BLUE).bold());
                modal.form.draw_screen(screen, x, y + 2, w);
            }
            Some(Overlay::DecisionForm(modal)) => {
                if matches!(modal.mode, DecisionMode::Review(_)) {
                    delta_tui::screens::decisions::draw_decision_review_form(
                        screen,
                        modal.form.fields[0].value(),
                        modal.form.fields[1].value(),
                    );
                } else {
                    let width = delta_tui::dialog::MODAL_WIDTH.min(screen.w.saturating_sub(4));
                    let height = (modal.form.height(width) + 5).min(screen.h.saturating_sub(2));
                    let (x, y, w, _) = delta_tui::dialog::dialog_frame(screen, width, height);
                    screen.text(x, y, &modal.form.title, Style::fg(color::BLUE).bold());
                    modal.form.draw_screen(screen, x, y + 2, w);
                }
            }
            Some(Overlay::DecisionFilter(query)) => {
                let (x, y, w, _) = delta_tui::dialog::dialog_frame(screen, 54, 7);
                screen.text(x, y, "filter decisions", Style::fg(color::BLUE).bold());
                screen.text(x, y + 2, &format!("> {query}_"), Style::fg(color::FG));
                screen.text(
                    x,
                    y + 4,
                    "enter apply · esc cancel",
                    Style::fg(color::MUTED),
                );
                let _ = w;
            }
            Some(Overlay::ProviderPicker(selected)) => {
                let (x, y, w, _) = delta_tui::dialog::dialog_frame(screen, 58, 11);
                screen.text(x, y, "select a provider", Style::fg(color::BLUE).bold());
                screen.text(x, y + 2, "Provider", Style::fg(color::MUTED).bold());
                for (index, name) in PROVIDER_CHOICES.iter().enumerate() {
                    let style = if index == *selected {
                        Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
                    } else {
                        Style::fg(color::FG)
                    };
                    screen.fill(x, y + 3 + index, x + w, y + 4 + index, style);
                    screen.text(x + 1, y + 3 + index, name, style);
                }
                screen.text(
                    x,
                    y + 8,
                    "enter connect · esc cancel",
                    Style::fg(color::MUTED),
                );
            }
            Some(Overlay::TargetPicker(targets, selected)) => {
                let height = (targets.len() + 7).min(screen.h.saturating_sub(2));
                let (x, y, w, _) = delta_tui::dialog::dialog_frame(screen, 58, height);
                screen.text(x, y, "targets", Style::fg(color::BLUE).bold());
                if targets.is_empty() {
                    screen.text(x, y + 2, "no targets", Style::fg(color::MUTED));
                }
                for (index, name) in targets.iter().take(height.saturating_sub(6)).enumerate() {
                    let style = if index == *selected {
                        Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
                    } else {
                        Style::fg(color::FG)
                    };
                    screen.fill(x, y + 2 + index, x + w, y + 3 + index, style);
                    screen.text(x + 1, y + 2 + index, name, style);
                }
                screen.text(
                    x,
                    y + height - 3,
                    "enter edit · n add · x remove · esc close",
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
    fn decision_db(&self) -> Result<delta_core::db::Db, String> {
        let delta_tui::desk::Source::Real(path) = &self.desk.source else {
            return Err("decision journal needs a local database".to_string());
        };
        delta_core::db::Db::open(path).map_err(|error| error.to_string())
    }

    fn refresh_decisions(&mut self) {
        let Ok(db) = self.decision_db() else {
            self.decisions = None;
            return;
        };
        let filter = self
            .decisions
            .as_ref()
            .map(|data| data.filter.clone())
            .unwrap_or_default();
        let selected = self.decisions.as_ref().map_or(0, |data| data.selected);
        let Ok(mut decisions) = delta_services::list_decisions(&db, None, true) else {
            self.status = "could not load decisions".to_string();
            return;
        };
        if !filter.is_empty() {
            let needle = filter.to_lowercase();
            decisions.retain(|item| {
                item.instrument_id.to_lowercase().contains(&needle)
                    || item.rationale.to_lowercase().contains(&needle)
            });
        }
        let selected = selected.min(decisions.len().saturating_sub(1));
        let (reviews, current_price) = if let Some(decision) = decisions.get(selected) {
            let reviews = delta_services::review_history(&db, &decision.id).unwrap_or_default();
            let price = db
                .bars(&decision.instrument_id)
                .ok()
                .and_then(|bars| bars.last().map(|bar| bar.close));
            (reviews, price)
        } else {
            (Vec::new(), None)
        };
        self.decisions = Some(DecisionsData {
            decisions,
            selected,
            reviews,
            current_price,
            filter,
            spend: delta_services::analytics::total_spend(&db, None),
            confirm_delete: self.confirm_decision_delete,
        });
    }

    fn selected_decision(&self) -> Option<&delta_services::Decision> {
        self.decisions.as_ref()?.selected()
    }

    fn handle_decisions_key(&mut self, key: KeyEvent) -> Option<Action> {
        match key.code {
            KeyCode::Up | KeyCode::Down => {
                if let Some(data) = &mut self.decisions {
                    if key.code == KeyCode::Up {
                        data.selected = data.selected.saturating_sub(1);
                    } else {
                        data.selected =
                            (data.selected + 1).min(data.decisions.len().saturating_sub(1));
                    }
                }
                self.refresh_decisions();
            }
            KeyCode::Char('n') => {
                self.overlay = Some(Overlay::DecisionForm(decision_form(None)));
            }
            KeyCode::Char('e') => {
                if let Some(decision) = self.selected_decision() {
                    self.overlay = Some(Overlay::DecisionForm(decision_form(Some(decision))));
                }
            }
            KeyCode::Char('r') => {
                if let Some(decision) = self.selected_decision() {
                    self.overlay = Some(Overlay::DecisionForm(review_form(&decision.id)));
                }
            }
            KeyCode::Char('/') => {
                let query = self
                    .decisions
                    .as_ref()
                    .map_or(String::new(), |d| d.filter.clone());
                self.overlay = Some(Overlay::DecisionFilter(query));
            }
            KeyCode::Char('d') => {
                if self.selected_decision().is_some() {
                    self.confirm_decision_delete = true;
                    if let Some(data) = &mut self.decisions {
                        data.confirm_delete = true;
                    }
                    self.status = "delete decision? press y to confirm".to_string();
                }
            }
            KeyCode::Char('y') if self.confirm_decision_delete => {
                self.confirm_decision_delete = false;
                if let Some(id) = self.selected_decision().map(|d| d.id.clone()) {
                    let result = self.decision_db().and_then(|db| {
                        delta_services::delete_decision(&db, &id).map_err(|error| error.to_string())
                    });
                    self.status = match result {
                        Ok(()) => "decision deleted".to_string(),
                        Err(error) => error,
                    };
                    self.refresh_decisions();
                }
            }
            KeyCode::Char('o') => {
                if let Some(decision) = self.selected_decision() {
                    self.status = format!("research for {}", decision.instrument_id);
                    self.tab = Tab::Research;
                }
            }
            KeyCode::Esc => {
                self.confirm_decision_delete = false;
                if let Some(data) = &mut self.decisions {
                    data.confirm_delete = false;
                }
                self.status.clear();
            }
            _ => return None,
        }
        Some(Action::Noop)
    }

    fn submit_decision_form(&mut self, modal: &mut DecisionModal) -> bool {
        let values = match modal.form.submit() {
            Ok(values) => values
                .into_iter()
                .map(|(_, value)| value)
                .collect::<Vec<_>>(),
            Err(errors) => {
                self.status = errors[0].1.clone();
                return false;
            }
        };
        let db = match self.decision_db() {
            Ok(db) => db,
            Err(error) => {
                self.status = error;
                return false;
            }
        };
        let result = match &modal.mode {
            DecisionMode::New | DecisionMode::Edit(_) => {
                let date = match NaiveDate::parse_from_str(&values[4], "%Y-%m-%d") {
                    Ok(date) => date,
                    Err(_) => {
                        self.status = "review date must be YYYY-MM-DD".to_string();
                        return false;
                    }
                };
                let thesis = (!values[6].trim().is_empty()).then_some(values[6].as_str());
                match &modal.mode {
                    DecisionMode::New => delta_services::create_decision(
                        &db, &values[0], &values[1], &values[2], &values[3], date, &values[5],
                        thesis, None,
                    )
                    .map(|_| ()),
                    DecisionMode::Edit(id) => delta_services::update_decision(
                        &db, id, &values[0], &values[1], &values[2], &values[3], date, &values[5],
                        thesis,
                    )
                    .map(|_| ()),
                    DecisionMode::Review(_) => unreachable!(),
                }
            }
            DecisionMode::Review(id) => {
                let status = values[1].trim().to_lowercase();
                delta_services::append_review(&db, id, &values[0], Some(&status), None).map(|_| ())
            }
        };
        match result {
            Ok(()) => {
                self.status = "decision saved".to_string();
                self.refresh_decisions();
                true
            }
            Err(error) => {
                self.status = error.to_string();
                false
            }
        }
    }

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
            KeyCode::Char('6') => {
                self.tab = Tab::Decisions;
                self.refresh_decisions();
            }
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

impl App {
    /// The Settings screen's keys (`config.py::BINDINGS`): `d` diagnostics,
    /// `r` refresh, `l` plugins, `s` configure source, `a`/`e`/`x`
    /// add/edit/remove market, `esc` back; plus the table keys (↑↓/tab/
    /// enter) and `t` to toggle a plugin.
    fn handle_settings_key(&mut self, key: KeyEvent) -> Option<Action> {
        let data = &self.desk.settings;
        let counts = (data.plugins.len(), data.sources.len(), data.markets.len());
        match key.code {
            KeyCode::Esc => {
                self.settings.close_diagnostics(self.viewport_width());
                Some(Action::Noop)
            }
            KeyCode::Up => {
                self.settings
                    .move_selection(-1, counts.0, counts.1, counts.2);
                Some(Action::Noop)
            }
            KeyCode::Down => {
                self.settings
                    .move_selection(1, counts.0, counts.1, counts.2);
                Some(Action::Noop)
            }
            KeyCode::Tab => {
                self.settings.cycle_focus(false);
                Some(Action::Noop)
            }
            KeyCode::BackTab => {
                self.settings.cycle_focus(true);
                Some(Action::Noop)
            }
            KeyCode::Enter => Some(self.settings_enter()),
            KeyCode::Char('d') => {
                self.settings.toggle_diagnostics(self.viewport_width());
                Some(Action::Noop)
            }
            KeyCode::Char('r') => {
                self.desk.reload_settings();
                let refreshed = self
                    .desk
                    .settings
                    .diagnostics
                    .as_ref()
                    .map(|d| d.refreshed.clone())
                    .unwrap_or_default();
                self.status = format!("diagnostics refreshed {refreshed}");
                Some(Action::Noop)
            }
            KeyCode::Char('l') => {
                self.settings.focus = SettingsFocus::Plugins;
                Some(Action::Noop)
            }
            KeyCode::Char('s') => Some(self.open_source_form()),
            KeyCode::Char('a') => {
                self.overlay = Some(Overlay::SettingsForm(market_form(None)));
                Some(Action::Noop)
            }
            KeyCode::Char('e') => Some(self.edit_market()),
            KeyCode::Char('x') => Some(self.remove_market()),
            KeyCode::Char('t') => Some(self.toggle_plugin()),
            KeyCode::Char('n') => {
                self.overlay = Some(Overlay::SettingsForm(target_form(None)));
                Some(Action::Noop)
            }
            KeyCode::Char('X') => {
                self.overlay = Some(Overlay::SettingsForm(remove_target_form()));
                Some(Action::Noop)
            }
            KeyCode::Char('w') => Some(self.open_target_picker()),
            _ => None,
        }
    }

    /// `enter` on the focused table's row (`on_data_table_row_selected`).
    fn settings_enter(&mut self) -> Action {
        match self.settings.focus {
            SettingsFocus::Provider => {
                if self.settings.provider_selected == 0 {
                    Action::ShowProviderPicker
                } else {
                    Action::ShowModelPicker
                }
            }
            SettingsFocus::Plugins => {
                let data = &self.desk.settings;
                match data.plugins.get(self.settings.plugin_selected) {
                    Some((id, enabled)) => Action::Status(format!(
                        "{id}: {} · t toggle",
                        if *enabled { "enabled" } else { "disabled" }
                    )),
                    None => Action::Noop,
                }
            }
            SettingsFocus::Sources => self.open_source_form(),
            SettingsFocus::Markets => self.edit_market(),
            SettingsFocus::Diagnostics => Action::Noop,
        }
    }

    /// `s`/enter on a source: the setup form for its provider fields.
    fn open_source_form(&mut self) -> Action {
        let Some(id) = self
            .desk
            .settings
            .sources
            .get(self.settings.source_selected)
            .map(|s| s.id.clone())
        else {
            return Action::Status("no data source selected".to_string());
        };
        let current = self
            .desk
            .settings
            .sources
            .get(self.settings.source_selected)
            .map(|source| source.contact.clone());
        self.overlay = Some(Overlay::SettingsForm(source_form(&id, current)));
        Action::Noop
    }

    /// `e`/enter on a market: the prefilled edit form.
    fn edit_market(&mut self) -> Action {
        let Some(market) = self
            .desk
            .settings
            .markets
            .get(self.settings.market_selected)
            .cloned()
        else {
            return Action::Status("no market selected".to_string());
        };
        self.overlay = Some(Overlay::SettingsForm(market_form(Some(&market))));
        Action::Noop
    }

    /// `x`: remove the selected market through the services layer.
    fn remove_market(&mut self) -> Action {
        let Some(id) = self
            .settings
            .selected_market_id(&self.desk.settings)
            .map(str::to_string)
        else {
            return Action::Status("no market selected".to_string());
        };
        let Some(config) = self.desk.config_path.clone() else {
            return Action::Status("offline: no config.toml".to_string());
        };
        match delta_services::config_ops::remove_market(&config, &id) {
            Ok(()) => {
                self.desk.reload_settings();
                let data = &self.desk.settings;
                self.settings
                    .reconcile(data.plugins.len(), data.sources.len(), data.markets.len());
                Action::Status(format!("market {id} removed"))
            }
            Err(e) => Action::Status(e.to_string()),
        }
    }

    /// `t`: flip the selected plugin's enabled flag in `config.toml`.
    fn toggle_plugin(&mut self) -> Action {
        let Some(id) = self
            .settings
            .selected_plugin_id(&self.desk.settings)
            .map(str::to_string)
        else {
            return Action::Status("no plugin selected".to_string());
        };
        let enabled = self
            .desk
            .settings
            .plugins
            .get(self.settings.plugin_selected)
            .map(|(_, on)| *on)
            .unwrap_or(true);
        let Some(config) = self.desk.config_path.clone() else {
            return Action::Status("offline: no config.toml".to_string());
        };
        match delta_services::config_ops::set_plugin_enabled(&config, &id, !enabled) {
            Ok(()) => {
                self.desk.reload_settings();
                Action::Status(format!(
                    "{id} {}",
                    if !enabled { "enabled" } else { "disabled" }
                ))
            }
            Err(e) => Action::Status(e.to_string()),
        }
    }

    /// Run the form's services call on submit; returns the action and
    /// whether the form should close (a rejected submit stays open).
    fn submit_settings_form(&mut self, modal: &mut FormModal) -> (Action, bool) {
        let values: std::collections::BTreeMap<String, String> = match modal.form.submit() {
            Ok(values) => values.into_iter().collect(),
            Err(errors) => {
                let message = errors
                    .first()
                    .map(|(_, m)| m.clone())
                    .unwrap_or_else(|| "invalid input".to_string());
                return (Action::Status(message), false);
            }
        };
        let Some(config) = self.desk.config_path.clone() else {
            return (Action::Status("offline: no config.toml".to_string()), false);
        };
        let result = match &modal.mode {
            FormMode::AddMarket | FormMode::EditMarket(_) => {
                let id = values.get("ID").cloned().unwrap_or_default();
                delta_services::config_ops::save_market(
                    &config,
                    &id,
                    values.get("Name").map(String::as_str).unwrap_or(""),
                    values.get("Currency").map(String::as_str).unwrap_or(""),
                    values.get("Yahoo suffix").map(String::as_str).unwrap_or(""),
                )
            }
            FormMode::Source(id) => {
                let mut fields = std::collections::BTreeMap::new();
                if let Some(value) = values.get("Contact email") {
                    fields.insert("contact".to_string(), value.clone());
                }
                delta_services::setup::configure_data_provider(
                    &config,
                    &self.env_path(),
                    &delta_plugins::provider_specs(),
                    id,
                    &fields,
                    None,
                )
            }
            FormMode::Provider(name) => delta_services::setup::save_provider_choice(
                &config,
                &self.env_path(),
                name,
                values.get("API key").map(String::as_str).unwrap_or(""),
                values.get("Base URL").map(String::as_str).unwrap_or(""),
                values
                    .get("Key environment variable")
                    .map(String::as_str)
                    .unwrap_or(""),
            ),
            FormMode::Model => delta_services::setup::save_model_choice(
                &config,
                values.get("Model ID").map(String::as_str).unwrap_or(""),
            ),
            FormMode::AddTarget => {
                let symbols = values
                    .get("Instruments")
                    .map(|s| {
                        s.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                delta_services::config_ops::add_target(
                    &config,
                    values.get("Name").map(String::as_str).unwrap_or(""),
                    values.get("Kind").map(String::as_str).unwrap_or("company"),
                    values.get("Market").map(String::as_str).unwrap_or("us"),
                    &symbols,
                    &[],
                    "",
                    None,
                    "equity",
                )
            }
            FormMode::EditTarget(name) => {
                if values.get("Name").is_none_or(|value| value != name) {
                    return (
                        Action::Status("target ID cannot be changed".to_string()),
                        false,
                    );
                }
                let symbols = values
                    .get("Instruments")
                    .map(|s| {
                        s.split(',')
                            .map(str::trim)
                            .filter(|s| !s.is_empty())
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                delta_services::config_ops::update_target(
                    &config,
                    name,
                    values.get("Kind").map(String::as_str).unwrap_or("company"),
                    values.get("Market").map(String::as_str).unwrap_or("us"),
                    &symbols,
                )
            }
            FormMode::RemoveTarget => delta_services::config_ops::remove_target(
                &config,
                values.get("Name").map(String::as_str).unwrap_or(""),
            ),
        };
        match result {
            Ok(()) => {
                let message = match &modal.mode {
                    FormMode::AddMarket | FormMode::EditMarket(_) => "market saved".to_string(),
                    FormMode::Source(_) => "source saved".to_string(),
                    FormMode::Provider(name) => format!("{name} selected"),
                    FormMode::Model => "model saved".to_string(),
                    FormMode::AddTarget => "target added".to_string(),
                    FormMode::EditTarget(_) => "target saved".to_string(),
                    FormMode::RemoveTarget => "target removed".to_string(),
                };
                self.desk.reload_settings();
                let data = &self.desk.settings;
                self.settings
                    .reconcile(data.plugins.len(), data.sources.len(), data.markets.len());
                (Action::Status(message), true)
            }
            Err(e) => (Action::Status(e.to_string()), false),
        }
    }

    fn viewport_width(&self) -> usize {
        self.viewport.0 as usize
    }

    fn env_path(&self) -> std::path::PathBuf {
        self.desk
            .config_path
            .as_ref()
            .and_then(|path| path.parent())
            .unwrap_or_else(|| std::path::Path::new("."))
            .join(".env")
    }

    fn open_target_picker(&mut self) -> Action {
        let Some(config) = &self.desk.config_path else {
            return Action::Status("offline: no config.toml".to_string());
        };
        match delta_services::config_ops::target_specs(config) {
            Ok(targets) => {
                self.overlay = Some(Overlay::TargetPicker(targets.into_keys().collect(), 0));
                Action::Noop
            }
            Err(e) => Action::Status(e.to_string()),
        }
    }
}

fn provider_form(name: &str) -> FormModal {
    let fields = if name == "custom" {
        vec![
            delta_tui::form::Field::new(
                "Base URL",
                "http://localhost:11434/v1",
                Some(delta_tui::form::required),
            ),
            delta_tui::form::Field::new("Key environment variable", "CUSTOM_API_KEY", None),
            delta_tui::form::Field::new("API key", "optional for local servers", None).secret(),
        ]
    } else {
        vec![delta_tui::form::Field::new("API key", "leave blank to use existing", None).secret()]
    };
    FormModal {
        form: Form::new(&format!("connect {name}"), fields),
        mode: FormMode::Provider(name.to_string()),
    }
}

fn model_form(current: &str) -> FormModal {
    let mut form = Form::new(
        "select a model",
        vec![delta_tui::form::Field::new(
            "Model ID",
            "provider/model-id",
            Some(delta_tui::form::required),
        )],
    );
    form.fields[0].input.set_value(current);
    FormModal {
        form,
        mode: FormMode::Model,
    }
}

fn target_form(current: Option<&delta_services::targets::WatchTarget>) -> FormModal {
    let mut form = Form::new(
        if current.is_some() {
            "edit target"
        } else {
            "add target"
        },
        vec![
            delta_tui::form::Field::new("Name", "e.g. apple", Some(delta_tui::form::required)),
            delta_tui::form::Field::new(
                "Kind",
                "company, theme, market…",
                Some(delta_tui::form::required),
            ),
            delta_tui::form::Field::new("Market", "us, asx…", Some(delta_tui::form::required)),
            delta_tui::form::Field::new("Instruments", "AAPL,MSFT", None),
        ],
    );
    let mut mode = FormMode::AddTarget;
    if let Some(target) = current {
        mode = FormMode::EditTarget(target.id.clone());
        for (field, value) in form.fields.iter_mut().zip([
            target.id.clone(),
            target.kind.clone(),
            target.markets.first().cloned().unwrap_or_default(),
            target.tickers.join(","),
        ]) {
            field.input.set_value(&value);
        }
    }
    FormModal { form, mode }
}

fn remove_target_form() -> FormModal {
    FormModal {
        form: Form::new(
            "remove target",
            vec![delta_tui::form::Field::new(
                "Name",
                "target id",
                Some(delta_tui::form::required),
            )],
        ),
        mode: FormMode::RemoveTarget,
    }
}

/// The market form (`market_setup.py::MarketSetupModal`): ID, name,
/// currency, Yahoo suffix.
fn market_form(current: Option<&delta_tui::screens::SettingsMarket>) -> FormModal {
    let mut form = Form::new(
        if current.is_some() {
            "edit market"
        } else {
            "add market"
        },
        vec![
            delta_tui::form::Field::new("ID", "e.g. lse", Some(delta_tui::form::required)),
            delta_tui::form::Field::new("Name", "Exchange name", Some(delta_tui::form::required)),
            delta_tui::form::Field::new("Currency", "GBP", Some(delta_tui::form::required)),
            delta_tui::form::Field::new("Yahoo suffix", ".L", None),
        ],
    );
    let mut mode = FormMode::AddMarket;
    if let Some(market) = current {
        mode = FormMode::EditMarket(market.id.clone());
        let prefill = [
            ("ID", market.id.as_str()),
            ("Name", market.label.as_str()),
            ("Currency", market.currency.as_str()),
            ("Yahoo suffix", market.yahoo_suffix.as_str()),
        ];
        for field in form.fields.iter_mut() {
            if let Some((_, value)) = prefill.iter().find(|(label, _)| *label == field.label) {
                field.input.set_value(value);
            }
        }
    }
    FormModal { form, mode }
}

/// The source setup form (`source_setup.py`): the provider's non-secret
/// fields; secrets stay in `configure_data_provider`.
fn source_form(id: &str, current: Option<String>) -> FormModal {
    let mut form = Form::new(
        "configure source",
        vec![delta_tui::form::Field::new(
            "Contact email",
            "you@example.com",
            Some(delta_tui::form::required),
        )],
    );
    if let Some(current) = current {
        form.fields[0].input.set_value(&current);
    }
    FormModal {
        form,
        mode: FormMode::Source(id.to_string()),
    }
}

fn decision_form(current: Option<&delta_services::Decision>) -> DecisionModal {
    use delta_tui::form::{required, Field};
    let fields = vec![
        Field::new("Instrument", "US:AAPL", Some(required)),
        Field::new("Rationale", "Why this decision?", Some(required)),
        Field::new("Valuation context", "Price and assumptions", Some(required)),
        Field::new("Time horizon", "e.g. 6m", Some(required)),
        Field::new("Review date", "YYYY-MM-DD", Some(required)),
        Field::new(
            "Invalidation criteria",
            "What would change your mind?",
            Some(required),
        ),
        Field::new("Thesis ID", "optional", None),
    ];
    let mut form = Form::new(
        if current.is_some() {
            "edit decision"
        } else {
            "new decision"
        },
        fields,
    );
    let mode = if let Some(decision) = current {
        for (field, value) in form.fields.iter_mut().zip([
            decision.instrument_id.as_str(),
            decision.rationale.as_str(),
            decision.valuation_context.as_str(),
            decision.time_horizon.as_str(),
            &decision.review_date.to_string(),
            decision.invalidation_criteria.as_str(),
            decision.thesis_id.as_deref().unwrap_or(""),
        ]) {
            field.input.set_value(value);
        }
        DecisionMode::Edit(decision.id.clone())
    } else {
        DecisionMode::New
    };
    DecisionModal { form, mode }
}

fn review_form(id: &str) -> DecisionModal {
    use delta_tui::form::{required, Field};
    let mut form = Form::new(
        "review decision",
        vec![
            Field::new("Review note", "What changed?", Some(required)),
            Field::new("Status", "open, reviewed, or retired", Some(required)),
        ],
    );
    form.fields[1].input.set_value("reviewed");
    DecisionModal {
        form,
        mode: DecisionMode::Review(id.to_string()),
    }
}

impl Component for App {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        if let Some(Overlay::ProviderPicker(selected)) = &mut self.overlay {
            match key.code {
                KeyCode::Esc => self.overlay = None,
                KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Down => *selected = (*selected + 1).min(PROVIDER_CHOICES.len() - 1),
                KeyCode::Enter => {
                    let name = PROVIDER_CHOICES[*selected];
                    self.overlay = Some(Overlay::SettingsForm(provider_form(name)));
                }
                _ => {}
            }
            return None;
        }
        if let Some(Overlay::TargetPicker(targets, selected)) = &mut self.overlay {
            match key.code {
                KeyCode::Esc => self.overlay = None,
                KeyCode::Up => *selected = selected.saturating_sub(1),
                KeyCode::Down => *selected = (*selected + 1).min(targets.len().saturating_sub(1)),
                KeyCode::Char('n') => self.overlay = Some(Overlay::SettingsForm(target_form(None))),
                KeyCode::Enter => {
                    if let (Some(name), Some(config)) =
                        (targets.get(*selected), &self.desk.config_path)
                    {
                        if let Ok(specs) = delta_services::config_ops::target_specs(config) {
                            if let Some(target) = specs.get(name) {
                                self.overlay =
                                    Some(Overlay::SettingsForm(target_form(Some(target))));
                            }
                        }
                    }
                }
                KeyCode::Char('x') => {
                    if let Some(name) = targets.get(*selected) {
                        let mut modal = remove_target_form();
                        modal.form.fields[0].input.set_value(name);
                        self.overlay = Some(Overlay::SettingsForm(modal));
                    }
                }
                _ => {}
            }
            return None;
        }
        if matches!(self.overlay, Some(Overlay::DecisionForm(_)))
            && (key.code == KeyCode::Enter
                || (key.code == KeyCode::Char('s')
                    && key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL)))
        {
            let Some(Overlay::DecisionForm(mut modal)) = self.overlay.take() else {
                unreachable!("checked above");
            };
            if !self.submit_decision_form(&mut modal) {
                self.overlay = Some(Overlay::DecisionForm(modal));
            }
            return Some(Action::Noop);
        }
        if let Some(Overlay::DecisionFilter(query)) = &mut self.overlay {
            match key.code {
                KeyCode::Esc => self.overlay = None,
                KeyCode::Enter => {
                    let filter = query.trim().to_string();
                    self.overlay = None;
                    if let Some(data) = &mut self.decisions {
                        data.filter = filter;
                        data.selected = 0;
                    }
                    self.refresh_decisions();
                }
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Char(ch)
                    if !key
                        .modifiers
                        .contains(crossterm::event::KeyModifiers::CONTROL) =>
                {
                    query.push(ch);
                }
                _ => {}
            }
            return Some(Action::Noop);
        }
        // The Settings form manages its own submit cycle: a rejected
        // submit keeps the modal open, the message on the status line.
        if key.code == KeyCode::Enter && matches!(self.overlay, Some(Overlay::SettingsForm(_))) {
            let Some(Overlay::SettingsForm(mut modal)) = self.overlay.take() else {
                unreachable!("checked the variant above");
            };
            let (action, saved) = self.submit_settings_form(&mut modal);
            if !saved {
                self.overlay = Some(Overlay::SettingsForm(modal));
            }
            self.update(action);
            return None;
        }
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
                Overlay::SettingsForm(modal) => match key.code {
                    KeyCode::Esc => Some(Action::CloseDialog),
                    _ => modal.form.handle_key(key),
                },
                Overlay::DecisionForm(modal) => match key.code {
                    KeyCode::Esc => Some(Action::CloseDialog),
                    _ => modal.form.handle_key(key),
                },
                Overlay::DecisionFilter(_) => unreachable!("handled above"),
                Overlay::ProviderPicker(_) => unreachable!("handled above"),
                Overlay::TargetPicker(_, _) => unreachable!("handled above"),
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
        if self.tab == Tab::Settings && key.code == KeyCode::Esc {
            return self.handle_settings_key(key);
        }
        if self.tab == Tab::Decisions {
            if let Some(action) = self.handle_decisions_key(key) {
                return Some(action);
            }
        }
        if let Some(action) = self.handle_shell_key(key) {
            return Some(action);
        }
        if self.tab == Tab::Settings {
            if let Some(action) = self.handle_settings_key(key) {
                return Some(action);
            }
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
                self.overlay = Some(Overlay::SettingsForm(model_form(&self.desk.settings.model)));
            }
            Action::ShowProviderPicker => {
                let selected = PROVIDER_CHOICES
                    .iter()
                    .position(|name| *name == self.desk.settings.provider)
                    .unwrap_or(0);
                self.overlay = Some(Overlay::ProviderPicker(selected));
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
        self.viewport = (area.width, area.height);
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
        settings: SettingsState::default(),
        decisions: None,
        confirm_decision_delete: false,
        viewport: (120, 40),
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
            settings: SettingsState::default(),
            decisions: None,
            confirm_decision_delete: false,
            viewport: (120, 40),
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
        assert!(matches!(a.overlay, Some(Overlay::SettingsForm(_))));
        a.handle_key(key(KeyCode::Esc));
        assert_eq!(
            a.handle_key(key(KeyCode::Char('p'))),
            Some(Action::ShowProviderPicker)
        );
        a.update(Action::ShowProviderPicker);
        assert!(matches!(a.overlay, Some(Overlay::ProviderPicker(_))));
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

#[cfg(test)]
mod settings_tests {
    use super::tests::app;
    use super::*;
    use delta_tui::screens::SettingsState;
    use ratatui::backend::TestBackend;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, crossterm::event::KeyModifiers::NONE)
    }

    fn settings_app() -> App {
        let mut a = app();
        a.handle_key(key(KeyCode::Char('c')));
        a
    }

    fn screen_text(a: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| a.draw(f, f.area())).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn settings_tab_paints_live_data_at_all_sizes() {
        let mut a = settings_app();
        for &(w, h) in &[(80u16, 24u16), (120, 40), (200, 50)] {
            let text = screen_text(&mut a, w, h);
            for probe in [
                "provider: ● openrouter",
                "model: ",
                "sec_edgar  enabled",
                "diagnostics",
            ] {
                assert!(text.contains(probe), "missing {probe:?} at {w}x{h}");
            }
            if w >= 100 {
                assert!(text.contains("delta.db · 4 KB"), "db size at {w}x{h}");
                assert!(text.contains("latest bar US:AAPL 20 Sep 00:00 UTC"));
            } else {
                assert!(
                    text.contains("80 rows · latest bar 20 Sep 00:00 UTC · spend $0.00"),
                    "folded summary at {w}x{h}"
                );
            }
        }
    }

    #[test]
    fn diagnostics_toggle_follows_the_breakpoint_and_pins() {
        let mut a = settings_app();
        a.viewport = (80, 24); // narrow: folded by default
        a.handle_key(key(KeyCode::Char('d')));
        assert_eq!(a.settings.focus, SettingsFocus::Diagnostics);
        assert!(a.settings.diagnostics_expanded(80));
        let text = screen_text(&mut a, 80, 24);
        assert!(
            text.contains("esc"),
            "the expanded narrow pane offers esc to close"
        );
        a.handle_key(key(KeyCode::Esc));
        assert!(!a.settings.diagnostics_expanded(80));
        assert_eq!(a.settings.focus, SettingsFocus::Provider);
        // Wide stays expanded and esc leaves it open.
        a.viewport = (120, 40);
        a.handle_key(key(KeyCode::Char('d')));
        assert!(a.settings.diagnostics_expanded(120));
        a.handle_key(key(KeyCode::Esc));
        assert!(a.settings.diagnostics_expanded(120));
    }

    #[test]
    fn focus_keys_and_selections_move() {
        let mut a = settings_app();
        a.handle_key(key(KeyCode::Char('l')));
        assert_eq!(a.settings.focus, SettingsFocus::Plugins);
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.settings.focus, SettingsFocus::Sources);
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.settings.focus, SettingsFocus::Markets);
        a.handle_key(key(KeyCode::BackTab));
        assert_eq!(a.settings.focus, SettingsFocus::Sources);
        // The provider table has two rows; the model row is the second.
        a.handle_key(key(KeyCode::Tab));
        a.handle_key(key(KeyCode::Tab));
        a.handle_key(key(KeyCode::Tab));
        assert_eq!(a.settings.focus, SettingsFocus::Provider);
        a.handle_key(key(KeyCode::Down));
        assert_eq!(a.settings.provider_selected, 1);
        // Enter on the model row opens the model picker action.
        assert_eq!(
            a.handle_key(key(KeyCode::Enter)),
            Some(Action::ShowModelPicker)
        );
        a.handle_key(key(KeyCode::Up));
        assert_eq!(
            a.handle_key(key(KeyCode::Enter)),
            Some(Action::ShowProviderPicker)
        );
    }

    #[test]
    fn plugin_detail_and_refresh_status() {
        let mut a = settings_app();
        a.handle_key(key(KeyCode::Char('l')));
        let action = a.handle_key(key(KeyCode::Enter)).expect("action");
        match action {
            Action::Status(msg) => {
                assert!(msg.starts_with("sec_edgar: enabled"), "{msg}");
            }
            other => panic!("expected a status line, got {other:?}"),
        }
        a.handle_key(key(KeyCode::Char('r')));
        assert!(
            a.status.starts_with("diagnostics refreshed "),
            "{}",
            a.status
        );
    }

    /// A desk over a real temp config + DB, like the live app.
    fn real_desk_app(dir: &std::path::Path) -> App {
        std::fs::write(
            dir.join("config.toml"),
            format!("db_path = {:?}\n[targets.apple]\nkind = \"company\"\nmarket = \"us\"\ntickers = [\"AAPL\"]\n\
             [llm]\nprovider = \"openrouter\"\nmodel = \"test-model\"\n\
             [plugins.sec_edgar]\nenabled = true\ncontact = \"oracle@example.test\"\n", dir.join("delta.db").display().to_string()),
        )
        .unwrap();
        let mut a = app();
        let now = delta_tui::desk::now_naive();
        let (_, mut config) = delta_core::config::load_config(&dir.join("config.toml")).unwrap();
        config.db_path = dir.join("delta.db").display().to_string();
        // The DB file is created by the settings loader on first open.
        a.desk.settings = delta_tui::screens::SettingsData::load(
            &config,
            &dir.join("delta.db"),
            &dir.join(".env"),
            now,
        );
        a.desk.settings_footer = a.desk.settings.footer(now);
        a.desk.config_path = Some(dir.join("config.toml"));
        a.handle_key(key(KeyCode::Char('c')));
        a
    }

    fn type_into(form_keys: &[&str], a: &mut App) {
        for text in form_keys {
            for ch in text.chars() {
                a.handle_key(key(KeyCode::Char(ch)));
            }
            a.handle_key(key(KeyCode::Tab));
        }
    }

    #[test]
    fn add_market_edit_flows_through_the_services_layer() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        // The default config has the built-in markets only.
        let ids: Vec<&str> = a
            .desk
            .settings
            .markets
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, ["asx", "us"], "the built-in markets before the edit");
        a.handle_key(key(KeyCode::Char('a')));
        assert!(matches!(a.overlay, Some(Overlay::SettingsForm(_))));
        type_into(&["lse", "London Stock Exchange", "GBP", ".L"], &mut a);
        a.handle_key(key(KeyCode::Enter));
        assert!(a.overlay.is_none(), "the form closes on save");
        assert_eq!(a.status, "market saved");
        let raw = delta_core::config::load_toml(&dir.path().join("config.toml")).unwrap();
        assert_eq!(raw["markets"]["lse"]["currency"], "GBP");
        assert_eq!(raw["markets"]["lse"]["yahoo_suffix"], ".L");
        // The reloaded screen shows the new market.
        let ids: Vec<&str> = a
            .desk
            .settings
            .markets
            .iter()
            .map(|m| m.id.as_str())
            .collect();
        assert_eq!(ids, ["asx", "lse", "us"]);
        let text = screen_text(&mut a, 120, 40);
        assert!(text.contains("London Stock Exchange"));
        assert!(text.contains(".L"));
    }

    #[test]
    fn edit_market_prefills_and_saves() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        delta_services::config_ops::save_market(
            &dir.path().join("config.toml"),
            "lse",
            "London Stock Exchange",
            "GBP",
            ".L",
        )
        .unwrap();
        a.desk.reload_settings();
        a.settings.market_selected = 1;
        a.handle_key(key(KeyCode::Char('e')));
        let Some(Overlay::SettingsForm(modal)) = &a.overlay else {
            panic!("edit form open");
        };
        assert_eq!(modal.form.title, "edit market");
        assert_eq!(modal.form.fields[0].value(), "lse");
        a.handle_key(key(KeyCode::Enter));
        assert!(a.overlay.is_none());
        assert_eq!(a.status, "market saved");
    }

    #[test]
    fn remove_market_is_blocked_for_builtins() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        if let Some(action) = a.handle_key(key(KeyCode::Char('x'))) {
            a.update(action);
        }
        assert_eq!(a.status, "only user-defined markets can be removed");
    }

    #[test]
    fn plugin_toggle_flips_the_config_and_the_row() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        if let Some(action) = a.handle_key(key(KeyCode::Char('t'))) {
            a.update(action);
        }
        assert_eq!(a.status, "sec_edgar disabled");
        let raw = delta_core::config::load_toml(&dir.path().join("config.toml")).unwrap();
        assert_eq!(raw["plugins"]["sec_edgar"]["enabled"], false);
        let text = screen_text(&mut a, 120, 40);
        assert!(text.contains("○  sec_edgar  disabled"), "{text}");
        if let Some(action) = a.handle_key(key(KeyCode::Char('t'))) {
            a.update(action);
        }
        assert_eq!(a.status, "sec_edgar enabled");
    }

    #[test]
    fn source_setup_persists_the_contact_field() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        // The configured contact makes the source ready.
        assert_eq!(a.desk.settings.sources.len(), 1);
        assert!(a.desk.settings.sources[0].ready);
        a.handle_key(key(KeyCode::Char('s')));
        assert!(matches!(a.overlay, Some(Overlay::SettingsForm(_))));
        if let Some(Overlay::SettingsForm(modal)) = &a.overlay {
            assert_eq!(modal.form.fields[0].value(), "oracle@example.test");
        }
        for _ in "oracle@example.test".chars() {
            a.handle_key(key(KeyCode::Backspace));
        }
        for ch in "next@example.test".chars() {
            a.handle_key(key(KeyCode::Char(ch)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(a.overlay.is_none());
        assert_eq!(a.status, "source saved");
        let raw = delta_core::config::load_toml(&dir.path().join("config.toml")).unwrap();
        assert_eq!(raw["plugins"]["sec_edgar"]["contact"], "next@example.test");
    }

    #[test]
    fn settings_adds_and_removes_a_target_through_services() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        a.handle_key(key(KeyCode::Char('n')));
        assert!(matches!(a.overlay, Some(Overlay::SettingsForm(_))));
        type_into(&["chips", "theme", "us", "NVDA,AMD"], &mut a);
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.status, "target added");
        let specs =
            delta_services::config_ops::target_specs(&dir.path().join("config.toml")).unwrap();
        assert_eq!(specs["chips"].tickers, ["NVDA", "AMD"]);

        a.handle_key(key(KeyCode::Char('w')));
        if let Some(Overlay::TargetPicker(targets, _)) = &a.overlay {
            assert_eq!(targets, &["apple", "chips"]);
        } else {
            panic!("target picker did not open");
        }
        a.handle_key(key(KeyCode::Down));
        a.handle_key(key(KeyCode::Enter));
        if let Some(Overlay::SettingsForm(modal)) = &a.overlay {
            assert_eq!(modal.form.fields[0].value(), "chips");
            assert_eq!(modal.form.fields[3].value(), "NVDA,AMD");
        } else {
            panic!("edit form did not open");
        }
        for _ in 0..3 {
            a.handle_key(key(KeyCode::Tab));
        }
        for _ in "NVDA,AMD".chars() {
            a.handle_key(key(KeyCode::Backspace));
        }
        for ch in "NVDA,INTC".chars() {
            a.handle_key(key(KeyCode::Char(ch)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.status, "target saved");
        let specs =
            delta_services::config_ops::target_specs(&dir.path().join("config.toml")).unwrap();
        assert_eq!(specs["chips"].tickers, ["NVDA", "INTC"]);
        a.handle_key(key(KeyCode::Char('w')));
        a.handle_key(key(KeyCode::Down));
        a.handle_key(key(KeyCode::Char('x')));
        if let Some(Overlay::SettingsForm(modal)) = &a.overlay {
            assert_eq!(modal.form.fields[0].value(), "chips");
        } else {
            panic!("remove form did not open");
        }
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.status, "target removed");
        let specs =
            delta_services::config_ops::target_specs(&dir.path().join("config.toml")).unwrap();
        assert!(!specs.contains_key("chips"));
    }

    #[test]
    fn rejected_submits_keep_the_form_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        a.handle_key(key(KeyCode::Char('a')));
        // Submit with empty fields: the required validator rejects.
        a.handle_key(key(KeyCode::Enter));
        assert!(
            matches!(a.overlay, Some(Overlay::SettingsForm(_))),
            "the form stays open on a rejected submit"
        );
        // A bad market id reaches the service and surfaces its message.
        type_into(&["No ID", "London", "GBP", ""], &mut a);
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(
            a.status,
            "market ID must use lowercase letters, numbers, or underscores"
        );
        assert!(matches!(a.overlay, Some(Overlay::SettingsForm(_))));
    }

    #[test]
    fn offline_edits_explain_themselves() {
        let mut a = settings_app();
        assert_eq!(a.desk.config_path, None);
        match a.handle_key(key(KeyCode::Char('t'))).unwrap() {
            Action::Status(msg) => assert_eq!(msg, "offline: no config.toml"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn settings_form_paints_over_the_screen() {
        let mut a = settings_app();
        a.handle_key(key(KeyCode::Char('a')));
        let text = screen_text(&mut a, 120, 40);
        assert!(text.contains("add market"), "{text}");
        assert!(text.contains("ID"));
        a.handle_key(key(KeyCode::Esc));
        assert!(a.overlay.is_none());
    }

    #[test]
    fn settings_state_default_matches_the_golden_world() {
        let state = SettingsState::default();
        assert_eq!(state.focus, SettingsFocus::Provider);
        assert_eq!(state.provider_selected, 0);
        assert_eq!(state.diagnostics_open, None);
    }

    #[test]
    fn decision_journal_creates_reviews_filters_and_deletes_via_services() {
        let dir = tempfile::tempdir().unwrap();
        let db_path = dir.path().join("delta.db");
        let mut a = real_desk_app(dir.path());
        a.desk.source = delta_tui::desk::Source::Real(db_path.clone());
        a.handle_key(key(KeyCode::Char('6')));
        a.handle_key(key(KeyCode::Char('n')));
        type_into(
            &[
                "US:AAPL",
                "Cash flows should grow",
                "20x earnings",
                "6m",
                "2027-01-01",
                "Margins contract",
                "",
            ],
            &mut a,
        );
        a.handle_key(key(KeyCode::Enter));
        assert!(a.overlay.is_none(), "{}", a.status);
        let db = delta_core::db::Db::open(&db_path).unwrap();
        let decisions = delta_services::list_decisions(&db, None, true).unwrap();
        assert_eq!(decisions.len(), 1);
        assert_eq!(decisions[0].rationale, "Cash flows should grow");

        a.handle_key(key(KeyCode::Char('r')));
        let review_screen = screen_text(&mut a, 120, 40);
        assert!(review_screen.contains("review decision"));
        assert!(review_screen.contains("ctrl+s save  esc cancel"));
        type_into(&["Margins remain healthy"], &mut a);
        if let Some(Overlay::DecisionForm(modal)) = &mut a.overlay {
            modal.form.fields[1].input.set_value("invalid");
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(matches!(a.overlay, Some(Overlay::DecisionForm(_))));
        if let Some(Overlay::DecisionForm(modal)) = &mut a.overlay {
            modal.form.fields[1].input.set_value("REVIEWED");
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(a.overlay.is_none(), "{}", a.status);
        let reviews = delta_services::review_history(&db, &decisions[0].id).unwrap();
        assert_eq!(reviews.len(), 1);
        assert_eq!(reviews[0].status.as_deref(), Some("reviewed"));

        a.handle_key(key(KeyCode::Char('e')));
        if let Some(Overlay::DecisionForm(modal)) = &mut a.overlay {
            modal.form.fields[1]
                .input
                .set_value("Cash flows keep growing");
        } else {
            panic!("edit form did not open");
        }
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(
            delta_services::get_decision(&db, &decisions[0].id)
                .unwrap()
                .rationale,
            "Cash flows keep growing"
        );
        assert_eq!(
            delta_services::review_history(&db, &decisions[0].id)
                .unwrap()
                .len(),
            1
        );

        a.handle_key(key(KeyCode::Char('/')));
        for ch in "other".chars() {
            a.handle_key(key(KeyCode::Char(ch)));
        }
        a.handle_key(key(KeyCode::Enter));
        assert!(a.decisions.as_ref().unwrap().decisions.is_empty());
        a.handle_key(key(KeyCode::Char('/')));
        for _ in 0..5 {
            a.handle_key(key(KeyCode::Backspace));
        }
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.decisions.as_ref().unwrap().decisions.len(), 1);

        a.handle_key(key(KeyCode::Char('d')));
        assert!(screen_text(&mut a, 120, 40).contains("confirm delete"));
        assert_eq!(
            delta_services::list_decisions(&db, None, true)
                .unwrap()
                .len(),
            1
        );
        a.handle_key(key(KeyCode::Char('y')));
        assert!(delta_services::list_decisions(&db, None, true)
            .unwrap()
            .is_empty());
    }

    #[test]
    fn decision_form_rejects_missing_fields_and_bad_dates_without_closing() {
        let dir = tempfile::tempdir().unwrap();
        let mut a = real_desk_app(dir.path());
        a.desk.source = delta_tui::desk::Source::Real(dir.path().join("delta.db"));
        a.handle_key(key(KeyCode::Char('6')));
        a.handle_key(key(KeyCode::Char('n')));
        a.handle_key(key(KeyCode::Enter));
        assert!(matches!(a.overlay, Some(Overlay::DecisionForm(_))));
        type_into(
            &[
                "US:AAPL",
                "Rationale",
                "20x earnings",
                "6m",
                "next year",
                "Margins",
                "",
            ],
            &mut a,
        );
        a.handle_key(key(KeyCode::Enter));
        assert_eq!(a.status, "review date must be YYYY-MM-DD");
        assert!(matches!(a.overlay, Some(Overlay::DecisionForm(_))));
        let db = delta_core::db::Db::open(dir.path().join("delta.db")).unwrap();
        assert!(delta_services::list_decisions(&db, None, true)
            .unwrap()
            .is_empty());
    }
}
