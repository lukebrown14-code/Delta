//! Settings pane state and rendering. Database and secret reads are supplied
//! by callers as cached values; painting never performs I/O.
use crate::research::{truncate, wrap};
use crate::screen::{color, Screen, Style};
use delta_core::config::AppConfig;
use delta_services::{CostRow, DataHealth};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SettingsFocus {
    #[default]
    Provider,
    Plugins,
    Sources,
    Markets,
    Diagnostics,
}

/// A cached result of the diagnostics worker. Refresh timestamps and byte
/// sizes are supplied explicitly so screen tests can remain deterministic.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SettingsDiagnostics {
    pub health: DataHealth,
    pub costs: Vec<CostRow>,
    pub today_usd: f64,
    pub database_size: String,
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
    pub fn summary(&self) -> String {
        let mut text = format!("{} rows", self.health.counts.values().sum::<usize>());
        if let Some(ts) = self.health.latest_bar.values().max() {
            text.push_str(&format!(" · latest bar {}", ts.format("%d %b %H:%M UTC")));
        }
        text.push_str(&format!(" · spend ${:.2}", self.total_usd()));
        text
    }
}

/// Configuration-ready sources only. Other discovered adapters belong in
/// Plugins rather than being presented as configurable sources.
#[derive(Debug, Clone)]
pub struct SettingsSource {
    pub id: String,
    pub label: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct SettingsState {
    pub focus: SettingsFocus,
    /// Python starts on the model row (provider = 0, model = 1).
    pub provider_selected: usize,
    pub plugin_selected: usize,
    pub source_selected: usize,
    pub market_selected: usize,
    /// None follows the breakpoint: expanded wide, folded narrow.
    pub diagnostics_open: Option<bool>,
    pub diagnostics_scroll: usize,
    pub setup_scroll: usize,
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
            setup_scroll: 0,
        }
    }
}
impl SettingsState {
    pub fn diagnostics_expanded(&self, width: usize) -> bool {
        self.diagnostics_open.unwrap_or(width >= 100)
    }
    pub fn toggle_diagnostics(&mut self, width: usize) {
        let expanded = !self.diagnostics_expanded(width);
        self.diagnostics_open = Some(expanded);
        if expanded && width < 100 {
            self.focus = SettingsFocus::Diagnostics;
        } else if !expanded && self.focus == SettingsFocus::Diagnostics {
            self.focus = SettingsFocus::Provider;
        }
    }
    pub fn back(&mut self) {
        self.diagnostics_open = Some(false);
        if self.focus == SettingsFocus::Diagnostics {
            self.focus = SettingsFocus::Provider;
        }
    }
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
        self.focus = order[(current + if reverse { 4 } else { 1 }) % order.len()];
    }
    /// Move inside the focused table, or scroll the diagnostics document.
    pub fn move_selection(&mut self, delta: isize, plugins: usize, sources: usize, markets: usize) {
        let (selected, len) = match self.focus {
            SettingsFocus::Provider => (&mut self.provider_selected, 2),
            SettingsFocus::Plugins => (&mut self.plugin_selected, plugins),
            SettingsFocus::Sources => (&mut self.source_selected, sources),
            SettingsFocus::Markets => (&mut self.market_selected, markets),
            SettingsFocus::Diagnostics => {
                self.diagnostics_scroll = self.diagnostics_scroll.saturating_add_signed(delta);
                return;
            }
        };
        *selected = selected
            .saturating_add_signed(delta)
            .min(len.saturating_sub(1));
    }
    pub fn reconcile(&mut self, cfg: &AppConfig, sources: usize) {
        self.plugin_selected = self
            .plugin_selected
            .min(cfg.plugins.len().saturating_sub(1));
        self.market_selected = self
            .market_selected
            .min(cfg.markets.len().saturating_sub(1));
        self.source_selected = self.source_selected.min(sources.saturating_sub(1));
    }
    pub fn selected_market<'a>(&self, cfg: &'a AppConfig) -> Option<&'a str> {
        cfg.markets
            .keys()
            .nth(self.market_selected)
            .map(String::as_str)
    }
    pub fn selected_plugin<'a>(&self, cfg: &'a AppConfig) -> Option<&'a str> {
        cfg.plugins
            .keys()
            .nth(self.plugin_selected)
            .map(String::as_str)
    }
    pub fn selected_source<'a>(&self, sources: &'a [SettingsSource]) -> Option<&'a str> {
        sources
            .get(self.source_selected)
            .map(|source| source.id.as_str())
    }
}

pub struct SettingsView<'a> {
    pub config: &'a AppConfig,
    pub state: &'a SettingsState,
    pub sources: &'a [SettingsSource],
    pub diagnostics: &'a Result<SettingsDiagnostics, String>,
    /// Resolve key availability during refresh, never from the renderer.
    pub provider_connected: bool,
}
impl SettingsView<'_> {
    pub fn paint(&self, screen: &mut Screen) {
        let bottom = screen.h.saturating_sub(3);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        if screen.w < 4 || bottom < 2 {
            return;
        }
        let wide = screen.w >= 100;
        let expanded = self.state.diagnostics_expanded(screen.w);
        if !wide && expanded {
            self.diagnostics_pane(
                screen,
                (1, 0, screen.w - 2, bottom.saturating_sub(1)),
                true,
                self.state.focus == SettingsFocus::Diagnostics,
            );
            return;
        }
        let setup_width = if wide {
            62.min(screen.w.saturating_sub(4))
        } else {
            screen.w - 2
        };
        let mut setup = Screen::new(setup_width, bottom);
        let end = self.setup(&mut setup, !wide);
        screen.blit_at(&setup, 1, 0);
        if wide {
            let x = setup_width + 1;
            self.diagnostics_pane(
                screen,
                (x, 0, screen.w.saturating_sub(x + 1), bottom),
                expanded,
                self.state.focus == SettingsFocus::Diagnostics,
            );
        } else {
            let y = end.min(bottom.saturating_sub(3));
            self.diagnostics_pane(
                screen,
                (1, y, screen.w - 2, (y + 2).min(bottom)),
                false,
                self.state.focus == SettingsFocus::Diagnostics,
            );
        }
    }
    fn setup(&self, screen: &mut Screen, narrow: bool) -> usize {
        let plugin_rows = self
            .config
            .plugins
            .len()
            .max(1)
            .min(if narrow { 3 } else { 12 });
        let source_rows = self.sources.len().max(1).min(if narrow { 2 } else { 4 });
        let market_rows = self.config.markets.len().min(if narrow { 3 } else { 6 });
        let mut content = Screen::new(screen.w, 14 + plugin_rows + source_rows + market_rows);
        let width = screen.w.saturating_sub(1);
        pane(
            &mut content,
            0,
            0,
            width,
            3,
            self.state.focus == SettingsFocus::Provider,
            "provider & model",
            "↑↓ choose  enter change",
        );
        let provider = if self.config.llm_provider.is_empty() {
            "provider: ○ none — press p".into()
        } else {
            format!(
                "provider: ● {}{}",
                self.config.llm_provider,
                if self.provider_connected {
                    ""
                } else {
                    "  no key"
                }
            )
        };
        self.row(
            &mut content,
            1,
            &provider,
            self.state.focus == SettingsFocus::Provider && self.state.provider_selected == 0,
        );
        self.row(
            &mut content,
            2,
            &format!(
                "model: {}",
                if self.config.llm_model.is_empty() {
                    "not chosen — press m"
                } else {
                    &self.config.llm_model
                }
            ),
            self.state.focus == SettingsFocus::Provider && self.state.provider_selected == 1,
        );
        let enabled = self
            .config
            .plugins
            .values()
            .filter(|spec| {
                spec.get("enabled")
                    .and_then(serde_json::Value::as_bool)
                    .unwrap_or(true)
            })
            .count();
        let plugin_y = 4;
        pane(
            &mut content,
            0,
            plugin_y,
            width,
            plugin_y + plugin_rows + 1,
            self.state.focus == SettingsFocus::Plugins,
            &format!("plugins · {enabled} of {} ok", self.config.plugins.len()),
            "↑↓ plugin  enter details",
        );
        let start = self
            .state
            .plugin_selected
            .saturating_sub(plugin_rows.saturating_sub(1));
        for (row, (index, (id, spec))) in self
            .config
            .plugins
            .iter()
            .enumerate()
            .skip(start)
            .take(plugin_rows)
            .enumerate()
        {
            let enabled = spec
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(true);
            self.table_row(
                &mut content,
                plugin_y + 1 + row,
                &format!(
                    "{}  {id}  {}",
                    if enabled { "●" } else { "○" },
                    if enabled { "enabled" } else { "disabled" }
                ),
                index == 0,
                self.state.focus == SettingsFocus::Plugins && index == self.state.plugin_selected,
            );
        }
        if self.config.plugins.is_empty() {
            self.row(&mut content, plugin_y + 1, "no plugins discovered", false);
        }
        let y = plugin_y + plugin_rows + 2;
        let end = y + 6 + source_rows + market_rows;
        pane(
            &mut content,
            0,
            y,
            width,
            end,
            matches!(
                self.state.focus,
                SettingsFocus::Sources | SettingsFocus::Markets
            ),
            "data sources & markets",
            "enter configure/edit  a add market  x remove market",
        );
        self.row(
            &mut content,
            y + 1,
            &format!("sources{:>51}", self.sources.len()),
            false,
        );
        self.row(&mut content, y + 2, "Source     Quality  Status", false);
        let start = self
            .state
            .source_selected
            .saturating_sub(source_rows.saturating_sub(1));
        for (row, (index, source)) in self
            .sources
            .iter()
            .enumerate()
            .skip(start)
            .take(source_rows)
            .enumerate()
        {
            self.table_row(
                &mut content,
                y + 3 + row,
                &format!("{:<11}{:<9}{}", source.label, "primary", source.status),
                index == 0,
                self.state.focus == SettingsFocus::Sources && index == self.state.source_selected,
            );
        }
        let market_y = y + 4 + source_rows;
        self.row(
            &mut content,
            market_y,
            &format!("markets{:>51}", self.config.markets.len()),
            false,
        );
        self.row(
            &mut content,
            market_y + 1,
            "ID   Market                          Currency  Yahoo",
            false,
        );
        let start = self
            .state
            .market_selected
            .saturating_sub(market_rows.saturating_sub(1));
        for (row, (index, (id, market))) in self
            .config
            .markets
            .iter()
            .enumerate()
            .skip(start)
            .take(market_rows)
            .enumerate()
        {
            self.table_row(
                &mut content,
                market_y + 2 + row,
                &format!(
                    "{id:<5}{:<32}{:<10}{}",
                    market.label,
                    market.currency,
                    if market.yahoo_suffix.is_empty() {
                        "—"
                    } else {
                        &market.yahoo_suffix
                    }
                ),
                index == 0,
                self.state.focus == SettingsFocus::Markets && index == self.state.market_selected,
            );
        }
        // Keep the active table visible on short terminals. The explicit
        // scroll permits an eventual mouse wheel implementation as well.
        let active_y = match self.state.focus {
            SettingsFocus::Provider => self.state.provider_selected + 1,
            SettingsFocus::Plugins => {
                plugin_y + 1 + self.state.plugin_selected.min(plugin_rows - 1)
            }
            SettingsFocus::Sources => y + 2 + self.state.source_selected.min(source_rows - 1),
            SettingsFocus::Markets => {
                market_y
                    + 2
                    + self
                        .state
                        .market_selected
                        .min(market_rows.saturating_sub(1))
            }
            SettingsFocus::Diagnostics => 0,
        };
        let available = screen.h.saturating_sub(if narrow { 3 } else { 0 }).max(1);
        let scroll = self
            .state
            .setup_scroll
            .max(active_y.saturating_sub(available - 1))
            .min(end);
        for target_y in 0..available {
            if target_y + scroll >= content.h {
                break;
            }
            for x in 0..screen.w {
                screen.cells[target_y * screen.w + x] =
                    content.cells[(target_y + scroll) * content.w + x].clone();
            }
        }
        (end + 1).saturating_sub(scroll)
    }
    fn row(&self, screen: &mut Screen, y: usize, text: &str, selected: bool) {
        if text.starts_with("model: ") && !selected {
            let base = Style::fg(color::FG);
            screen.fill(1, y, screen.w.saturating_sub(1), y + 1, base);
            let x = screen.text(2, y, "model: ", base.bold());
            screen.text(x, y, text.trim_start_matches("model: "), base);
        } else if text.starts_with("sources") || text.starts_with("markets") {
            let label = text.split_whitespace().next().unwrap_or_default();
            screen.text(2, y, label, Style::fg(color::MUTED).bold());
            let count = text.trim_start_matches(label).trim();
            if !count.is_empty() {
                screen.text_right(screen.w - 2, y, count, Style::fg(color::MUTED));
            }
        } else if text.starts_with("Source     Quality") || text.starts_with("ID   Market") {
            let style = Style::fg(color::FG).bg(color::PANEL).bold();
            screen.fill(1, y, screen.w.saturating_sub(1), y + 1, style);
            screen.text(2, y, text, style);
        } else {
            let style = if selected {
                Style::fg(color::WHITE).bg("#494949").bold()
            } else {
                Style::fg(color::FG)
            };
            if selected {
                screen.fill(1, y, screen.w.saturating_sub(1), y + 1, style);
            }
            screen.text(2, y, &truncate(text, screen.w.saturating_sub(3)), style);
        }
    }
    fn table_row(&self, screen: &mut Screen, y: usize, text: &str, cursor: bool, focused: bool) {
        let style = if focused {
            Style::fg(color::WHITE).bg("#494949").bold()
        } else if cursor {
            Style::fg(color::FG).bg("#242424")
        } else {
            Style::fg(color::FG)
        };
        screen.fill(1, y, screen.w.saturating_sub(1), y + 1, style);
        screen.text(2, y, &truncate(text, screen.w.saturating_sub(3)), style);
    }
    fn diagnostics_pane(
        &self,
        screen: &mut Screen,
        bounds: (usize, usize, usize, usize),
        expanded: bool,
        focused: bool,
    ) {
        let (x, y, width, bottom) = bounds;
        if width < 2 || bottom <= y {
            return;
        }
        let badge = self
            .diagnostics
            .as_ref()
            .map(|data| {
                format!(
                    "{} rows · ${:.2}",
                    data.health.counts.values().sum::<usize>(),
                    data.total_usd()
                )
            })
            .unwrap_or_else(|_| "unavailable".into());
        pane(
            screen,
            x,
            y,
            x + width - 1,
            bottom,
            focused,
            &format!("diagnostics · {badge}"),
            if expanded {
                "r refresh  d fold  ↑↓ scroll"
            } else {
                "d expand  r refresh"
            },
        );
        let inner_width = width.saturating_sub(3);
        let lines = match self.diagnostics {
            Err(error) => wrap(&format!("Diagnostics: {error}"), inner_width),
            Ok(data) if !expanded => vec![format!("▸ {}", data.summary())],
            Ok(data) => {
                let mut lines = vec!["evidence".into(), "Table        Rows".into()];
                for (table, count) in &data.health.counts {
                    lines.push(format!("{table:12} {count}"));
                }
                if data.health.latest_bar.is_empty() {
                    lines.push("no prices gathered yet — press 2, then U to gather".into());
                }
                for (id, ts) in &data.health.latest_bar {
                    lines.push(format!("latest bar {id} {}", ts.format("%d %b %H:%M UTC")));
                }
                if let Some(ts) = data.health.last_llm {
                    lines.push(format!("last model call {}", ts.format("%d %b %H:%M UTC")));
                }
                lines.extend([
                    String::new(),
                    "model spend · cumulative".into(),
                    "Task  Model  Calls  USD".into(),
                ]);
                for row in &data.costs {
                    lines.push(format!(
                        "{}  {}  {}  ${:.3}",
                        row.task, row.model, row.calls, row.cost_usd
                    ));
                }
                lines.extend([
                    format!(
                        "total  {} calls  ${:.2}",
                        data.costs.iter().map(|row| row.calls).sum::<usize>(),
                        data.total_usd()
                    ),
                    format!("today ${:.2}", data.today_usd),
                    String::new(),
                    format!("refreshed {} · press r to refresh", data.refreshed),
                ]);
                lines
                    .iter()
                    .flat_map(|line| {
                        if line.is_empty() {
                            vec![String::new()]
                        } else if line.width() > inner_width {
                            wrap(line, inner_width)
                        } else {
                            vec![line.clone()]
                        }
                    })
                    .collect()
            }
        };
        let visible = bottom.saturating_sub(y + 1);
        let scroll = if expanded {
            self.state
                .diagnostics_scroll
                .min(lines.len().saturating_sub(visible))
        } else {
            0
        };
        for (row, line) in lines.iter().skip(scroll).take(visible).enumerate() {
            let index = scroll + row;
            let line_y = y + 1 + row;
            if !expanded {
                if let Some(summary) = line.strip_prefix("▸ ") {
                    screen.text(x + 2, line_y, "▸", Style::fg(color::MUTED));
                    screen.text(x + 3, line_y, " ", Style::fg(color::FG));
                    screen.text(x + 4, line_y, summary, Style::fg(color::FG));
                } else if !line.is_empty() {
                    screen.text(x + 2, line_y, line, Style::fg(color::FG));
                }
                continue;
            }
            if index == 0 {
                screen.text(x + 2, line_y, "evidence", Style::fg(color::MUTED).bold());
                if let Ok(data) = self.diagnostics {
                    let size_x = x + width - 2 - data.database_size.width();
                    screen.text(size_x, line_y, &data.database_size, Style::fg(color::MUTED));
                }
                continue;
            }
            if index == 1 || index == 10 {
                let style = Style::fg(color::FG).bg(color::PANEL).bold();
                screen.fill(x + 1, line_y, x + width - 1, line_y + 1, style);
                screen.text(x + 2, line_y, line, style);
                continue;
            }
            let cost_rows = self.diagnostics.as_ref().map_or(0, |data| data.costs.len());
            if (2..=6).contains(&index) || (11..11 + cost_rows).contains(&index) {
                let style = if index == 2 || index == 11 {
                    Style::fg(color::FG).bg("#242424")
                } else {
                    Style::fg(color::FG)
                };
                screen.fill(x + 1, line_y, x + width - 1, line_y + 1, style);
                screen.text(x + 2, line_y, line, style);
                continue;
            }
            if !line.is_empty() {
                let style = if index == 7 || index == 12 + cost_rows || index == 14 + cost_rows {
                    Style::fg(color::MUTED)
                } else if index == 9 {
                    Style::fg(color::MUTED).bold()
                } else if index == 11 + cost_rows {
                    Style::fg(color::FG).bold()
                } else {
                    Style::fg(color::FG)
                };
                if index == lines.len() - 1 {
                    if let Some(position) = line.find("press r") {
                        let before = &line[..position + 6];
                        let after = &line[position + 7..];
                        let next = screen.text(x + 2, line_y, before, style);
                        let next = screen.text(next, line_y, "r", Style::fg("#898989").bold());
                        screen.text(next, line_y, after, style);
                        continue;
                    }
                }
                screen.text(x + 2, line_y, &truncate(line, inner_width), style);
            }
        }
    }
}
#[allow(clippy::too_many_arguments)]
fn pane(
    screen: &mut Screen,
    x: usize,
    y: usize,
    right: usize,
    bottom: usize,
    active: bool,
    title: &str,
    hints: &str,
) {
    let border = Style::fg(if active { "#898989" } else { "#333333" });
    let (tl, tr, bl, br, horizontal, vertical) = if active {
        ('┏', '┓', '┗', '┛', '━', '┃')
    } else {
        ('┌', '┐', '└', '┘', '─', '│')
    };
    screen.put(x, y, tl, border);
    screen.put(right, y, tr, border);
    screen.put(x, bottom, bl, border);
    screen.put(right, bottom, br, border);
    for cx in x + 1..right {
        screen.put(cx, y, horizontal, border);
        screen.put(cx, bottom, horizontal, border);
    }
    for cy in y + 1..bottom {
        screen.put(x, cy, vertical, border);
        screen.put(right, cy, vertical, border);
    }
    let title_style = Style::fg("#898989").bold();
    let mut title_x = x + 2;
    screen.put(title_x, y, ' ', title_style);
    let (title, badge) = title
        .split_once(" · ")
        .map_or((title, None), |(a, b)| (a, Some(b)));
    title_x = screen.text(title_x + 1, y, title, title_style);
    if let Some(badge) = badge {
        screen.put(title_x, y, ' ', title_style);
        title_x += 1;
        title_x = screen.text(title_x, y, "· ", Style::fg(color::MUTED).bold());
        title_x = screen.text(title_x, y, badge, Style::fg(color::MUTED).bold());
    }
    screen.put(title_x, y, ' ', title_style);
    screen.put(x + 1, bottom, horizontal, border);
    let mut cursor_x = x + 2;
    cursor_x = screen.text(cursor_x, bottom, " ", Style::fg(color::MUTED));
    let mut cursor = 0;
    for word in hints.split_whitespace() {
        let offset = hints[cursor..].find(word).unwrap_or(0) + cursor;
        if offset > cursor {
            let spaces = &hints[cursor..offset];
            cursor_x = screen.text(cursor_x, bottom, spaces, Style::fg(color::MUTED));
        }
        let style = if matches!(word, "enter" | "a" | "x" | "r" | "d" | "↑↓") {
            Style::fg("#898989").bold()
        } else {
            Style::fg(color::MUTED)
        };
        cursor_x = screen.text(cursor_x, bottom, word, style);
        cursor = offset + word.len();
    }
    if cursor < hints.len() {
        cursor_x = screen.text(cursor_x, bottom, &hints[cursor..], Style::fg(color::MUTED));
    }
    screen.put(cursor_x, bottom, ' ', Style::fg(color::MUTED));
}

#[cfg(test)]
mod tests {
    use super::*;
    fn text(screen: &Screen) -> String {
        screen.cells.iter().map(|cell| cell.ch).collect()
    }
    #[test]
    fn navigation_tracks_the_focused_table_and_preserves_selection() {
        let cfg = AppConfig::default();
        let mut state = SettingsState::default();
        state.move_selection(-1, 3, 1, 2);
        assert_eq!(state.provider_selected, 0);
        state.cycle_focus(false);
        state.move_selection(10, 3, 1, 2);
        assert_eq!(state.plugin_selected, 2);
        state.focus = SettingsFocus::Markets;
        state.move_selection(1, 3, 1, cfg.markets.len());
        assert_eq!(
            state.selected_market(&cfg),
            cfg.markets.keys().nth(1).map(String::as_str)
        );
        state.cycle_focus(true);
        assert_eq!(state.focus, SettingsFocus::Sources);
    }
    #[test]
    fn diagnostics_follow_breakpoint_until_explicitly_toggled() {
        let mut state = SettingsState::default();
        assert!(!state.diagnostics_expanded(80));
        assert!(state.diagnostics_expanded(120));
        state.toggle_diagnostics(80);
        assert_eq!(state.focus, SettingsFocus::Diagnostics);
        assert!(state.diagnostics_expanded(80));
        state.back();
        assert_eq!(state.focus, SettingsFocus::Provider);
        assert!(!state.diagnostics_expanded(120));
    }
    #[test]
    fn renders_cached_spend_and_market_rows_and_short_viewports() {
        let cfg = AppConfig::default();
        let mut state = SettingsState::default();
        let diagnostics = Ok(SettingsDiagnostics {
            costs: vec![CostRow {
                task: "report".into(),
                model: "test-model".into(),
                calls: 2,
                cost_usd: 0.25,
            }],
            today_usd: 0.10,
            ..Default::default()
        });
        for (w, h) in [(80, 24), (120, 40), (200, 50), (20, 8), (1, 1)] {
            let mut screen = Screen::new(w, h);
            SettingsView {
                config: &cfg,
                state: &state,
                sources: &[],
                diagnostics: &diagnostics,
                provider_connected: false,
            }
            .paint(&mut screen);
            if w >= 100 {
                assert!(text(&screen).contains("today $0.10"));
                assert!(text(&screen).contains("test-model"));
            }
        }
        state.focus = SettingsFocus::Markets;
        let mut screen = Screen::new(80, 24);
        SettingsView {
            config: &cfg,
            state: &state,
            sources: &[],
            diagnostics: &diagnostics,
            provider_connected: false,
        }
        .paint(&mut screen);
        assert!(text(&screen).contains("Australian Securities Exchange"));
    }
}
