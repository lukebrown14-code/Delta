//! The Settings screen, live and data-driven (port of
//! `delta/tui/screens/config.py`): provider & model, plugins, data sources
//! & markets, diagnostics. Painting takes a [`SettingsView`] (data +
//! interaction state + footer) and never performs I/O; the desk loads the
//! data through `delta-services`, and the golden builders construct the
//! exporter's worlds.

use std::collections::BTreeMap;
use std::path::Path;

use chrono::NaiveDateTime;
use delta_core::config::AppConfig;
use delta_services::analytics::{CostRow, DataHealth};
use unicode_width::UnicodeWidthStr;

use super::status_bar::{
    draw_status_bar_wide_with_footer, pane_hints, settings_status_bar,
    status_bar_narrow_with_footer, Footer, NarrowTab,
};
use crate::screen::{color, Screen, Style};

// ------------------------------------------------------------------ state

/// Which table the keys act on (Python's focusable widgets, in tab order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsFocus {
    #[default]
    Provider,
    Plugins,
    Sources,
    Markets,
    Diagnostics,
}

/// The interaction state: focus, cursors, diagnostics fold.
#[derive(Debug, Clone, PartialEq)]
pub struct SettingsState {
    pub focus: SettingsFocus,
    /// The provider & model table's cursor: 0 the provider row, 1 the model.
    pub provider_selected: usize,
    pub plugin_selected: usize,
    pub source_selected: usize,
    pub market_selected: usize,
    /// `None` follows the breakpoint (open wide, folded narrow); `d` pins.
    pub diagnostics_open: Option<bool>,
    pub diagnostics_scroll: usize,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            focus: SettingsFocus::Provider,
            provider_selected: 0,
            plugin_selected: 0,
            source_selected: 0,
            market_selected: 0,
            diagnostics_open: None,
            diagnostics_scroll: 0,
        }
    }
}

impl SettingsState {
    /// `diag_open`: the width's default unless `d` pinned a choice.
    pub fn diagnostics_expanded(&self, width: usize) -> bool {
        self.diagnostics_open.unwrap_or(width >= 100)
    }

    /// `d` (action_toggle_diagnostics): pin the fold; expanding on a narrow
    /// screen focuses the diagnostics document, folding returns to setup.
    pub fn toggle_diagnostics(&mut self, width: usize) {
        let expanded = !self.diagnostics_expanded(width);
        self.diagnostics_open = Some(expanded);
        if expanded && width < 100 {
            self.focus = SettingsFocus::Diagnostics;
        } else if !expanded && self.focus == SettingsFocus::Diagnostics {
            self.focus = SettingsFocus::Provider;
        }
    }

    /// `esc` on narrow expanded diagnostics folds it
    /// (action_close_diagnostics).
    pub fn close_diagnostics(&mut self, width: usize) {
        if width < 100 && self.diagnostics_expanded(width) {
            self.diagnostics_open = Some(false);
            self.focus = SettingsFocus::Provider;
        }
    }

    /// Tab through the panes (Python's focus chain).
    pub fn cycle_focus(&mut self, reverse: bool) {
        let order = [
            SettingsFocus::Provider,
            SettingsFocus::Plugins,
            SettingsFocus::Sources,
            SettingsFocus::Markets,
            SettingsFocus::Diagnostics,
        ];
        let current = order
            .iter()
            .position(|focus| *focus == self.focus)
            .unwrap_or(0);
        self.focus = order[(current + if reverse { order.len() - 1 } else { 1 }) % order.len()];
    }

    /// ↑↓ inside the focused table; in diagnostics the arrows scroll the
    /// document instead.
    pub fn move_selection(&mut self, delta: isize, plugins: usize, sources: usize, markets: usize) {
        if self.focus == SettingsFocus::Diagnostics {
            self.diagnostics_scroll = self.diagnostics_scroll.saturating_add_signed(delta);
            return;
        }
        let (selected, len) = match self.focus {
            SettingsFocus::Provider => (&mut self.provider_selected, 2),
            SettingsFocus::Plugins => (&mut self.plugin_selected, plugins),
            SettingsFocus::Sources => (&mut self.source_selected, sources),
            SettingsFocus::Markets => (&mut self.market_selected, markets),
            SettingsFocus::Diagnostics => return,
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }

    /// Keep cursors inside the data after an edit or reload.
    pub fn reconcile(&mut self, plugins: usize, sources: usize, markets: usize) {
        self.provider_selected = self.provider_selected.min(1);
        self.plugin_selected = self.plugin_selected.min(plugins.saturating_sub(1));
        self.source_selected = self.source_selected.min(sources.saturating_sub(1));
        self.market_selected = self.market_selected.min(markets.saturating_sub(1));
    }

    pub fn selected_plugin_id<'a>(&self, data: &'a SettingsData) -> Option<&'a str> {
        data.plugins
            .get(self.plugin_selected)
            .map(|(id, _)| id.as_str())
    }

    pub fn selected_market_id<'a>(&self, data: &'a SettingsData) -> Option<&'a str> {
        data.markets
            .get(self.market_selected)
            .map(|m| m.id.as_str())
    }
}

// -------------------------------------------------------------------- data

/// One data source row: the plugin id, its label, disclosure quality and
/// readiness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsSource {
    pub id: String,
    pub label: String,
    pub primary: bool,
    pub ready: bool,
    /// Non-secret configured contact, for pre-filling the setup form.
    pub contact: String,
}

/// One market profile row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettingsMarket {
    pub id: String,
    pub label: String,
    pub currency: String,
    pub yahoo_suffix: String,
}

/// A cached diagnostics snapshot. Refresh stamps and byte sizes are
/// supplied by the loader so tests stay deterministic.
#[derive(Debug, Clone)]
pub struct SettingsDiagnostics {
    pub health: DataHealth,
    pub costs: Vec<CostRow>,
    pub today_usd: f64,
    pub database_size: String,
    /// The `HH:MM:SS` refresh stamp.
    pub refreshed: String,
}

impl SettingsDiagnostics {
    pub fn total_usd(&self) -> f64 {
        let total: f64 = self.costs.iter().map(|row| row.cost_usd).sum();
        if total == 0.0 {
            0.0
        } else {
            total
        }
    }

    pub fn total_rows(&self) -> usize {
        self.health.counts.values().sum()
    }

    /// The folded one-line summary (`_render_diagnostics`' summary).
    pub fn summary(&self) -> String {
        let mut text = format!("{} rows", comma(self.total_rows()));
        if let Some(ts) = self.health.latest_bar.values().max() {
            text.push_str(&format!(" · latest bar {}", stamp(ts)));
        }
        text.push_str(&format!(" · spend ${:.2}", self.total_usd()));
        text
    }
}

/// Everything the screen paints; loaded through the services layer.
#[derive(Debug, Clone)]
pub struct SettingsData {
    pub provider: String,
    pub provider_connected: bool,
    pub model: String,
    /// `(id, enabled)`, sorted like Python's `sorted(plugins)`.
    pub plugins: Vec<(String, bool)>,
    pub sources: Vec<SettingsSource>,
    pub markets: Vec<SettingsMarket>,
    pub diagnostics: Result<SettingsDiagnostics, String>,
}

/// The exporter's empty-state world (fresh app: one plugin, no sources, no
/// markets, the temp DB's 80 bars, no model chosen) — what the committed
/// `settings-*.json` goldens capture.
pub fn exporter_world() -> SettingsData {
    let mut counts = BTreeMap::new();
    for (table, count) in [
        ("bar", 80),
        ("event", 0),
        ("fundamental", 0),
        ("llmcall", 0),
        ("newsitem", 0),
    ] {
        counts.insert(table.to_string(), count);
    }
    let mut latest_bar = BTreeMap::new();
    latest_bar.insert(
        "US:AAPL".to_string(),
        NaiveDateTime::parse_from_str("2026-09-20 00:00:00", "%Y-%m-%d %H:%M:%S")
            .expect("fixed stamp"),
    );
    SettingsData {
        provider: "openrouter".to_string(),
        provider_connected: true,
        model: String::new(),
        plugins: vec![("sec_edgar".to_string(), true)],
        sources: vec![],
        markets: vec![],
        diagnostics: Ok(SettingsDiagnostics {
            health: DataHealth {
                counts,
                latest_bar,
                last_llm: None,
            },
            costs: vec![],
            today_usd: 0.0,
            database_size: "delta.db · 4 KB".to_string(),
            refreshed: "09:30:00".to_string(),
        }),
    }
}

impl SettingsData {
    /// Load the live settings state from config, the DB and the provider
    /// specs (the read side of `config.py::refresh_view`).
    pub fn load(config: &AppConfig, db_path: &Path, env_path: &Path, now: NaiveDateTime) -> Self {
        let diagnostics = match delta_core::db::Db::open(db_path) {
            Ok(db) => load_diagnostics(&db, db_path, now),
            Err(e) => Err(format!("diagnostics: {e}")),
        };
        let specs = delta_plugins::provider_specs();
        let sources = delta_services::setup::data_provider_status(config, env_path, &specs)
            .into_iter()
            .map(|status| SettingsSource {
                contact: config
                    .plugins
                    .get(&status.name)
                    .and_then(|spec| spec.get("contact"))
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
                id: status.name,
                label: status.label,
                primary: status.primary_disclosure,
                ready: status.configured && status.enabled,
            })
            .collect();
        let provider_connected = !config.llm_provider.is_empty()
            && delta_services::setup::provider_connected(config, env_path);
        SettingsData {
            provider: config.llm_provider.clone(),
            provider_connected,
            model: config.llm_model.clone(),
            plugins: sorted_plugins(config),
            sources,
            markets: sorted_markets(config),
            diagnostics,
        }
    }

    /// The status-bar footer this data implies (age of the newest bar,
    /// provider, all-time spend).
    pub fn footer(&self, now: NaiveDateTime) -> Footer {
        let mut footer = Footer::default();
        match &self.diagnostics {
            Ok(diag) => {
                if let Some(newest) = diag.health.latest_bar.values().max() {
                    let seconds = (now - *newest).num_seconds().max(0);
                    footer.data = format!("data {}", age_label(seconds));
                } else {
                    footer.data = "data none".to_string();
                }
                footer.spend = format!("${:.2}", diag.total_usd());
            }
            Err(_) => footer.data = "data ?".to_string(),
        }
        footer.provider = if self.provider.is_empty() {
            "—".to_string()
        } else {
            self.provider.clone()
        };
        footer
    }
}

fn load_diagnostics(
    db: &delta_core::db::Db,
    db_path: &Path,
    now: NaiveDateTime,
) -> Result<SettingsDiagnostics, String> {
    let health =
        delta_services::analytics::data_health(db).map_err(|e| format!("diagnostics: {e}"))?;
    let costs =
        delta_services::analytics::llm_costs(db, None).map_err(|e| format!("costs: {e}"))?;
    let today = delta_services::analytics::total_spend(
        db,
        Some(now.date().and_hms_opt(0, 0, 0).unwrap_or(now)),
    );
    let size = std::fs::metadata(db_path)
        .map(|m| format!("{} · {}", db_path.display(), human_size(m.len())))
        .unwrap_or_default();
    Ok(SettingsDiagnostics {
        health,
        costs,
        today_usd: today,
        database_size: size,
        refreshed: now.format("%H:%M:%S").to_string(),
    })
}

/// `(id, enabled)` in Python's `sorted(plugins)` order; `enabled` defaults
/// true when the table omits it.
pub fn sorted_plugins(config: &AppConfig) -> Vec<(String, bool)> {
    config
        .plugins
        .iter()
        .map(|(id, table)| {
            let enabled = table
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            (id.clone(), enabled)
        })
        .collect()
}

/// Markets sorted by id (BTreeMap order matches Python's `sorted`).
pub fn sorted_markets(config: &AppConfig) -> Vec<SettingsMarket> {
    config
        .markets
        .iter()
        .map(|(id, market)| SettingsMarket {
            id: id.clone(),
            label: market.label.clone(),
            currency: market.currency.clone(),
            yahoo_suffix: market.yahoo_suffix.clone(),
        })
        .collect()
}

/// `_human_size` (`config.py`).
pub fn human_size(size: u64) -> String {
    let mut value = size as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if value < 1024.0 || unit == "GB" {
            return if unit == "B" || unit == "KB" {
                format!("{value:.0} {unit}")
            } else {
                format!("{value:.1} {unit}")
            };
        }
        value /= 1024.0;
    }
    format!("{value:.1} GB")
}

/// `age_text` (`shell.py`): humanise seconds into the footer label.
pub fn age_label(seconds: i64) -> String {
    if seconds < 300 {
        "live".to_string()
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else if seconds < 86400 {
        format!("{}h", seconds / 3600)
    } else {
        format!("{}d", seconds / 86400)
    }
}

/// `_stamp`: `20 Sep 00:00 UTC`.
pub fn stamp(ts: &NaiveDateTime) -> String {
    ts.format("%d %b %H:%M UTC").to_string()
}

/// Python's `f"{n:,}"`.
pub fn comma(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, ch) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    out
}

// ------------------------------------------------------------------- paint

/// What the screen paints: the data, the interaction state, the footer.
pub struct SettingsView<'a> {
    pub data: &'a SettingsData,
    pub state: &'a SettingsState,
    pub footer: &'a Footer,
}

/// The Settings screen at the normal breakpoint (100..160 columns).
pub fn draw_settings(screen: &mut Screen, view: &SettingsView) {
    paint(screen, view);
}

/// The Settings screen at the wide breakpoint (160+ columns).
pub fn draw_settings_wide(screen: &mut Screen, view: &SettingsView) {
    paint(screen, view);
}

/// The Settings screen below 100 columns: panes stack, diagnostics folds.
pub fn draw_settings_narrow(screen: &mut Screen, view: &SettingsView) {
    paint(screen, view);
}

fn paint(screen: &mut Screen, view: &SettingsView) {
    let w = screen.w;
    let h = screen.h;
    if w < 6 || h < 6 {
        return;
    }
    let bottom = h - 3;
    let narrow = w < 100;
    let expanded = view.state.diagnostics_expanded(w);
    if narrow && expanded {
        // `-diag-full`: diagnostics takes the whole screen; setup is hidden.
        diagnostics_pane(screen, (1, 0, w - 1, bottom), view, true);
    } else {
        let pane_right = if narrow { w - 2 } else { 62 };
        let end = setup_column(screen, view, pane_right, narrow);
        if !narrow {
            diagnostics_pane(screen, (63, 0, w - 2, bottom), view, expanded);
        } else {
            let y = end.min(bottom.saturating_sub(3));
            diagnostics_pane(screen, (1, y, pane_right, y + 2), view, false);
        }
    }
    let y = h - 1;
    if w >= 160 {
        draw_status_bar_wide_with_footer(screen, y, w, "c Settings", view.footer);
    } else if narrow {
        status_bar_narrow_with_footer(screen, NarrowTab::Settings, view.footer);
    } else {
        settings_status_bar(screen, view.footer);
    }
}

/// The provider & model, plugins, sources and markets panes; returns the
/// first row after the stack (where narrow diagnostics folds).
fn setup_column(
    screen: &mut Screen,
    view: &SettingsView,
    pane_right: usize,
    narrow: bool,
) -> usize {
    let data = view.data;
    let state = view.state;
    let plugin_cap = if narrow { 3 } else { 12 };
    let plugin_rows = if data.plugins.is_empty() {
        1
    } else {
        data.plugins.len().min(plugin_cap)
    };

    // Provider & model pane.
    screen.pane(
        1,
        0,
        pane_right,
        3,
        state.focus == SettingsFocus::Provider,
        &[("provider & model", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("↑↓", "choose"), ("enter", "change")]),
    );
    let provider_focused = state.focus == SettingsFocus::Provider;
    provider_row(
        screen,
        1,
        pane_right,
        provider_focused && state.provider_selected == 0,
        data,
    );
    model_row(
        screen,
        2,
        pane_right,
        provider_focused && state.provider_selected == 1,
        data,
    );

    // Plugins pane.
    let plugin_y = 4;
    let enabled = data.plugins.iter().filter(|(_, on)| *on).count();
    let mut title = vec![("plugins ".to_string(), Style::fg(color::BLUE).bold())];
    if !data.plugins.is_empty() {
        title.push((
            format!("· {} of {} ok", enabled, data.plugins.len()),
            Style::fg(color::MUTED).bold(),
        ));
    }
    let title_refs: Vec<(&str, Style)> = title
        .iter()
        .map(|(text, style)| (text.as_str(), *style))
        .collect();
    screen.pane(
        1,
        plugin_y,
        pane_right,
        plugin_y + plugin_rows + 1,
        state.focus == SettingsFocus::Plugins,
        &title_refs,
        &pane_hints(&[("↑↓", "plugin"), ("enter", "details")]),
    );
    if data.plugins.is_empty() {
        screen.text(
            3,
            plugin_y + 1,
            "no plugins discovered",
            Style::fg(color::MUTED),
        );
    } else {
        let focused = state.focus == SettingsFocus::Plugins;
        let id_width = data
            .plugins
            .iter()
            .map(|(id, _)| id.width())
            .max()
            .unwrap_or(0);
        let cols = [3, id_width + 2, 9];
        let start = state
            .plugin_selected
            .saturating_sub(plugin_rows.saturating_sub(1))
            .min(data.plugins.len().saturating_sub(plugin_rows));
        for (row, (index, (id, on))) in data
            .plugins
            .iter()
            .enumerate()
            .skip(start)
            .take(plugin_rows)
            .enumerate()
        {
            let cells = [
                if *on { "●" } else { "○" }.to_string(),
                id.clone(),
                if *on { "enabled" } else { "disabled" }.to_string(),
            ];
            table_row(
                screen,
                plugin_y + 1 + row,
                pane_right,
                &cells,
                &cols,
                index == state.plugin_selected,
                focused && index == state.plugin_selected,
            );
        }
    }

    // Data sources & markets pane.
    let y = plugin_y + plugin_rows + 2;
    let mut cursor = y + 1;
    section_heading(screen, cursor, pane_right, "sources", data.sources.len());
    cursor += 1;
    if !data.sources.is_empty() {
        let rows: Vec<[String; 3]> = data
            .sources
            .iter()
            .map(|source| {
                [
                    source.label.clone(),
                    if source.primary {
                        "primary".to_string()
                    } else {
                        "secondary".to_string()
                    },
                    if source.ready {
                        "ready".to_string()
                    } else {
                        "needs setup".to_string()
                    },
                ]
            })
            .collect();
        let cols = columns(&["Source", "Quality", "Status"], &rows);
        table_header(
            screen,
            cursor,
            pane_right,
            &["Source", "Quality", "Status"],
            &cols,
        );
        cursor += 1;
        let focused = state.focus == SettingsFocus::Sources;
        for (row, cells) in rows.iter().enumerate() {
            table_row(
                screen,
                cursor,
                pane_right,
                cells,
                &cols,
                row == state.source_selected,
                focused && row == state.source_selected,
            );
            cursor += 1;
        }
    }
    cursor += 1; // #cfg-markets-head margin-top: 1
    section_heading(screen, cursor, pane_right, "markets", data.markets.len());
    cursor += 1;
    let market_rows: Vec<[String; 4]> = data
        .markets
        .iter()
        .map(|market| {
            [
                market.id.clone(),
                market.label.clone(),
                market.currency.clone(),
                if market.yahoo_suffix.is_empty() {
                    "—".to_string()
                } else {
                    market.yahoo_suffix.clone()
                },
            ]
        })
        .collect();
    let market_cols = columns(&["ID", "Market", "Currency", "Yahoo"], &market_rows);
    table_header(
        screen,
        cursor,
        pane_right,
        &["ID", "Market", "Currency", "Yahoo"],
        &market_cols,
    );
    cursor += 1;
    let focused = state.focus == SettingsFocus::Markets;
    for (row, cells) in market_rows.iter().enumerate() {
        table_row(
            screen,
            cursor,
            pane_right,
            cells,
            &market_cols,
            row == state.market_selected,
            focused && row == state.market_selected,
        );
        cursor += 1;
    }
    screen.pane(
        1,
        y,
        pane_right,
        cursor,
        state.focus == SettingsFocus::Sources || state.focus == SettingsFocus::Markets,
        &[("data sources & markets", Style::fg(color::BLUE).bold())],
        &pane_hints(&[
            ("enter", "configure/edit"),
            ("a", "add market"),
            ("x", "remove market"),
        ]),
    );
    cursor + 1
}

fn section_heading(screen: &mut Screen, y: usize, pane_right: usize, label: &str, count: usize) {
    screen.text(3, y, label, Style::fg(color::MUTED).bold());
    if count > 0 {
        screen.text_right(
            pane_right - 1,
            y,
            &count.to_string(),
            Style::fg(color::MUTED),
        );
    }
}

/// Textual DataTable column widths: max(header, cells) + 2 padding.
fn columns<const N: usize>(headers: &[&str; N], rows: &[[String; N]]) -> [usize; N] {
    let mut out = [0usize; N];
    for index in 0..N {
        let content = rows
            .iter()
            .map(|row| row[index].width())
            .max()
            .unwrap_or(0)
            .max(headers[index].width());
        out[index] = content + 2;
    }
    out
}

/// A table header row: panel background, bold, full inner width.
fn table_header(
    screen: &mut Screen,
    y: usize,
    pane_right: usize,
    headers: &[&str],
    cols: &[usize],
) {
    let style = Style::fg(color::FG).bg(color::PANEL).bold();
    screen.fill(2, y, pane_right, y + 1, style);
    let mut x = 2usize;
    for (index, header) in headers.iter().enumerate() {
        screen.put(x, y, ' ', style);
        screen.text(x + 1, y, header, style);
        x += cols[index];
    }
}

/// One table row: full-width fill; the cursor row takes the blurred block
/// cursor, a focused cursor the full block cursor.
fn table_row(
    screen: &mut Screen,
    y: usize,
    pane_right: usize,
    cells: &[String],
    cols: &[usize],
    cursor: bool,
    focused: bool,
) {
    let style = if focused {
        Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
    } else if cursor {
        Style::fg(color::FG).bg("#13254b")
    } else {
        Style::fg(color::FG)
    };
    screen.fill(2, y, pane_right, y + 1, style);
    let mut x = 2usize;
    for (index, cell) in cells.iter().enumerate() {
        if x + 1 >= pane_right {
            break;
        }
        screen.put(x, y, ' ', style);
        let visible = (pane_right - x - 1).min(cell.width());
        let text: String = cell.chars().take(visible).collect();
        screen.text(x + 1, y, &text, style);
        x += cols[index];
    }
}

/// The `provider: …` row of the provider & model table.
fn provider_row(
    screen: &mut Screen,
    y: usize,
    pane_right: usize,
    selected: bool,
    data: &SettingsData,
) {
    if selected {
        let style = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
        screen.fill(2, y, pane_right, y + 1, style);
        let text = if data.provider.is_empty() {
            "provider: ○ none — press p".to_string()
        } else {
            format!(
                "provider: ● {}{}",
                data.provider,
                if data.provider_connected {
                    ""
                } else {
                    "  no key"
                }
            )
        };
        screen.text(3, y, &text, style);
        return;
    }
    let base = Style::fg(color::FG);
    screen.fill(2, y, pane_right, y + 1, base);
    if data.provider.is_empty() {
        screen.text(3, y, "provider: ○ none — press p", Style::fg(color::RED));
        return;
    }
    let x = screen.text(3, y, "provider: ", base.bold());
    let dot = if data.provider_connected {
        Style::fg(color::GREEN)
    } else {
        Style::fg(color::AMBER)
    };
    let x = screen.text(x, y, "● ", dot);
    let x = screen.text(x, y, &data.provider, base.bold());
    if !data.provider_connected {
        screen.text(x, y, "  no key", Style::fg(color::AMBER));
    }
}

/// The `model: …` row.
fn model_row(
    screen: &mut Screen,
    y: usize,
    pane_right: usize,
    selected: bool,
    data: &SettingsData,
) {
    let base = Style::fg(color::FG);
    if selected {
        let style = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
        screen.fill(2, y, pane_right, y + 1, style);
        let text = if data.model.is_empty() {
            "model: not chosen — press m".to_string()
        } else {
            format!("model: {}", data.model)
        };
        screen.text(3, y, &text, style);
        return;
    }
    screen.fill(2, y, pane_right, y + 1, base);
    let x = screen.text(3, y, "model: ", base.bold());
    if data.model.is_empty() {
        screen.text(x, y, "not chosen — press m", Style::fg(color::AMBER));
    } else {
        screen.text(x, y, &data.model, base);
    }
}

// ---------------------------------------------------------- diagnostics

/// The diagnostics pane (expanded wide/full, folded narrow summary).
fn diagnostics_pane(
    screen: &mut Screen,
    rect: (usize, usize, usize, usize),
    view: &SettingsView,
    expanded: bool,
) {
    let (x0, y0, x1, y1) = rect;
    if x1 <= x0 + 2 || y1 <= y0 {
        return;
    }
    let narrow = screen.w < 100;
    let mut title = vec![("diagnostics ".to_string(), Style::fg(color::BLUE).bold())];
    if let Ok(diag) = &view.data.diagnostics {
        title.push((
            format!(
                "· {} rows · ${:.2}",
                comma(diag.total_rows()),
                diag.total_usd()
            ),
            Style::fg(color::MUTED).bold(),
        ));
    }
    let title_refs: Vec<(&str, Style)> = title
        .iter()
        .map(|(text, style)| (text.as_str(), *style))
        .collect();
    let mut hints = if expanded {
        vec![("r", "refresh"), ("d", "fold"), ("↑↓", "scroll")]
    } else {
        vec![("d", "expand"), ("r", "refresh")]
    };
    if expanded && narrow {
        hints.push(("esc", "close"));
    }
    screen.pane(
        x0,
        y0,
        x1,
        y1,
        view.state.focus == SettingsFocus::Diagnostics,
        &title_refs,
        &pane_hints(&hints),
    );
    // Inner canvas: one padding column each side of the pane border.
    let inner = x1 - x0 - 1; // fill width across x0+1 ..= x1-1
    match &view.data.diagnostics {
        Err(error) => {
            for (offset, line) in wrap(&format!("Diagnostics: {error}"), inner.saturating_sub(2))
                .into_iter()
                .take(y1 - y0 - 1)
                .enumerate()
            {
                screen.text(x0 + 2, y0 + 1 + offset, &line, Style::fg(color::FG));
            }
        }
        Ok(diag) if !expanded => {
            screen.put(x0 + 2, y0 + 1, '▸', Style::fg(color::MUTED));
            screen.text(
                x0 + 3,
                y0 + 1,
                &format!(" {}", diag.summary()),
                Style::fg(color::FG),
            );
        }
        Ok(diag) => {
            let document = DiagnosticsDocument::new(diag, inner);
            let visible = y1 - y0 - 1;
            let scroll = view
                .state
                .diagnostics_scroll
                .min(document.lines.len().saturating_sub(visible));
            for (row, line) in document.lines.iter().skip(scroll).take(visible).enumerate() {
                document.paint_line(screen, x0, y0 + 1 + row, inner, scroll + row, line);
            }
        }
    }
}

fn wrap(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    crate::wrap::wrap_text(text, width)
        .into_iter()
        .map(|line| line.trim_end().to_string())
        .collect()
}

/// One styled line of the expanded diagnostics document.
enum DiagLine {
    /// Section heading with an optional right-aligned value.
    Heading(&'static str, String),
    HealthTable {
        cols: [usize; 2],
        header: bool,
        row: Option<(String, String)>,
    },
    /// Muted prose (latest bar, last model call, today), pre-wrapped.
    Note(String),
    Blank,
    CostsTable {
        cols: [usize; 4],
        header: bool,
        row: Option<[String; 4]>,
    },
    /// The costs table's horizontal scrollbar (only when it overflows).
    CostsBar {
        thumb: usize,
        cap: Option<char>,
    },
    Total(String),
    Refreshed(String),
}

/// The expanded diagnostics document, laid out once then painted by index
/// (mirrors `_render_diagnostics`).
struct DiagnosticsDocument {
    lines: Vec<DiagLine>,
}

impl DiagnosticsDocument {
    fn new(diag: &SettingsDiagnostics, fill: usize) -> Self {
        let text_width = fill.saturating_sub(2);
        let mut lines = Vec::new();
        lines.push(DiagLine::Heading("evidence", diag.database_size.clone()));
        let table_w = "Table".width().max(
            diag.health
                .counts
                .keys()
                .map(|k| k.width())
                .max()
                .unwrap_or(0),
        ) + 2;
        let rows_w = "Rows".width().max(
            diag.health
                .counts
                .values()
                .map(|c| comma(*c).width())
                .max()
                .unwrap_or(0),
        ) + 2;
        let health_cols = [table_w, rows_w];
        lines.push(DiagLine::HealthTable {
            cols: health_cols,
            header: true,
            row: None,
        });
        for (table, count) in &diag.health.counts {
            lines.push(DiagLine::HealthTable {
                cols: health_cols,
                header: false,
                row: Some((table.clone(), comma(*count))),
            });
        }
        let mut notes = Vec::new();
        if diag.health.latest_bar.is_empty() {
            notes.push("no prices gathered yet — press 2, then U to gather".to_string());
        } else {
            let parts: Vec<String> = diag
                .health
                .latest_bar
                .iter()
                .map(|(id, ts)| format!("{id} {}", stamp(ts)))
                .collect();
            notes.push(format!("latest bar {}", parts.join(" · ")));
        }
        if let Some(ts) = diag.health.last_llm {
            notes.push(format!("last model call {}", stamp(&ts)));
        }
        for note in notes {
            lines.extend(
                wrap(&note, text_width)
                    .into_iter()
                    .map(|piece| piece.trim_end().to_string())
                    .map(DiagLine::Note),
            );
        }
        lines.push(DiagLine::Blank);
        lines.push(DiagLine::Heading("model spend · cumulative", String::new()));
        let cost_cells: Vec<[String; 4]> = diag
            .costs
            .iter()
            .map(|row| {
                [
                    row.task.clone(),
                    row.model.clone(),
                    row.calls.to_string(),
                    format!("${:.3}", row.cost_usd),
                ]
            })
            .collect();
        let mut costs_cols = [
            "Task".width() + 2,
            "Model".width() + 2,
            "Calls".width() + 2,
            "USD".width() + 2,
        ];
        for index in 0..4 {
            let content = cost_cells
                .iter()
                .map(|cells| cells[index].width())
                .max()
                .unwrap_or(0);
            costs_cols[index] = costs_cols[index].max(content + 2);
        }
        lines.push(DiagLine::CostsTable {
            cols: costs_cols,
            header: true,
            row: None,
        });
        for cells in &cost_cells {
            lines.push(DiagLine::CostsTable {
                cols: costs_cols,
                header: false,
                row: Some(cells.clone()),
            });
        }
        let virtual_width: usize = costs_cols.iter().sum();
        if virtual_width > fill {
            lines.push(DiagLine::costs_bar(fill, virtual_width));
        }
        lines.push(DiagLine::Total(format!(
            "total  {} calls  ${:.2}",
            diag.costs.iter().map(|row| row.calls).sum::<usize>(),
            diag.total_usd()
        )));
        lines.push(DiagLine::Note(format!("today ${:.2}", diag.today_usd)));
        lines.push(DiagLine::Blank);
        lines.push(DiagLine::Refreshed(format!(
            "refreshed {} · press r to refresh",
            diag.refreshed
        )));
        Self { lines }
    }

    fn paint_line(
        &self,
        screen: &mut Screen,
        x0: usize,
        y: usize,
        fill: usize,
        index: usize,
        line: &DiagLine,
    ) {
        match line {
            DiagLine::Heading(label, right) => {
                screen.text(x0 + 2, y, label, Style::fg(color::MUTED).bold());
                if !right.is_empty() {
                    screen.text_right(x0 + fill, y, right, Style::fg(color::MUTED));
                }
            }
            DiagLine::HealthTable { cols, header, row } => {
                if *header {
                    table_header_at(screen, x0, y, fill, &["Table", "Rows"], cols);
                } else if let Some((table, count)) = row {
                    table_row_at(
                        screen,
                        x0,
                        y,
                        fill,
                        &[table.clone(), count.clone()],
                        cols,
                        self.is_first_health_row(index),
                    );
                }
            }
            DiagLine::Note(text) => {
                screen.text(x0 + 2, y, text, Style::fg(color::MUTED));
            }
            DiagLine::Blank => {}
            DiagLine::CostsTable { cols, header, row } => {
                if *header {
                    table_header_at(
                        screen,
                        x0,
                        y,
                        fill,
                        &["Task", "Model", "Calls", "USD"],
                        cols,
                    );
                } else if let Some(cells) = row {
                    table_row_at(
                        screen,
                        x0,
                        y,
                        fill,
                        cells,
                        cols,
                        self.is_first_cost_row(index),
                    );
                }
            }
            DiagLine::CostsBar { thumb, cap } => {
                // Textual's ScrollBarRender at position 0: thumb cells in
                // $scrollbar on the default ground, the end cap in
                // $scrollbar on $scrollbar-background, the remainder the
                // scrollbar background itself.
                let track = Style::fg(color::SURFACE_SCROLLBAR);
                let mut x = x0 + 1;
                for _ in 0..*thumb {
                    screen.put(x, y, ' ', track);
                    x += 1;
                }
                if let Some(cap) = cap {
                    screen.put(
                        x,
                        y,
                        *cap,
                        Style::fg(color::SURFACE_SCROLLBAR).bg(color::SURFACE),
                    );
                    x += 1;
                }
                while x < x0 + 1 + fill {
                    screen.put(x, y, ' ', Style::fg(color::FG).bg(color::SURFACE));
                    x += 1;
                }
            }
            DiagLine::Total(text) => {
                screen.text(x0 + 2, y, text, Style::fg(color::FG).bold());
            }
            DiagLine::Refreshed(text) => {
                if let Some(position) = text.find("press r") {
                    let before = &text[..position + 6];
                    let after = &text[position + 7..];
                    let x = screen.text(x0 + 2, y, before, Style::fg(color::MUTED));
                    let x = screen.text(x, y, "r", Style::fg(color::BLUE).bold());
                    screen.text(x, y, after, Style::fg(color::MUTED));
                } else {
                    screen.text(x0 + 2, y, text, Style::fg(color::MUTED));
                }
            }
        }
    }

    fn is_first_health_row(&self, index: usize) -> bool {
        matches!(
            self.lines.get(index.wrapping_sub(1)),
            Some(DiagLine::HealthTable { header: true, .. })
        )
    }

    fn is_first_cost_row(&self, index: usize) -> bool {
        matches!(
            self.lines.get(index.wrapping_sub(1)),
            Some(DiagLine::CostsTable { header: true, .. })
        )
    }
}

impl DiagLine {
    /// Textual `ScrollBarRender.render_bar` at position 0 for a horizontal
    /// bar of `size` cells with `virtual` content width.
    fn costs_bar(size: usize, virtual_width: usize) -> Self {
        const BARS: [char; 8] = ['▉', '▊', '▋', '▌', '▍', '▎', '▏', ' '];
        let bar_ratio = virtual_width as f64 / size as f64;
        let thumb = ((size as f64) / bar_ratio).max(1.0);
        let end = (thumb * 8.0).ceil() as usize;
        let (end_index, end_bar) = (end / 8, end % 8);
        let cap = BARS
            .get(7usize.saturating_sub(end_bar))
            .copied()
            .filter(|ch| *ch != ' ');
        DiagLine::CostsBar {
            thumb: end_index.min(size),
            cap,
        }
    }
}

/// Header row inside the diagnostics pane (cells offset from pane x0).
fn table_header_at(
    screen: &mut Screen,
    x0: usize,
    y: usize,
    fill: usize,
    headers: &[&str],
    cols: &[usize],
) {
    let style = Style::fg(color::FG).bg(color::PANEL).bold();
    screen.fill(x0 + 1, y, x0 + 1 + fill, y + 1, style);
    let mut x = x0 + 1;
    for (index, header) in headers.iter().enumerate() {
        screen.put(x, y, ' ', style);
        screen.text(x + 1, y, header, style);
        x += cols[index];
    }
}

/// Data row inside the diagnostics pane (the cursor row is always the
/// table's first data row; the diagnostics tables never take focus).
fn table_row_at(
    screen: &mut Screen,
    x0: usize,
    y: usize,
    fill: usize,
    cells: &[String],
    cols: &[usize],
    cursor: bool,
) {
    let style = if cursor {
        Style::fg(color::FG).bg("#13254b")
    } else {
        Style::fg(color::FG)
    };
    screen.fill(x0 + 1, y, x0 + 1 + fill, y + 1, style);
    let mut x = x0 + 1;
    for (index, cell) in cells.iter().enumerate() {
        if x + 1 >= x0 + 1 + fill {
            break;
        }
        screen.put(x, y, ' ', style);
        let visible = (x0 + 1 + fill - x - 1).min(cell.width());
        let text: String = cell.chars().take(visible).collect();
        screen.text(x + 1, y, &text, style);
        x += cols[index];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> SettingsState {
        SettingsState::default()
    }

    #[test]
    fn diagnostics_follow_the_breakpoint_until_pinned() {
        let mut s = state();
        assert!(s.diagnostics_expanded(120), "wide default: open");
        assert!(!s.diagnostics_expanded(80), "narrow default: folded");
        s.toggle_diagnostics(80);
        assert!(s.diagnostics_expanded(80), "d pins it open");
        assert_eq!(s.focus, SettingsFocus::Diagnostics, "focus moves in");
        s.toggle_diagnostics(80);
        assert!(!s.diagnostics_expanded(80));
        assert_eq!(s.focus, SettingsFocus::Provider, "focus returns");
        // Pinning survives a resize.
        s.diagnostics_open = Some(true);
        assert!(s.diagnostics_expanded(80));
    }

    #[test]
    fn esc_closes_narrow_expanded_diagnostics_only() {
        let mut s = state();
        s.diagnostics_open = Some(true);
        s.focus = SettingsFocus::Diagnostics;
        s.close_diagnostics(80);
        assert!(!s.diagnostics_expanded(80));
        assert_eq!(s.focus, SettingsFocus::Provider);
        // On wide, esc leaves it open (Python checks narrow).
        let mut wide = state();
        wide.diagnostics_open = Some(true);
        wide.close_diagnostics(120);
        assert!(wide.diagnostics_expanded(120));
    }

    #[test]
    fn focus_cycles_through_the_panes_both_ways() {
        let mut s = state();
        s.cycle_focus(false);
        assert_eq!(s.focus, SettingsFocus::Plugins);
        s.cycle_focus(false);
        s.cycle_focus(false);
        s.cycle_focus(false);
        assert_eq!(s.focus, SettingsFocus::Diagnostics);
        s.cycle_focus(false);
        assert_eq!(s.focus, SettingsFocus::Provider, "wraps");
        s.cycle_focus(true);
        assert_eq!(s.focus, SettingsFocus::Diagnostics, "reverse wraps");
    }

    #[test]
    fn selection_moves_clamp_to_the_data() {
        let mut s = state();
        s.move_selection(-1, 1, 1, 2);
        assert_eq!(s.provider_selected, 0);
        s.move_selection(1, 1, 1, 2);
        assert_eq!(s.provider_selected, 1, "provider/model rows");
        s.move_selection(1, 1, 1, 2);
        assert_eq!(s.provider_selected, 1, "clamped at the model row");
        s.focus = SettingsFocus::Markets;
        s.move_selection(1, 1, 1, 2);
        s.move_selection(1, 1, 1, 2);
        s.move_selection(9, 1, 1, 2);
        assert_eq!(s.market_selected, 1, "clamped at last market");
        // Focus diagnostics and the arrows scroll instead.
        s.focus = SettingsFocus::Diagnostics;
        s.move_selection(3, 1, 1, 2);
        assert_eq!(s.diagnostics_scroll, 3);
        assert_eq!(s.market_selected, 1);
    }

    #[test]
    fn reconcile_pulls_cursors_back_after_edits() {
        let mut s = state();
        s.plugin_selected = 5;
        s.source_selected = 3;
        s.market_selected = 9;
        s.provider_selected = 4;
        s.reconcile(2, 1, 3);
        assert_eq!(s.plugin_selected, 1);
        assert_eq!(s.source_selected, 0);
        assert_eq!(s.market_selected, 2);
        assert_eq!(s.provider_selected, 1);
    }

    #[test]
    fn selected_ids_follow_the_cursor() {
        let data = exporter_world();
        let mut s = state();
        assert_eq!(s.selected_plugin_id(&data), Some("sec_edgar"));
        assert_eq!(s.selected_market_id(&data), None, "no markets");
        s.move_selection(1, 1, 0, 0);
        assert_eq!(s.selected_plugin_id(&data), Some("sec_edgar"), "clamped");
    }

    #[test]
    fn summary_and_counts_format_like_python() {
        assert_eq!(comma(0), "0");
        assert_eq!(comma(999), "999");
        assert_eq!(comma(1_234), "1,234");
        assert_eq!(comma(1_234_567), "1,234,567");
        let diag = exporter_world().diagnostics.unwrap();
        assert_eq!(
            diag.summary(),
            "80 rows · latest bar 20 Sep 00:00 UTC · spend $0.00"
        );
        assert_eq!(diag.total_rows(), 80);
        let ts = NaiveDateTime::parse_from_str("2026-09-20 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap();
        assert_eq!(stamp(&ts), "20 Sep 00:00 UTC");
    }

    #[test]
    fn age_labels_match_shell_py() {
        assert_eq!(age_label(60), "live");
        assert_eq!(age_label(600), "10m");
        assert_eq!(age_label(7_200), "2h");
        assert_eq!(age_label(90_000), "1d");
    }

    #[test]
    fn human_size_matches_config_py() {
        assert_eq!(human_size(80), "80 B");
        assert_eq!(human_size(4_096), "4 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }

    #[test]
    fn footer_reads_diagnostics_and_provider() {
        let data = exporter_world();
        let now =
            NaiveDateTime::parse_from_str("2026-09-21 09:30:00", "%Y-%m-%d %H:%M:%S").unwrap();
        let footer = data.footer(now);
        assert_eq!(footer.data, "data 1d");
        assert_eq!(footer.provider, "openrouter");
        assert_eq!(footer.spend, "$0.00");
    }

    #[test]
    fn costs_scrollbar_math_matches_textual() {
        // The live golden at 120x40: 54 visible cells, 71 of columns.
        let DiagLine::CostsBar { thumb, cap } = DiagLine::costs_bar(54, 71) else {
            panic!("scrollbar line");
        };
        assert_eq!(thumb, 41);
        assert_eq!(cap, Some('▏'));
        // No overflow: no bar is constructed (guarded by the caller).
        let DiagLine::CostsBar { thumb, cap } = DiagLine::costs_bar(134, 71) else {
            unreachable!();
        };
        assert_eq!(thumb, 134, "thumb covers the track when it fits");
        assert_eq!(cap, None);
    }
}
