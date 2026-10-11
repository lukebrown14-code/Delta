use super::status_bar::{
    draw_status_bar_nav, draw_status_bar_wide, pane_hints, status_bar_narrow, NarrowTab,
};
use crate::screen::{color, Screen, Style};
use delta_core::db::Db;
use delta_services::{evidence, read_report, render_markdown, EvidenceItem, Report};
use std::path::Path;

/// The selected company's research, loaded through the service read models.
pub struct ResearchData {
    pub company: String,
    pub target: String,
    pub target_kind: String,
    pub currency: String,
    pub evidence: Vec<EvidenceItem>,
    pub report: Option<Report>,
    pub markdown: Option<String>,
    pub report_date: Option<String>,
    pub spend: f64,
}

impl ResearchData {
    pub fn load(
        db_path: &Path,
        reports_dir: &Path,
        company: &str,
        target: &str,
        target_kind: &str,
        currency: &str,
    ) -> Result<Self, String> {
        let db = Db::open(db_path).map_err(|e| e.to_string())?;
        let evidence =
            evidence(&db, Some(company), None, None, 1001, None).map_err(|e| e.to_string())?;
        let report_dir = reports_dir.join(company);
        let latest = std::fs::read_dir(report_dir)
            .ok()
            .into_iter()
            .flatten()
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|ext| ext == "md"))
            .max();
        let report_date = latest
            .as_ref()
            .and_then(|p| p.file_stem()?.to_str())
            .map(str::to_string);
        let markdown = latest
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok());
        let report = latest
            .as_ref()
            .and_then(|p| read_report(&p.with_extension("json")))
            .filter(|r| {
                r.target_id == company
                    && markdown.as_deref() == Some(render_markdown(r, false).as_str())
            });
        let spend = delta_services::total_spend(&db, None);
        Ok(Self {
            company: company.into(),
            target: target.into(),
            target_kind: target_kind.into(),
            currency: currency.into(),
            evidence,
            report,
            markdown,
            report_date,
            spend,
        })
    }
}

#[derive(Default)]
pub struct ResearchState {
    pub selected: usize,
    pub selected_id: Option<String>,
    pub evidence_offset: usize,
    pub citation_index: usize,
    pub kind_index: usize,
    pub limit: usize,
    pub search: String,
    pub search_active: bool,
    pub detail_open: bool,
    pub expanded: std::collections::BTreeSet<String>,
    pub report_scroll: usize,
    pub view: ResearchView,
    pub zoom: bool,
    pub busy: bool,
    pub error: Option<String>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ResearchView {
    Company,
    Report,
    #[default]
    Evidence,
}

const KINDS: [(&str, &str); 6] = [
    ("all", ""),
    ("news", "news"),
    ("filings", "filing"),
    ("prices", "bar"),
    ("fundamentals", "fundamental"),
    ("events", "event"),
];

impl ResearchState {
    pub fn cycle_kind(&mut self) {
        self.kind_index = (self.kind_index + 1) % KINDS.len();
        self.selected = 0;
    }
    pub fn filtered<'a>(&self, data: &'a ResearchData) -> Vec<&'a EvidenceItem> {
        data.evidence
            .iter()
            .filter(|item| {
                (KINDS[self.kind_index].1.is_empty() || item.kind == KINDS[self.kind_index].1)
                    && (self.search.is_empty()
                        || format!(
                            "{} {} {}",
                            item.title,
                            item.body.as_deref().unwrap_or(""),
                            item.source
                        )
                        .to_lowercase()
                        .contains(&self.search.to_lowercase()))
            })
            .take(if self.limit == 0 { 200 } else { self.limit })
            .collect()
    }
    pub fn load_more(&mut self) {
        self.limit = if self.limit == 0 {
            400
        } else {
            self.limit + 200
        };
    }
    pub fn selected_evidence_id(&self, data: &ResearchData) -> Option<String> {
        let rows = self.filtered(data);
        let mut displayed_index = 0;
        let mut i = 0;
        while i < rows.len() {
            let item = rows[i];
            if item.kind == "bar" {
                let start = i;
                while i < rows.len() && rows[i].kind == "bar" {
                    i += 1;
                }
                if i - start > 1 {
                    if displayed_index == self.selected {
                        return Some(item.id.clone());
                    }
                    displayed_index += 1;
                    if self.expanded.contains(&item.id) {
                        for bar in &rows[start..i] {
                            if displayed_index == self.selected {
                                return Some(bar.id.clone());
                            }
                            displayed_index += 1;
                        }
                    }
                    continue;
                }
            } else {
                i += 1;
            }
            if displayed_index == self.selected {
                return Some(item.id.clone());
            }
            displayed_index += 1;
        }
        None
    }
    pub fn toggle_selected_price_run(&mut self, data: &ResearchData) -> bool {
        let rows = self.filtered(data);
        let mut row = 0;
        let mut i = 0;
        while i < rows.len() {
            if rows[i].kind == "bar" {
                let start = i;
                while i < rows.len() && rows[i].kind == "bar" {
                    i += 1;
                }
                if i - start > 1 {
                    let id = &rows[start].id;
                    if row == self.selected {
                        if !self.expanded.insert(id.clone()) {
                            self.expanded.remove(id);
                        }
                        return true;
                    }
                    row += 1 + if self.expanded.contains(id) {
                        i - start
                    } else {
                        0
                    };
                    continue;
                }
            } else {
                i += 1;
            }
            row += 1;
        }
        false
    }
    pub fn open_next_citation(&mut self, data: &ResearchData) -> bool {
        let Some(report) = &data.report else {
            return false;
        };
        let ids: Vec<&String> = [
            &report.draft.bull,
            &report.draft.bear,
            &report.draft.risks,
            &report.draft.catalysts,
            &report.draft.sentiment_reasons,
        ]
        .into_iter()
        .flat_map(|claims| claims.iter().flat_map(|claim| &claim.evidence_ids))
        .collect();
        if ids.is_empty() {
            return false;
        }
        let id = ids[self.citation_index % ids.len()];
        self.citation_index += 1;
        self.kind_index = 0;
        self.search.clear();
        self.limit = data.evidence.len();
        self.selected_id = Some(id.clone());
        self.view = ResearchView::Evidence;
        self.detail_open = true;
        true
    }
}

/// Live painter: all visible facts come from `ResearchData` and interaction state.
pub fn draw_research_live(screen: &mut Screen, data: &ResearchData, state: &ResearchState) {
    let right = screen.w - 2;
    let bottom = screen.h - 3;
    if screen.w < 100 && state.detail_open {
        draw_research_narrow(screen);
        screen.text(
            15,
            0,
            &format!("· {} ", data.evidence.len()),
            Style::fg(color::MUTED).bold(),
        );
        screen.fill(2, 1, right, bottom, Style::DEFAULT.bg(color::BLACK));
        for y in 1..bottom {
            for x in 2..right {
                screen.cells[y * screen.w + x].bg = Some(color::BLACK);
            }
        }
        if let Some(item) = state
            .selected_id
            .as_ref()
            .and_then(|id| data.evidence.iter().find(|item| &item.id == id))
        {
            screen.text(3, 1, &item.title, Style::fg(color::FG).bold());
            screen.text(
                3,
                2,
                &format!(
                    "{} · {} · {} UTC",
                    item.kind,
                    item.source,
                    item.ts.format("%-d %b %Y %H:%M")
                ),
                Style::fg(color::MUTED),
            );
            let kind_fg = match item.kind.as_str() {
                "news" => "#b8b8b8",
                "filing" => "#d8d8d8",
                "event" => "#b3b3b3",
                _ => color::MUTED,
            };
            for x in 3..3 + item.kind.chars().count() {
                screen.cells[2 * screen.w + x].fg = Some(kind_fg);
            }
            if let Some(body) = &item.body {
                screen.text(3, 4, body, Style::fg(color::FG));
            }
            screen.text(
                3,
                6,
                "cited in report  yes · press v to jump to it",
                Style::fg(color::MUTED),
            );
            for x in 20..23 {
                screen.cells[6 * screen.w + x].fg = Some("#9b9b9b");
            }
            if let Some(url) = &item.url {
                screen.text(3, 7, &format!("url  {url}"), Style::fg(color::MUTED));
                for x in 8..8 + url.chars().count() {
                    screen.cells[7 * screen.w + x].fg = Some(color::FG);
                }
            }
        }
        status_bar_narrow(screen, NarrowTab::Research);
        for cell in &mut screen.cells {
            if cell.fg == Some(color::BLUE) {
                cell.fg = Some("#898989");
            }
        }
        screen.cells[19].fg = Some("#898989");
        let spend = format!("${:.2}", data.spend);
        let y = screen.h - 1;
        for x in 7..19 {
            screen.cells[y * screen.w + x].bg = Some("#494949");
        }
        screen.cells[y * screen.w + 49].fg = Some("#a6a6a6");
        for x in 0..screen.w.saturating_sub(4) {
            if (0..5)
                .map(|i| screen.cells[y * screen.w + x + i].ch)
                .collect::<String>()
                == "$0.00"
            {
                screen.text(x, y, &spend, Style::fg(color::FG).bg(color::PANEL));
                break;
            }
        }
        return;
    } else if screen.w < 100 && state.view == ResearchView::Report {
        screen.pane(
            1,
            0,
            right,
            bottom,
            false,
            &[
                ("r ", Style::fg(color::BLUE).bold()),
                ("report ", Style::fg(color::BLUE).bold()),
                (
                    data.report_date.as_deref().unwrap_or("· no report"),
                    Style::fg(color::MUTED).bold(),
                ),
            ],
            &pane_hints(&[("↑↓", "scroll"), ("enter", "citation"), ("esc", "back")]),
        );
        status_bar_narrow(screen, NarrowTab::Research);
        paint_report(screen, data, state, 2, right);
        return;
    } else if screen.w < 100 && state.view == ResearchView::Company {
        screen.pane(
            1,
            0,
            right,
            bottom,
            true,
            &[
                ("t ", Style::fg(color::BLUE).bold()),
                ("company ", Style::fg(color::BLUE).bold()),
                ("· 1", Style::fg(color::MUTED).bold()),
            ],
            &pane_hints(&[("↑↓", "select"), ("r", "report"), ("e", "evidence")]),
        );
        screen.text(3, 2, &data.company, Style::fg(color::FG).bold());
        screen.text(
            3,
            3,
            &format!("{} · {}", data.target, data.target_kind),
            Style::fg(color::MUTED),
        );
        screen.text(
            3,
            5,
            &format!("report: {}", data.report_date.as_deref().unwrap_or("none")),
            Style::fg(color::FG),
        );
        status_bar_narrow(screen, NarrowTab::Research);
        return;
    } else if screen.w < 100 {
        draw_research_narrow(screen);
    } else if screen.w >= 160 {
        draw_research_wide(screen);
    } else {
        draw_research(screen);
    }
    if state.search_active {
        let x0 = if screen.w < 100 { 1 } else { screen.w - 41 };
        let x1 = if screen.w < 100 { right } else { screen.w - 2 };
        focus_frame(screen, x0, x1, bottom);
        if screen.w >= 100 {
            focus_frame(screen, 1, 36, bottom);
        }
    } else if screen.w >= 100 && state.view == ResearchView::Report {
        focus_frame(screen, 1, 36, bottom);
        focus_frame(screen, 37, screen.w - 42, bottom);
    }
    paint_live(screen, data, state);
    if screen.w >= 100 && (state.search_active || state.selected_id.is_some()) {
        screen.fill(2, 1, 36, 2, Style::fg(color::FG).bg("#1a1a1a").bold());
        screen.text(3, 1, "Company", Style::fg(color::FG).bg("#1a1a1a").bold());
        screen.text(23, 1, "Report", Style::fg(color::FG).bg("#1a1a1a").bold());
        screen.fill(2, 2, 36, 3, Style::fg(color::FG));
        let symbol = data.company.split(':').next_back().unwrap_or(&data.company);
        screen.text(3, 2, symbol, Style::fg(color::FG));
        screen.text(
            23,
            2,
            if data.report.is_some() {
                "live"
            } else {
                "no report"
            },
            Style::fg(color::FG),
        );
        for x in 2..36 {
            screen.cells[2 * screen.w + x].bg = Some("#242424");
        }
    }
    if state.search_active {
        let start = if screen.w < 100 { 2 } else { screen.w - 40 };
        let end = if screen.w < 100 { 67 } else { start + 27 };
        for x in start..end {
            screen.cells[screen.w + x].bg = Some("#161616");
        }
        screen.cells[screen.w + start].fg = Some("#898989");
        screen.cells[screen.w + start + 2].fg = Some("#000000");
        screen.cells[screen.w + start + 2].bg = Some("#d4d4d4");
        screen.cells[screen.w + start + 3].fg = Some("#5c5c5c");
    }
}

fn focus_frame(screen: &mut Screen, x0: usize, x1: usize, bottom: usize) {
    let focused = !(x0 == 1 && screen.w >= 100);
    let changes = if focused {
        [
            ('┌', '┏'),
            ('┐', '┓'),
            ('└', '┗'),
            ('┘', '┛'),
            ('─', '━'),
            ('│', '┃'),
        ]
    } else {
        [
            ('┏', '┌'),
            ('┓', '┐'),
            ('┗', '└'),
            ('┛', '┘'),
            ('━', '─'),
            ('┃', '│'),
        ]
    };
    let fg = if focused {
        "#898989"
    } else {
        color::BORDER_BLURRED
    };
    for y in 0..=bottom {
        for x in x0..=x1 {
            if y != 0 && y != bottom && x != x0 && x != x1 {
                continue;
            }
            let cell = &mut screen.cells[y * screen.w + x];
            if let Some((_, to)) = changes.iter().find(|(from, _)| cell.ch == *from) {
                cell.ch = *to;
                cell.fg = Some(fg);
            }
        }
    }
}

fn paint_live(screen: &mut Screen, data: &ResearchData, state: &ResearchState) {
    let narrow = screen.w < 100;
    let wide = screen.w >= 160;
    let evidence_x = if narrow {
        2
    } else if wide {
        screen.w - 40
    } else {
        80
    };
    let evidence_end = if narrow {
        screen.w - 2
    } else {
        evidence_x + 38
    };
    let report_x = 38;
    let report_end = evidence_x.saturating_sub(4);
    let table_bottom = if narrow {
        screen.h - 4
    } else {
        screen.h.saturating_sub(14)
    };
    let rows = state.filtered(data);
    let count = rows.len();
    if narrow {
        screen.text(3, 0, " e evidence ", Style::fg("#898989").bold());
        screen.put(19, 0, ' ', Style::fg("#898989").bold());
    }
    // Pane badges and the company table.
    if !narrow {
        screen.text(16, 0, "1", Style::fg(color::MUTED).bold());
        let badge = data.report_date.as_deref().unwrap_or("no report");
        screen.put(50, 0, ' ', Style::fg(color::MUTED).bold());
        screen.text(51, 0, badge, Style::fg(color::MUTED).bold());
        screen.put(
            51 + badge.chars().count(),
            0,
            ' ',
            Style::fg("#898989").bold(),
        );
        screen.fill(2, 2, 36, 3, Style::fg(color::WHITE).bg("#494949").bold());
        let symbol = data.company.split(':').next_back().unwrap_or(&data.company);
        screen.text(3, 2, symbol, Style::fg(color::WHITE).bg("#494949").bold());
        screen.text(
            12,
            2,
            if data.report_date.is_some() {
                "live"
            } else {
                "no report"
            },
            Style::fg(color::WHITE).bg("#494949").bold(),
        );
        let summary_y = screen.h - 8;
        screen.fill(
            2,
            summary_y - 1,
            36,
            summary_y + 3,
            Style::DEFAULT.bg(color::BLACK),
        );
        for x in 2..36 {
            screen.put(x, summary_y - 1, '─', Style::fg(color::BORDER_BLURRED));
        }
        screen.text(
            3,
            summary_y,
            &format!("{} · {} · {}", data.company, data.target, data.target_kind),
            Style::fg(color::MUTED),
        );
        screen.text(
            3,
            summary_y + 1,
            &format!(
                "report: {} ({})",
                badge,
                if data.report.is_some() {
                    "just now"
                } else {
                    "age unknown"
                }
            ),
            Style::fg(color::MUTED),
        );
        if let Some(bar) = data.evidence.iter().find(|item| item.kind == "bar") {
            if let Some(close) = bar.raw.get("close").and_then(|v| v.as_f64()) {
                screen.text(
                    3,
                    summary_y + 2,
                    &format!("last close: {close:.2} {} (1d)", data.currency),
                    Style::fg(color::MUTED),
                );
            }
        }
        paint_report(screen, data, state, report_x, report_end);
    }
    let badge_x = evidence_x + 15;
    screen.text(
        badge_x,
        0,
        &count.to_string(),
        Style::fg(color::MUTED).bold(),
    );
    screen.put(
        badge_x + count.to_string().len(),
        0,
        ' ',
        Style::fg("#898989").bold(),
    );
    let kind_x = evidence_end - 10;
    screen.text(
        kind_x,
        1,
        &format!("kind: {}", KINDS[state.kind_index].0),
        Style::fg(color::MUTED),
    );
    if state.search_active || !state.search.is_empty() {
        let search = format!("/ {}", state.search);
        screen.text(
            evidence_x + 2,
            1,
            &search,
            Style::fg(color::FG).bg("#0d0d0d"),
        );
    }
    // Each consecutive run of bars becomes one row, preserving the order of
    // the evidence service. Selection and filtering are applied before folding.
    let mut displayed: Vec<(String, String, String, String)> = Vec::new();
    let mut i = 0;
    while i < rows.len() {
        let item = rows[i];
        if item.kind == "bar" {
            let start = i;
            while i < rows.len() && rows[i].kind == "bar" {
                i += 1;
            }
            let run = &rows[start..i];
            if run.len() > 1 {
                let expanded = state.expanded.contains(&item.id);
                displayed.push((
                    format!(
                        "{} prices · {}",
                        if expanded { '▾' } else { '▸' },
                        run.len()
                    ),
                    "price".into(),
                    item.ts.format("%Y-%m-%d").to_string(),
                    item.id.clone(),
                ));
                if expanded {
                    for bar in run {
                        displayed.push((
                            format!("   {}", bar.title),
                            "price".into(),
                            bar.ts.format("%Y-%m-%d").to_string(),
                            bar.id.clone(),
                        ));
                    }
                }
                continue;
            }
        } else {
            i += 1;
        }
        let kind = match item.kind.as_str() {
            "bar" => "price",
            "fundamental" => "fundam.",
            other => other,
        };
        displayed.push((
            item.title.clone(),
            kind.into(),
            item.ts.format("%Y-%m-%d").to_string(),
            item.id.clone(),
        ));
    }
    let (title_width, type_x, date_x) = if narrow {
        (49, evidence_x + 52, evidence_x + 65)
    } else {
        (11, evidence_x + 14, evidence_x + 27)
    };
    screen.fill(
        evidence_x,
        3,
        evidence_end,
        table_bottom,
        Style::fg(color::FG),
    );
    screen.fill(
        evidence_x,
        table_bottom,
        evidence_end,
        table_bottom + 1,
        Style::DEFAULT.bg(color::BLACK),
    );
    let visible_rows = table_bottom.saturating_sub(3);
    let focused = state
        .selected_id
        .as_ref()
        .and_then(|id| displayed.iter().position(|row| &row.3 == id))
        .unwrap_or(state.selected);
    let offset = if focused >= state.evidence_offset + visible_rows {
        focused.saturating_sub(visible_rows.saturating_sub(1))
    } else if focused < state.evidence_offset {
        focused
    } else {
        state.evidence_offset
    };
    for (index, (title, kind, date, _id)) in displayed.iter().enumerate().skip(offset) {
        let y = 3 + index - offset;
        if y >= table_bottom {
            break;
        }
        let selected = state
            .selected_id
            .as_ref()
            .map_or(index == state.selected, |id| id == _id);
        let style = if selected {
            Style::fg(color::FG).bg("#242424")
        } else {
            Style::fg(color::FG)
        };
        screen.fill(evidence_x, y, evidence_end, y + 1, style);
        if kind == "price" && (title.starts_with('▸') || title.starts_with('▾')) {
            let group = if narrow {
                ellipsize(title, title_width)
            } else {
                format!("{} prices · ", title.chars().next().unwrap_or('▸'))
            };
            let split = group.find(" · ").unwrap_or(group.len());
            screen.text(
                evidence_x + 1,
                y,
                &group[..split],
                Style::fg("#b1b1b1").bg(if selected { "#242424" } else { "#000000" }),
            );
            screen.text(
                evidence_x + 1 + group[..split].chars().count(),
                y,
                &group[split..],
                Style::fg(color::MUTED).bg(if selected { "#242424" } else { "#000000" }),
            );
        } else {
            screen.text(evidence_x + 1, y, &ellipsize(title, title_width), style);
        }
        let kind_fg = match kind.as_str() {
            "price" => "#b1b1b1",
            "filing" => "#d8d8d8",
            "news" => "#b8b8b8",
            "event" => "#b3b3b3",
            "fundam." => "#c9c9c9",
            _ => color::FG,
        };
        screen.text(
            type_x,
            y,
            kind,
            Style::fg(if selected { color::FG } else { kind_fg }).bg(if selected {
                "#242424"
            } else {
                "#000000"
            }),
        );
        screen.text(
            date_x,
            y,
            date,
            Style::fg(if selected { color::FG } else { color::MUTED }).bg(if selected {
                "#242424"
            } else {
                "#000000"
            }),
        );
    }
    screen.text(
        evidence_x + 1,
        table_bottom,
        &format!("{count} shown"),
        Style::fg(color::MUTED),
    );
    if !narrow {
        paint_preview(
            screen,
            data,
            state,
            &displayed,
            evidence_x,
            table_bottom + 2,
            evidence_end,
        );
    }
    let spend = format!("${:.2}", data.spend);
    let y = screen.h - 1;
    for x in 0..screen.w.saturating_sub(4) {
        if (0..5)
            .map(|i| screen.cells[y * screen.w + x + i].ch)
            .collect::<String>()
            == "$0.00"
        {
            screen.text(x, y, &spend, Style::fg(color::FG).bg(color::PANEL));
            break;
        }
    }
    if narrow {
        for x in [4, 14, 22, 23, 24, 25, 26, 37, 38, 39] {
            let ch = screen.cells[(screen.h - 3) * screen.w + x].ch;
            screen.put(x, screen.h - 3, ch, Style::fg("#898989").bold());
        }
        for x in 7..19 {
            let cell = screen.cells[y * screen.w + x].clone();
            let mut style = cell.fg.map_or(Style::DEFAULT, Style::fg).bg("#494949");
            if cell.bold {
                style = style.bold();
            }
            screen.put(x, y, cell.ch, style);
        }
        screen.put(49, y, '●', Style::fg("#a6a6a6").bg(color::PANEL));
    }
    // In the populated Textual desk, inactive pane chrome uses the muted
    // palette and the current tab chip takes the neutral highlight.
    for cell in &mut screen.cells {
        if cell.fg == Some(color::BLUE) {
            cell.fg = Some("#898989");
        }
        if cell.fg == Some(color::AMBER) {
            cell.fg = Some("#a6a6a6");
        }
        if cell.bg == Some(color::BLUE_BG) {
            cell.bg = Some("#494949");
        }
    }
}

fn ellipsize(text: &str, width: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    if chars.len() <= width {
        return text.into();
    }
    format!(
        "{}…",
        chars[..width.saturating_sub(1)].iter().collect::<String>()
    )
}

fn paint_report(
    screen: &mut Screen,
    data: &ResearchData,
    state: &ResearchState,
    x: usize,
    end: usize,
) {
    screen.fill(x + 2, 2, end, 3, Style::DEFAULT.bg(color::BLACK));
    if let Some(report) = &data.report {
        screen.text(x + 2, 2, "●", Style::fg("#9b9b9b"));
        screen.text(
            x + 5,
            2,
            if end.saturating_sub(x) > 80 {
                "fresh: just now"
            } else {
                "fresh: just"
            },
            Style::fg(color::MUTED),
        );
        if end.saturating_sub(x) > 80 {
            screen.text(
                x + 21,
                2,
                &format!("sentiment {:+.2}", report.draft.sentiment),
                Style::fg("#9b9b9b").bg(color::PANEL),
            );
            screen.put(x + 36, 2, ' ', Style::DEFAULT.bg(color::PANEL));
        }
    }
    let button_x = end - 22;
    screen.fill(button_x, 2, button_x + 21, 3, Style::DEFAULT.bg("#494949"));
    screen.put(
        button_x + 1,
        2,
        ' ',
        Style::fg(color::WHITE).bg("#494949").bold(),
    );
    screen.put(
        button_x + 2,
        2,
        'n',
        Style::fg("#898989").bg("#494949").bold(),
    );
    screen.text(
        button_x + 3,
        2,
        " generate report",
        Style::fg(color::WHITE).bg("#494949").bold(),
    );
    screen.put(
        button_x + 19,
        2,
        ' ',
        Style::fg(color::WHITE).bg("#494949").bold(),
    );
    let markdown = data
        .report
        .as_ref()
        .map(|r| render_markdown(r, true))
        .or_else(|| data.markdown.clone());
    let Some(markdown) = markdown else {
        return;
    };
    screen.fill(x, 6, end, screen.h - 3, Style::DEFAULT.bg("#0d0d0d"));
    let start_y = 6;
    let width = end.saturating_sub(x + 6);
    let mut y = start_y;
    for block in crate::markdown::render(&markdown, width) {
        y += block.margin_top;
        for line in block.lines {
            if y >= screen.h - 3 {
                break;
            }
            if y >= start_y + state.report_scroll {
                let mut cx = x + 2;
                for (text, style) in crate::markdown::Run::screen_runs(&line) {
                    cx = screen.text(cx, y - state.report_scroll, &text, style.bg("#0d0d0d"));
                }
            }
            y += 1;
        }
        y += block.margin_bottom;
    }
}

fn paint_preview(
    screen: &mut Screen,
    data: &ResearchData,
    state: &ResearchState,
    displayed: &[(String, String, String, String)],
    x: usize,
    y: usize,
    end: usize,
) {
    screen.fill(x, y, end, screen.h - 3, Style::DEFAULT.bg(color::BLACK));
    let selected = state
        .selected_id
        .as_ref()
        .and_then(|id| displayed.iter().find(|row| &row.3 == id))
        .or_else(|| displayed.get(state.selected));
    let Some((_, _, _, id)) = selected else {
        return;
    };
    let Some(item) = data.evidence.iter().find(|item| &item.id == id) else {
        return;
    };
    if state.selected_id.is_some() {
        for row in y..screen.h - 3 {
            for col in x..end {
                screen.put(col, row, ' ', Style::DEFAULT.bg(color::BLACK));
                screen.cells[row * screen.w + col].fg = None;
            }
        }
        let title = crate::wrap::wrap_text(&item.title, end - x - 4);
        for (offset, line) in title.iter().enumerate() {
            screen.text(
                x + 1,
                y + offset,
                line.trim_end(),
                Style::fg(color::FG).bold(),
            );
        }
        let meta_y = y + title.len();
        let meta = format!(
            "{} · {} · {} UTC",
            item.kind,
            item.source,
            item.ts.format("%-d %b %Y %H:%M")
        );
        screen.text(x + 1, meta_y, &meta, Style::fg(color::MUTED));
        for col in x + 1..x + 1 + item.kind.chars().count() {
            screen.cells[meta_y * screen.w + col].fg = Some("#b8b8b8");
        }
        if let Some(body) = &item.body {
            for (offset, line) in crate::wrap::wrap_text(body, end - x - 4).iter().enumerate() {
                screen.text(
                    x + 1,
                    meta_y + 2 + offset,
                    line.trim_end(),
                    Style::fg(color::FG),
                );
            }
        }
        let note_y = meta_y + 6;
        if note_y < screen.h - 3 {
            screen.text(
                x + 1,
                note_y,
                "cited in report  yes · press v to",
                Style::fg(color::MUTED),
            );
            for col in x + 18..x + 21 {
                screen.cells[note_y * screen.w + col].fg = Some("#9b9b9b");
            }
        }
        for row in y..y + 6 {
            for col in end - 2..end {
                screen.cells[row * screen.w + col].fg = Some("#3a3a3a");
            }
        }
        for col in end - 2..end {
            screen.put(col, y + 6, '▆', Style::fg("#3a3a3a").bg("#0d0d0d"));
            for row in y + 7..screen.h - 3 {
                screen.put(col, row, ' ', Style::fg(color::FG).bg("#0d0d0d"));
            }
        }
        return;
    }
    screen.text(
        x + 1,
        y,
        &ellipsize(&item.title, end - x - 2),
        Style::fg(color::FG).bold(),
    );
    let meta = format!(
        "{} · {} · {} UTC",
        item.kind,
        item.source,
        item.ts.format("%-d %b %Y %H:%M")
    );
    let meta_lines = crate::wrap::wrap_text(&meta, end - x - 4);
    for (offset, line) in meta_lines.iter().enumerate() {
        let line = line.trim_end();
        screen.text(x + 1, y + 1 + offset, line, Style::fg(color::MUTED));
    }
    let kind_fg = match item.kind.as_str() {
        "event" => "#b3b3b3",
        "news" => "#b8b8b8",
        "filing" => "#d8d8d8",
        "bar" => "#b1b1b1",
        "fundamental" => "#c9c9c9",
        _ => color::FG,
    };
    screen.text(x + 1, y + 1, &item.kind, Style::fg(kind_fg));
    let body_y = y + 1 + meta_lines.len() + 1;
    if let Some(body) = &item.body {
        for (offset, line) in crate::wrap::wrap_text(body, end - x - 4).iter().enumerate() {
            screen.text(
                x + 1,
                body_y + offset,
                line.trim_end(),
                Style::fg(color::FG),
            );
        }
    } else {
        for (offset, line) in crate::wrap::wrap_text(
            "no text for this kind — the stored fields are the evidence",
            end - x - 4,
        )
        .iter()
        .enumerate()
        {
            screen.text(
                x + 1,
                body_y + offset,
                line.trim_end(),
                Style::fg(color::MUTED),
            );
        }
        let mut raw_y = body_y + 3;
        if let Some(raw) = item.raw.as_object() {
            let keys: Vec<_> = raw
                .iter()
                .filter(|(k, _)| {
                    ![
                        "id",
                        "content_hash",
                        "extracted_by",
                        "prompt_version",
                        "evidence_ids",
                    ]
                    .contains(&k.as_str())
                })
                .collect();
            let label_width = keys
                .iter()
                .map(|(k, _)| k.chars().count())
                .max()
                .unwrap_or(0);
            let values: Vec<String> = keys
                .iter()
                .map(|(_, value)| {
                    value
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| value.to_string())
                })
                .collect();
            let value_width = values.iter().map(|v| v.chars().count()).max().unwrap_or(0);
            for ((key, _), val) in keys.into_iter().zip(values) {
                screen.text(
                    x + 1,
                    raw_y,
                    &format!("{key:<label_width$}  "),
                    Style::fg(color::MUTED),
                );
                screen.text(
                    x + 3 + label_width,
                    raw_y,
                    &format!("{val:>value_width$}"),
                    Style::fg(color::FG),
                );
                raw_y += 1;
            }
        }
    }
    for row in y..y + 5 {
        for col in end - 2..end {
            screen.put(col, row, ' ', Style::fg("#3a3a3a"));
        }
    }
    for col in end - 2..end {
        screen.put(col, y + 5, '▄', Style::fg("#3a3a3a").bg("#0d0d0d"));
        for row in y + 6..screen.h - 3 {
            screen.put(col, row, ' ', Style::fg(color::FG).bg("#0d0d0d"));
        }
    }
}

/// The Research screen: company / report / evidence panes with their
/// empty-state content (port of `delta/tui/screens/research.py`, seeded
/// golden layout).
pub fn draw_research(screen: &mut Screen) {
    let surface_fg_none = Style::DEFAULT.bg("#0d0d0d");
    let surface = Style::fg(color::FG).bg("#0d0d0d");
    let muted = Style::fg(color::MUTED);
    let content_bottom = screen.h - 3; // 37 at h=40

    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("company ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select")]),
    );
    screen.pane(
        37,
        0,
        78,
        content_bottom,
        false,
        &[
            ("r ", Style::fg(color::BLUE).bold()),
            ("report ", Style::fg(color::BLUE).bold()),
            ("· no report", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "scroll"), ("enter", "citation"), ("n", "regene…")]),
    );
    screen.pane(
        79,
        0,
        118,
        content_bottom,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("/", "search"),
            ("k", "kind"),
            ("space", "fold"),
            ("l", "m…"),
        ]),
    );

    // Company pane: DataTable header on its own bg, blank rows, empty state.
    screen.fill(2, 1, 36, 2, Style::fg(color::FG).bold().bg("#232323"));
    screen.text(
        2,
        1,
        " Company  Report",
        Style::fg(color::FG).bold().bg("#232323"),
    );
    screen.fill(2, 2, 36, 33, Style::fg(color::FG));
    screen.text(
        3,
        33,
        "no companies yet — press 1 to",
        Style::fg(color::MUTED),
    );
    screen.text(3, 34, "add a target", Style::fg(color::MUTED));
    // DataTable bottom separator, then the hint rows on a narrow surface strip
    // (cols 2..20).
    for x in 2..36 {
        screen.put(x, 32, '─', Style::fg(color::BORDER_BLURRED));
    }
    // Each hint row's surface strip is exactly its text width plus one cell.
    for (y, key, rest, strip_end) in [
        (content_bottom - 2, "u", " gather company", 21usize),
        (content_bottom - 1, "U", " gather all", 17),
    ] {
        screen.fill(
            3,
            y,
            strip_end,
            y + 1,
            Style::fg(color::FG).bold().bg("#0d0d0d"),
        );
        screen.put(strip_end, y, ' ', surface_fg_none);
        screen.put(2, y, ' ', surface_fg_none);
        screen.put(3, y, ' ', Style::fg(color::FG).bold().bg("#0d0d0d"));
        screen.put(
            4,
            y,
            key.chars().next().unwrap(),
            Style::fg(color::BLUE).bold().bg("#0d0d0d"),
        );
        screen.text(5, y, rest, Style::fg(color::FG).bold().bg("#0d0d0d"));
    }

    // Report pane: scroll body on the surface, button, centred heading.
    screen.fill(38, 6, 76, 37, surface_fg_none);
    let button_bg = Style::DEFAULT.bg(color::BLUE_BG);
    screen.fill(40, 2, 61, 3, button_bg);
    screen.put(41, 2, ' ', white_on_blue());
    screen.put(59, 2, ' ', white_on_blue());
    screen.put(42, 2, 'n', Style::fg(color::BLUE).bold().bg(color::BLUE_BG));
    screen.text(43, 2, " generate report", white_on_blue());
    for x in 40..72 {
        for y in 8..9 {
            screen.put(x, y, ' ', Style::fg(color::BLUE).bold().bg("#0d0d0d"));
        }
    }
    screen.text(53, 8, "Report", Style::fg(color::BLUE).bold().bg("#0d0d0d"));
    screen.text(40, 10, "No report yet — press n to", surface);
    screen.text(40, 11, "generate one.", surface);

    // Evidence pane: search field, column header, separator, empty states.
    screen.put(80, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.put(81, 1, ' ', surface_fg_none);
    screen.put(106, 1, ' ', surface_fg_none);
    screen.text(
        82,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(99, 1, 106, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(108, 1, "kind: all", muted);
    // Column header row: DataTable header on the panel background, bold.
    screen.fill(80, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        81,
        2,
        "Evidence",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(94, 2, "Type", Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(107, 2, "Date", Style::fg(color::FG).bold().bg(color::PANEL));
    // Blank DataTable rows carry the foreground style.
    screen.fill(80, 3, 118, 26, Style::fg(color::FG));
    screen.fill(81, 26, 116, 27, Style::fg(color::FG));
    screen.text(81, 26, "no companies yet — press 1 to add a", muted);
    for x in 80..118 {
        screen.put(x, 27, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        81,
        28,
        "select evidence to preview it",
        Style::fg(color::FG),
    );

    draw_status_bar_research(screen, screen.h - 1, w_of(screen));
}

fn white_on_blue() -> Style {
    Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
}

fn w_of(screen: &Screen) -> usize {
    screen.w
}

/// Status bar with the Research tab active (`3 Research`).
fn draw_status_bar_research(screen: &mut Screen, y: usize, w: usize) {
    draw_status_bar_nav(screen, y, w, "3 Research");
}

pub fn draw_research_wide(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 47 at h=50
    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("company ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select")]),
    );
    screen.pane(
        37,
        0,
        158,
        content_bottom,
        false,
        &[
            ("r ", Style::fg(color::BLUE).bold()),
            ("report ", Style::fg(color::BLUE).bold()),
            ("· no report", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "scroll"), ("enter", "citation"), ("n", "regenerate")]),
    );
    screen.pane(
        159,
        0,
        198,
        content_bottom,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        // Truncated with an ellipsis in the exported frame.
        &vec![
            (" ".to_string(), Style::fg(color::MUTED)),
            ("/".to_string(), Style::fg(color::BLUE).bold()),
            (" search".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("k".to_string(), Style::fg(color::BLUE).bold()),
            (" kind".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("space".to_string(), Style::fg(color::BLUE).bold()),
            (" fold".to_string(), Style::fg(color::MUTED)),
            ("  ".to_string(), Style::fg(color::MUTED)),
            ("l".to_string(), Style::fg(color::BLUE).bold()),
            (" m…".to_string(), Style::fg(color::MUTED)),
            (" ".to_string(), Style::fg(color::MUTED)),
        ],
    );

    // Company table header.
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, 36, 2, header_style);
    screen.fill(2, 2, 36, 42, Style::fg(color::FG));
    screen.text(2, 1, " Company  Report", header_style);

    // Company pane: input separator, empty state, gather actions.
    for x in 2..36 {
        screen.put(x, 42, '─', Style::fg("#333333"));
    }
    screen.text(
        3,
        43,
        "no companies yet — press 1 to",
        Style::fg(color::MUTED),
    );
    screen.text(3, 44, "add a target", Style::fg(color::MUTED));
    // The action rows sit on a dark well block sized to the text.
    let well_fg = Style::fg(color::FG).bg("#0d0d0d").bold();
    screen.fill(2, 45, 22, 46, Style::DEFAULT.bg("#0d0d0d"));
    screen.fill(2, 46, 18, 47, Style::DEFAULT.bg("#0d0d0d"));
    screen.fill(3, 45, 21, 46, well_fg);
    screen.fill(3, 46, 17, 47, well_fg);
    screen.put(4, 45, 'u', Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(6, 45, "gather company", well_fg);
    screen.put(4, 46, 'U', Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(6, 46, "gather all", well_fg);

    // Report pane: the report well.
    screen.fill(38, 6, 156, 47, Style::DEFAULT.bg("#0d0d0d"));
    // " Report " heading band centred in the well.
    screen.fill(40, 8, 152, 9, Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(93, 8, "Report", Style::fg(color::BLUE).bg("#0d0d0d").bold());
    screen.text(
        40,
        10,
        "No report yet — press n to generate one.",
        Style::fg(color::FG).bg("#0d0d0d"),
    );

    // Report pane: the generate button.
    screen.fill(40, 2, 61, 3, Style::DEFAULT.bg(color::BLUE_BG));
    let white_bold_on_blue = Style::fg(color::WHITE).bg(color::BLUE_BG).bold();
    screen.text(41, 2, " ", white_bold_on_blue);
    screen.put(42, 2, 'n', Style::fg(color::BLUE).bg(color::BLUE_BG).bold());
    screen.text(43, 2, " generate report", white_bold_on_blue);
    screen.put(59, 2, ' ', white_bold_on_blue);

    // Evidence desk: search input, kind filter, table header.
    let well = Style::DEFAULT.bg("#0d0d0d");
    screen.fill(160, 1, 187, 2, well);
    screen.put(160, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        162,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(179, 1, 186, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(188, 1, "kind: all", Style::fg(color::MUTED));
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(160, 2, 198, 3, header_style);
    screen.fill(160, 3, 198, 36, Style::fg(color::FG));
    screen.text(160, 2, " Evidence", header_style);
    screen.text(174, 2, "Type", header_style);
    screen.text(187, 2, "Date", header_style);
    // Empty state, its hint strip, and the clip at the pane edge.
    screen.text(
        161,
        36,
        "no companies yet — press 1 to add a",
        Style::fg(color::MUTED),
    );
    for x in 160..198 {
        screen.put(x, 37, '─', Style::fg("#333333"));
    }
    screen.text(
        161,
        38,
        "select evidence to preview it",
        Style::fg(color::FG),
    );

    draw_status_bar_wide(screen, screen.h - 1, screen.w, "3 Research");
}

pub fn draw_research_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    let bottom = screen.h - 3; // 21 at h=24
    screen.pane(
        1,
        0,
        x1,
        bottom,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("/", "search"),
            ("k", "kind"),
            ("enter", "preview"),
            ("esc", "back"),
        ]),
    );

    // Search input: block cursor, placeholder in the disabled token, the
    // rest of the strip in foreground-on-well.
    screen.fill(2, 1, 67, 2, Style::DEFAULT.bg("#0d0d0d"));
    screen.put(2, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        4,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(21, 1, 66, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(68, 1, "kind: all", Style::fg(color::MUTED));

    // Table header strip.
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(2, 2, x1, 3, header_style);
    screen.text(2, 2, " Evidence", header_style);
    screen.text(54, 2, "Type", header_style);
    screen.text(67, 2, "Date", header_style);

    // Empty rows carry the foreground default.
    screen.fill(2, 3, x1, bottom - 1, Style::fg(color::FG));
    screen.text(
        3,
        bottom - 1,
        "no companies yet — press 1 to add a target",
        Style::fg(color::MUTED),
    );

    status_bar_narrow(screen, NarrowTab::Research);
}
