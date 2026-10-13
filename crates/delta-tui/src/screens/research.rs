use super::status_bar::{
    draw_status_bar_nav, draw_status_bar_wide, pane_hints, status_bar_narrow, NarrowTab,
};
use crate::screen::{color, FrameCanvas, PaintCanvas, Screen, Style};
use crate::theme::Palette;
use crate::{Action, Component};
use crossterm::event::{KeyCode, KeyEvent};
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
    pub preview_expanded: bool,
    pub expanded: std::collections::BTreeSet<String>,
    pub report_scroll: usize,
    pub view: ResearchView,
    pub zoom: bool,
    pub busy: bool,
    pub activity: Option<String>,
    pub error: Option<String>,
}

/// Owns Research data and interaction state. The app supplies refreshed data
/// and executes returned service/browser actions; key and view state stay here.
#[derive(Default)]
pub struct ResearchComponent {
    pub data: Option<ResearchData>,
    pub state: ResearchState,
    viewport_width: usize,
}

impl ResearchComponent {
    pub fn new(data: Option<ResearchData>) -> Self {
        Self {
            data,
            ..Self::default()
        }
    }

    pub fn set_data(&mut self, data: Option<ResearchData>) {
        self.data = data;
    }

    pub fn set_viewport_width(&mut self, width: usize) {
        self.viewport_width = width;
    }

    pub fn paint_into(&self, screen: &mut impl PaintCanvas) {
        match &self.data {
            Some(data) => draw_research_live(screen, data, &self.state),
            None => {
                let layout = ResearchLayout::new(screen, false);
                paint_research_live_frame(screen, layout);
            }
        }
    }
}

impl crate::Screen for ResearchComponent {
    fn name(&self) -> &'static str {
        "reports"
    }

    fn paint_into(&self, screen: &mut Screen) {
        ResearchComponent::paint_into(self, screen);
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let state = &mut self.state;
        if state.search_active {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => state.search_active = false,
                KeyCode::Backspace => {
                    state.search.pop();
                }
                KeyCode::Char(c) => {
                    state.search.push(c);
                    state.selected = 0;
                }
                _ => return None,
            }
            return Some(Action::Noop);
        }
        match key.code {
            KeyCode::Char('n') => Some(Action::GenerateReport),
            KeyCode::Char('u') => self
                .data
                .as_ref()
                .map(|data| Action::GatherCompany(data.company.clone())),
            KeyCode::Char('e') => {
                state.view = ResearchView::Evidence;
                Some(Action::Noop)
            }
            KeyCode::Char('r') => {
                state.view = ResearchView::Report;
                Some(Action::Noop)
            }
            KeyCode::Char('t') => {
                state.view = ResearchView::Company;
                Some(Action::Noop)
            }
            KeyCode::Char('k') => {
                state.cycle_kind();
                state.view = ResearchView::Evidence;
                Some(Action::Noop)
            }
            KeyCode::Char('l') => {
                state.load_more();
                Some(Action::Noop)
            }
            KeyCode::Char('o') => {
                let data = self.data.as_ref()?;
                let id = state
                    .selected_id
                    .clone()
                    .or_else(|| state.selected_evidence_id(data))?;
                let url = data
                    .evidence
                    .iter()
                    .find(|item| item.id == id)?
                    .url
                    .as_ref()?;
                if url.starts_with("https://") || url.starts_with("http://") {
                    Some(Action::OpenSource(url.clone()))
                } else {
                    None
                }
            }
            KeyCode::Char(' ') => {
                if let Some(data) = &self.data {
                    state.toggle_selected_price_run(data);
                }
                Some(Action::Noop)
            }
            KeyCode::Char('/') => {
                state.search_active = true;
                state.view = ResearchView::Evidence;
                Some(Action::Noop)
            }
            KeyCode::Esc => {
                if state.detail_open {
                    state.detail_open = false;
                } else {
                    state.view = ResearchView::Company;
                }
                state.zoom = false;
                Some(Action::Noop)
            }
            KeyCode::Enter if state.view == ResearchView::Report => {
                if let Some(data) = &self.data {
                    state.open_next_citation(data);
                }
                Some(Action::Noop)
            }
            KeyCode::Enter if state.view == ResearchView::Evidence => {
                if let Some(data) = &self.data {
                    state.selected_id = state.selected_evidence_id(data);
                    state.detail_open = state.selected_id.is_some();
                }
                Some(Action::Noop)
            }
            KeyCode::Up if state.view == ResearchView::Report => {
                state.report_scroll = state.report_scroll.saturating_sub(1);
                Some(Action::Noop)
            }
            KeyCode::Down if state.view == ResearchView::Report => {
                state.report_scroll += 1;
                Some(Action::Noop)
            }
            KeyCode::Up => {
                state.selected_id = None;
                state.selected = state.selected.saturating_sub(1);
                Some(Action::Noop)
            }
            KeyCode::Down => {
                state.selected_id = None;
                state.selected += 1;
                Some(Action::Noop)
            }
            KeyCode::Char('z') => {
                state.zoom = !state.zoom;
                Some(Action::Noop)
            }
            KeyCode::Char('v') if state.selected_id.is_some() => {
                if let (Some(data), Some(id)) = (&self.data, state.selected_id.clone()) {
                    state.reveal_claim(data, &id, self.viewport_width);
                }
                Some(Action::Noop)
            }
            _ => None,
        }
    }

    fn update(&mut self, _action: Action) {}

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        self.viewport_width = area.width as usize;
        let mut canvas = FrameCanvas::new(frame, area, Palette::Dark);
        self.paint_into(&mut canvas);
    }
}

impl Component for ResearchComponent {
    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        crate::Screen::handle_key(self, key)
    }

    fn update(&mut self, action: Action) {
        crate::Screen::update(self, action)
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        crate::Screen::draw(self, frame, area)
    }
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
    /// Bring the first claim citing this evidence to the top of the report.
    pub fn reveal_claim(
        &mut self,
        data: &ResearchData,
        evidence_id: &str,
        screen_width: usize,
    ) -> bool {
        let Some(report) = &data.report else {
            return false;
        };
        let claim = [
            &report.draft.bull,
            &report.draft.bear,
            &report.draft.risks,
            &report.draft.catalysts,
            &report.draft.sentiment_reasons,
        ]
        .into_iter()
        .flat_map(|claims| claims.iter())
        .find(|claim| claim.evidence_ids.iter().any(|id| id == evidence_id));
        let Some(claim) = claim else {
            return false;
        };
        let report_width = if screen_width < 100 {
            screen_width.saturating_sub(10)
        } else if screen_width >= 160 {
            screen_width.saturating_sub(88)
        } else {
            32
        };
        let needle: String = claim.text.chars().take(12).collect();
        let markdown = render_markdown(report, true);
        let mut line_number = 0;
        for block in crate::markdown::render(&markdown, report_width) {
            line_number += block.margin_top;
            for line in block.lines {
                let text: String = line.iter().map(|run| run.text.as_str()).collect();
                if text.contains(&needle) {
                    self.report_scroll = line_number;
                    self.view = ResearchView::Report;
                    self.detail_open = false;
                    return true;
                }
                line_number += 1;
            }
            line_number += block.margin_bottom;
        }
        false
    }

    pub fn cycle_kind(&mut self) {
        self.kind_index = (self.kind_index + 1) % KINDS.len();
        self.selected = 0;
        self.preview_expanded = true;
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

#[derive(Clone, Copy)]
struct ResearchLayout {
    full: super::PaneRect,
    company: Option<super::PaneRect>,
    report: Option<super::PaneRect>,
    evidence: super::PaneRect,
}

impl ResearchLayout {
    fn new(screen: &impl PaintCanvas, zoom_evidence: bool) -> Self {
        let bottom = screen.height() - 3;
        let end = screen.width() - 1;
        let full = super::pane_layout(1, 0, end, bottom + 1, &[bottom + 1], &[end - 1])[0][0];
        if zoom_evidence {
            return Self {
                full,
                company: None,
                report: None,
                evidence: super::pane_layout(
                    full.0,
                    full.1,
                    full.0 + 40,
                    full.3,
                    &[full.3 - full.1],
                    &[40],
                )[0][0],
            };
        }
        if screen.width() < 100 {
            return Self {
                full,
                company: None,
                report: None,
                evidence: super::pane_layout(1, 0, end, bottom + 1, &[bottom + 1], &[end - 1])[0]
                    [0],
            };
        }
        let panes = super::pane_layout(
            full.0,
            full.1,
            end,
            bottom + 1,
            &[bottom + 1],
            &[36, end - 1 - 76, 40],
        );
        Self {
            full,
            company: Some(panes[0][0]),
            report: Some(panes[0][1]),
            evidence: panes[0][2],
        }
    }
}

fn paint_research_status(screen: &mut impl PaintCanvas) {
    if screen.width() < 100 {
        status_bar_narrow(screen, NarrowTab::Research);
    } else if screen.width() >= 160 {
        draw_status_bar_wide(screen, screen.height() - 1, screen.width(), "3 Research");
    } else {
        draw_status_bar_research(screen, screen.height() - 1, screen.width());
    }
}

/// Build the live pane frame from layout rectangles. Static golden painters
/// remain as empty-screen compatibility entry points; populated screens never
/// render one and then copy or overlay its cells.
fn paint_research_live_frame(screen: &mut impl PaintCanvas, layout: ResearchLayout) {
    let bottom = layout.full.3 - 1;
    let surface = Style::DEFAULT.bg("#0d0d0d");
    let header = Style::fg(color::FG).bold().bg(color::PANEL);
    if layout.company.is_none() {
        let evidence = layout.evidence;
        let x = evidence.0 + 1;
        let end = evidence.2 - 1;
        screen.pane(
            evidence.0,
            evidence.1,
            end,
            evidence.3 - 1,
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
        screen.fill(x, evidence.1 + 1, end - 11, evidence.1 + 2, surface);
        screen.put(x, evidence.1 + 1, '█', Style::fg("#333333").bg("#0d0d0d"));
        screen.text(
            x + 2,
            evidence.1 + 1,
            "/ search evidence",
            Style::fg(color::DISABLED).bg("#0d0d0d"),
        );
        screen.fill(
            x + 19,
            evidence.1 + 1,
            end - 12,
            evidence.1 + 2,
            Style::fg(color::FG).bg("#0d0d0d"),
        );
        screen.text(
            end - 10,
            evidence.1 + 1,
            "kind: all",
            Style::fg(color::MUTED),
        );
        screen.fill(x, evidence.1 + 2, end, evidence.1 + 3, header);
        screen.text(x, evidence.1 + 2, " Evidence", header);
        screen.text(end - 24, evidence.1 + 2, "Type", header);
        screen.text(end - 11, evidence.1 + 2, "Date", header);
        screen.fill(x, evidence.1 + 3, end, evidence.3 - 2, Style::fg(color::FG));
        screen.text(
            x + 1,
            evidence.3 - 2,
            "no companies yet — press 1 to add a target",
            Style::fg(color::MUTED),
        );
        paint_research_status(screen);
        return;
    }
    let wide = screen.width() >= 160;
    let company = layout.company.expect("wide Research company pane");
    let report = layout.report.expect("wide Research report pane");
    let evidence = layout.evidence;
    let cx = company.0 + 1;
    let cr = company.2 - 1;
    let rx = report.0 + 1;
    let rr = report.2 - 3;
    let ex = evidence.0 + 1;
    let er = evidence.2 - 1;
    let company_header = Style::fg(color::FG).bold().bg("#232323");
    screen.pane(
        company.0,
        company.1,
        cr,
        bottom,
        true,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("company ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("↑↓", "select")]),
    );
    screen.pane(
        report.0,
        report.1,
        report.2 - 1,
        bottom,
        false,
        &[
            ("r ", Style::fg(color::BLUE).bold()),
            ("report ", Style::fg(color::BLUE).bold()),
            ("· no report", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("↑↓", "scroll"),
            ("enter", "citation"),
            ("n", if wide { "regenerate" } else { "regene…" }),
        ]),
    );
    let evidence_hints = if wide {
        vec![
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
        ]
    } else {
        pane_hints(&[
            ("/", "search"),
            ("k", "kind"),
            ("space", "fold"),
            ("l", "m…"),
        ])
    };
    screen.pane(
        evidence.0,
        evidence.1,
        er,
        bottom,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &evidence_hints,
    );
    screen.fill(cx, company.1 + 1, cr, company.1 + 2, company_header);
    screen.text(cx, company.1 + 1, " Company  Report", company_header);
    screen.fill(
        cx,
        company.1 + 2,
        cr,
        bottom - if wide { 5 } else { 4 },
        Style::fg(color::FG),
    );
    for x in cx..cr {
        screen.put(x, bottom - 5, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        cx + 1,
        bottom - 4,
        "no companies yet — press 1 to",
        Style::fg(color::MUTED),
    );
    screen.text(cx + 1, bottom - 3, "add a target", Style::fg(color::MUTED));
    let well = Style::fg(color::FG).bold().bg("#0d0d0d");
    for (offset, key, label, length) in [(2, 'u', "gather company", 18), (1, 'U', "gather all", 14)]
    {
        let y = bottom - offset;
        screen.fill(cx, y, cx + length + 2, y + 1, surface);
        screen.fill(cx + 1, y, cx + length + 1, y + 1, well);
        screen.put(cx + 2, y, key, Style::fg(color::BLUE).bold().bg("#0d0d0d"));
        screen.text(
            cx + if wide { 4 } else { 3 },
            y,
            if wide {
                label.to_string()
            } else {
                format!(" {label}")
            }
            .as_str(),
            well,
        );
    }
    if !wide {
        for y in [bottom - 2, bottom - 1] {
            screen.put(cx, y, ' ', surface);
            screen.put(cx + 1, y, ' ', well);
        }
    }
    screen.fill(rx, report.1 + 6, rr, bottom, surface);
    screen.fill(
        rx + 2,
        report.1 + 2,
        rx + 23,
        report.1 + 3,
        Style::DEFAULT.bg(color::BLUE_BG),
    );
    screen.put(rx + 3, report.1 + 2, ' ', white_on_blue());
    screen.put(
        rx + 4,
        report.1 + 2,
        'n',
        Style::fg(color::BLUE).bold().bg(color::BLUE_BG),
    );
    screen.text(rx + 5, report.1 + 2, " generate report", white_on_blue());
    screen.put(rx + 21, report.1 + 2, ' ', white_on_blue());
    let report_band = Style::fg(color::BLUE).bold().bg("#0d0d0d");
    screen.fill(rx + 2, report.1 + 8, rr - 4, report.1 + 9, report_band);
    screen.text(
        (report.0 + report.2) / 2 - 5,
        report.1 + 8,
        "Report",
        report_band,
    );
    if wide {
        screen.text(
            rx + 2,
            report.1 + 10,
            "No report yet — press n to generate one.",
            Style::fg(color::FG).bg("#0d0d0d"),
        );
    } else {
        screen.text(
            rx + 2,
            report.1 + 10,
            "No report yet — press n to",
            Style::fg(color::FG).bg("#0d0d0d"),
        );
        screen.text(
            rx + 2,
            report.1 + 11,
            "generate one.",
            Style::fg(color::FG).bg("#0d0d0d"),
        );
    }
    screen.fill(ex, evidence.1 + 1, ex + 27, evidence.1 + 2, surface);
    screen.put(ex, evidence.1 + 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        ex + 2,
        evidence.1 + 1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(
        ex + 19,
        evidence.1 + 1,
        ex + 26,
        evidence.1 + 2,
        Style::fg(color::FG).bg("#0d0d0d"),
    );
    screen.text(
        ex + 28,
        evidence.1 + 1,
        "kind: all",
        Style::fg(color::MUTED),
    );
    screen.fill(ex, evidence.1 + 2, er, evidence.1 + 3, header);
    screen.text(
        ex + usize::from(!wide),
        evidence.1 + 2,
        if wide { " Evidence" } else { "Evidence" },
        header,
    );
    screen.text(ex + 14, evidence.1 + 2, "Type", header);
    screen.text(ex + 27, evidence.1 + 2, "Date", header);
    screen.fill(
        ex,
        evidence.1 + 3,
        er,
        evidence.3 - 12,
        Style::fg(color::FG),
    );
    screen.text(
        ex + 1,
        evidence.3 - 12,
        "no companies yet — press 1 to add a",
        Style::fg(color::MUTED),
    );
    for x in ex..er {
        screen.put(x, evidence.3 - 11, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        ex + 1,
        evidence.3 - 10,
        "select evidence to preview it",
        Style::fg(color::FG),
    );
    paint_research_status(screen);
}

/// Live painter: all visible facts come from `ResearchData` and interaction state.
pub fn draw_research_live(
    screen: &mut impl PaintCanvas,
    data: &ResearchData,
    state: &ResearchState,
) {
    let layout = ResearchLayout::new(screen, false);
    let right = layout.full.2 - 1;
    let bottom = layout.full.3 - 1;
    if state.zoom && state.view == ResearchView::Report {
        paint_research_status(screen);
        let hints = if screen.width() < 100 {
            pane_hints(&[
                ("↑↓", "scroll"),
                ("t", "company"),
                ("e", "evidence"),
                ("esc", "back"),
            ])
        } else {
            pane_hints(&[("z", "restore"), ("↑↓", "scroll"), ("enter", "citation")])
        };
        screen.pane(
            layout.full.0,
            layout.full.1,
            right,
            bottom,
            true,
            &[
                ("r ", Style::fg(color::BLUE).bold()),
                ("report ", Style::fg(color::BLUE).bold()),
                ("· ", Style::fg(color::MUTED).bold()),
                (
                    data.report_date.as_deref().unwrap_or("no report"),
                    Style::fg(color::MUTED).bold(),
                ),
            ],
            &hints,
        );
        paint_report(screen, data, state, layout.full, true);
        for row in 6..bottom {
            for col in right - 2..right {
                screen.put(col, row, ' ', Style::DEFAULT.bg(color::BLACK));
            }
        }
        paint_zoom_footer(screen, data);
        return;
    }
    if screen.width() < 100 && state.detail_open {
        paint_research_live_frame(screen, layout);
        screen.text(
            layout.evidence.0 + 14,
            layout.evidence.1,
            &format!("· {} ", data.evidence.len()),
            Style::fg(color::MUTED).bold(),
        );
        screen.fill(
            layout.full.0 + 1,
            layout.full.1 + 1,
            right,
            bottom,
            Style::DEFAULT.bg(color::BLACK),
        );
        for y in layout.full.1 + 1..bottom {
            for x in layout.full.0 + 1..right {
                screen.edit(y * screen.width() + x, |cell| cell.bg = Some(color::BLACK));
            }
        }
        if let Some(item) = state
            .selected_id
            .as_ref()
            .and_then(|id| data.evidence.iter().find(|item| &item.id == id))
        {
            screen.text(
                layout.full.0 + 2,
                layout.full.1 + 1,
                &item.title,
                Style::fg(color::FG).bold(),
            );
            screen.text(
                layout.full.0 + 2,
                layout.full.1 + 2,
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
            for x in layout.full.0 + 2..layout.full.0 + 2 + item.kind.chars().count() {
                screen.edit((layout.full.1 + 2) * screen.width() + x, |cell| {
                    cell.fg = Some(kind_fg)
                });
            }
            if let Some(body) = &item.body {
                screen.text(
                    layout.full.0 + 2,
                    layout.full.1 + 4,
                    body,
                    Style::fg(color::FG),
                );
            }
            screen.text(
                layout.full.0 + 2,
                layout.full.1 + 6,
                "cited in report  yes · press v to jump to it",
                Style::fg(color::MUTED),
            );
            for x in layout.full.0 + 19..layout.full.0 + 22 {
                screen.edit((layout.full.1 + 6) * screen.width() + x, |cell| {
                    cell.fg = Some("#9b9b9b")
                });
            }
            if let Some(url) = &item.url {
                screen.text(
                    layout.full.0 + 2,
                    layout.full.1 + 7,
                    &format!("url  {url}"),
                    Style::fg(color::MUTED),
                );
                for x in layout.full.0 + 7..layout.full.0 + 7 + url.chars().count() {
                    screen.edit((layout.full.1 + 7) * screen.width() + x, |cell| {
                        cell.fg = Some(color::FG)
                    });
                }
            }
        }
        status_bar_narrow(screen, NarrowTab::Research);
        for index in 0..screen.width() * screen.height() {
            screen.edit(index, |cell| {
                if cell.fg == Some(color::BLUE) {
                    cell.fg = Some("#898989");
                }
            });
        }
        screen.edit(layout.evidence.0 + 18, |cell| cell.fg = Some("#898989"));
        let spend = format!("${:.2}", data.spend);
        let y = screen.height() - 1;
        for x in 7..19 {
            screen.edit(y * screen.width() + x, |cell| cell.bg = Some("#494949"));
        }
        screen.edit(y * screen.width() + 49, |cell| cell.fg = Some("#a6a6a6"));
        for x in 0..screen.width().saturating_sub(4) {
            if (0..5)
                .map(|i| screen.read(y * screen.width() + x + i).ch)
                .collect::<String>()
                == "$0.00"
            {
                screen.text(x, y, &spend, Style::fg(color::FG).bg(color::PANEL));
                break;
            }
        }
        return;
    } else if screen.width() >= 100 && state.zoom && state.view == ResearchView::Evidence {
        let zoom_layout = ResearchLayout::new(screen, true);
        paint_research_status(screen);
        paint_zoom_evidence_frame(screen, zoom_layout.evidence);
        paint_live(screen, data, state, zoom_layout);
        let count = state.filtered(data).len().to_string();
        screen.pane(
            zoom_layout.evidence.0,
            zoom_layout.evidence.1,
            zoom_layout.evidence.2 - 1,
            bottom,
            true,
            &[
                ("e ", Style::fg(color::BLUE).bold()),
                ("evidence ", Style::fg(color::BLUE).bold()),
                ("· ", Style::fg(color::MUTED).bold()),
                (&count, Style::fg(color::MUTED).bold()),
            ],
            &pane_hints(&[("z", "restore"), ("/", "search"), ("k", "kind")]),
        );
        for x in zoom_layout.evidence.0 + 1..zoom_layout.evidence.2 - 1 {
            screen.edit((zoom_layout.evidence.1 + 2) * screen.width() + x, |cell| {
                cell.bg = Some("#232323")
            });
            screen.edit(
                (zoom_layout.evidence.1 + 3) * screen.width() + x,
                |selected| {
                    selected.fg = Some(color::WHITE);
                    selected.bg = Some(color::BLUE_BG);
                    selected.bold = true;
                },
            );
        }
        return;
    } else if screen.width() < 100 && state.view == ResearchView::Report {
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
        paint_report(screen, data, state, layout.full, true);
        return;
    } else if screen.width() < 100 && state.view == ResearchView::Company {
        screen.pane(
            layout.full.0,
            layout.full.1,
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
        screen.text(
            layout.full.0 + 2,
            layout.full.1 + 2,
            &data.company,
            Style::fg(color::FG).bold(),
        );
        screen.text(
            layout.full.0 + 2,
            layout.full.1 + 3,
            &format!("{} · {}", data.target, data.target_kind),
            Style::fg(color::MUTED),
        );
        screen.text(
            layout.full.0 + 2,
            layout.full.1 + 5,
            &format!("report: {}", data.report_date.as_deref().unwrap_or("none")),
            Style::fg(color::FG),
        );
        status_bar_narrow(screen, NarrowTab::Research);
        return;
    } else {
        paint_research_live_frame(screen, layout);
    }
    if state.search_active {
        focus_frame(screen, layout.evidence.0, layout.evidence.2 - 1, bottom);
        if let Some(company) = layout.company {
            focus_frame(screen, company.0, company.2 - 1, bottom);
        }
    } else if screen.width() >= 100 && state.view == ResearchView::Report {
        let company = layout.company.expect("Research company pane");
        let report = layout.report.expect("Research report pane");
        focus_frame(screen, company.0, company.2 - 1, bottom);
        focus_frame(screen, report.0, report.2 - 1, bottom);
    }
    paint_live(screen, data, state, layout);
    if screen.width() >= 100 && (state.search_active || state.selected_id.is_some()) {
        let company = layout.company.expect("Research company pane");
        let x = company.0 + 1;
        let end = company.2 - 1;
        screen.fill(
            x,
            company.1 + 1,
            end,
            company.1 + 2,
            Style::fg(color::FG).bg("#1a1a1a").bold(),
        );
        screen.text(
            x + 1,
            company.1 + 1,
            "Company",
            Style::fg(color::FG).bg("#1a1a1a").bold(),
        );
        screen.text(
            x + 21,
            company.1 + 1,
            "Report",
            Style::fg(color::FG).bg("#1a1a1a").bold(),
        );
        screen.fill(x, company.1 + 2, end, company.1 + 3, Style::fg(color::FG));
        let symbol = data.company.split(':').next_back().unwrap_or(&data.company);
        screen.text(x + 1, company.1 + 2, symbol, Style::fg(color::FG));
        screen.text(
            x + 21,
            company.1 + 2,
            if data.report.is_some() {
                "live"
            } else {
                "no report"
            },
            Style::fg(color::FG),
        );
        for col in x..end {
            screen.edit((company.1 + 2) * screen.width() + col, |cell| {
                cell.bg = Some("#242424")
            });
        }
    }
    if state.search_active {
        let start = layout.evidence.0 + 1;
        let end = if screen.width() < 100 {
            layout.evidence.2 - 12
        } else {
            start + 27
        };
        for x in start..end {
            screen.edit((layout.evidence.1 + 1) * screen.width() + x, |cell| {
                cell.bg = Some("#161616")
            });
        }
        let search_row = (layout.evidence.1 + 1) * screen.width();
        screen.edit(search_row + start, |cell| cell.fg = Some("#898989"));
        screen.edit(search_row + start + 2, |cell| cell.fg = Some("#000000"));
        screen.edit(search_row + start + 2, |cell| cell.bg = Some("#d4d4d4"));
        screen.edit(search_row + start + 3, |cell| cell.fg = Some("#5c5c5c"));
    }
    if state.busy && screen.width() >= 100 {
        paint_research_busy(
            screen,
            state,
            layout.company.expect("Research company pane"),
        );
    }
    if screen.width() < 100 && state.zoom && state.view == ResearchView::Evidence {
        let count = state.filtered(data).len().to_string();
        for x in layout.evidence.0 + 1..layout.evidence.2 - 1 {
            screen.edit((layout.evidence.1 + 2) * screen.width() + x, |cell| {
                cell.bg = Some("#232323")
            });
            screen.edit((layout.evidence.1 + 3) * screen.width() + x, |selected| {
                selected.fg = Some(color::WHITE);
                selected.bg = Some(color::BLUE_BG);
                selected.bold = true;
            });
        }
        screen.pane(
            layout.full.0,
            layout.full.1,
            right,
            bottom,
            true,
            &[
                ("e ", Style::fg(color::BLUE).bold()),
                ("evidence ", Style::fg(color::BLUE).bold()),
                ("· ", Style::fg(color::MUTED).bold()),
                (&count, Style::fg(color::MUTED).bold()),
            ],
            &pane_hints(&[
                ("/", "search"),
                ("k", "kind"),
                ("enter", "preview"),
                ("esc", "back"),
            ]),
        );
    } else if screen.width() >= 100 && state.zoom && state.view == ResearchView::Company {
        let company = layout.company.expect("Research company pane");
        screen.fill(
            layout.full.0.saturating_sub(1),
            layout.full.1,
            company.0,
            bottom + 1,
            Style::DEFAULT.bg(color::BLACK),
        );
        screen.fill(
            company.2,
            company.1,
            layout.full.2 + 1,
            bottom + 1,
            Style::DEFAULT.bg(color::BLACK),
        );
        screen.pane(
            company.0,
            company.1,
            company.2 - 1,
            bottom,
            true,
            &[
                ("t ", Style::fg(color::BLUE).bold()),
                ("company ", Style::fg(color::BLUE).bold()),
                ("· 1", Style::fg(color::MUTED).bold()),
            ],
            &pane_hints(&[("z", "restore"), ("↑↓", "select")]),
        );
    }
}

fn paint_research_busy(
    screen: &mut impl PaintCanvas,
    state: &ResearchState,
    company: super::PaneRect,
) {
    let bottom = company.3 - 1;
    let x = company.0;
    for (row, end) in [(bottom - 2, x + 20), (bottom - 1, x + 16)] {
        screen.edit(row * screen.width() + x + 2, |cell| {
            cell.fg = Some("#3c3c3c")
        });
        screen.edit(row * screen.width() + x + 3, |cell| {
            cell.fg = Some("#3b5994")
        });
        for x in x + 4..end {
            screen.edit(row * screen.width() + x, |cell| cell.fg = Some("#3c3c3c"));
        }
    }
    for x in x + 2..company.2 - 1 {
        screen.put(x, bottom, '━', Style::fg(color::BLUE));
    }
    screen.put(x + 2, bottom, ' ', Style::fg("#8a8a8a"));
    if let Some(activity) = &state.activity {
        screen.text(
            x + 3,
            bottom,
            &ellipsize(activity, 30),
            Style::fg(color::AMBER),
        );
    }
    screen.put(company.2 - 3, bottom, ' ', Style::fg("#8a8a8a"));
}

fn paint_zoom_evidence_frame(screen: &mut impl PaintCanvas, pane: super::PaneRect) {
    let bottom = screen.height() - 3;
    let x = pane.0 + 1;
    let end = pane.2 - 1;
    let header = Style::fg(color::FG).bold().bg(color::PANEL);
    let surface = Style::DEFAULT.bg("#0d0d0d");
    screen.pane(
        pane.0,
        pane.1,
        end,
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
            ("space", "fold"),
            ("l", "m…"),
        ]),
    );
    screen.fill(x, 1, x + 27, 2, surface);
    screen.put(x, 1, '█', Style::fg("#333333").bg("#0d0d0d"));
    screen.text(
        x + 2,
        1,
        "/ search evidence",
        Style::fg(color::DISABLED).bg("#0d0d0d"),
    );
    screen.fill(x + 19, 1, x + 26, 2, Style::fg(color::FG).bg("#0d0d0d"));
    screen.text(x + 28, 1, "kind: all", Style::fg(color::MUTED));
    screen.fill(x, 2, end, 3, header);
    screen.text(
        x + usize::from(screen.width() < 160),
        2,
        if screen.width() >= 160 {
            " Evidence"
        } else {
            "Evidence"
        },
        header,
    );
    screen.text(x + 14, 2, "Type", header);
    screen.text(x + 27, 2, "Date", header);
    screen.fill(x, 3, end, screen.height() - 14, Style::fg(color::FG));
    screen.text(
        x + 1,
        screen.height() - 14,
        "no companies yet — press 1 to add a",
        Style::fg(color::MUTED),
    );
    for col in x..end {
        screen.put(
            col,
            screen.height() - 13,
            '─',
            Style::fg(color::BORDER_BLURRED),
        );
    }
    screen.text(
        x + 1,
        screen.height() - 12,
        "select evidence to preview it",
        Style::fg(color::FG),
    );
}

fn paint_zoom_footer(screen: &mut impl PaintCanvas, data: &ResearchData) {
    let spend = format!("${:.2}", data.spend);
    let y = screen.height() - 1;
    for x in 0..screen.width().saturating_sub(4) {
        if (0..5)
            .map(|i| screen.read(y * screen.width() + x + i).ch)
            .collect::<String>()
            == "$0.00"
        {
            screen.text(x, y, &spend, Style::fg(color::FG).bg(color::PANEL));
            break;
        }
    }
}

fn focus_frame(screen: &mut impl PaintCanvas, x0: usize, x1: usize, bottom: usize) {
    let focused = !(x0 == 1 && screen.width() >= 100);
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
            screen.edit(y * screen.width() + x, |cell| {
                if let Some((_, to)) = changes.iter().find(|(from, _)| cell.ch == *from) {
                    cell.ch = *to;
                    cell.fg = Some(fg);
                }
            });
        }
    }
}

fn paint_live(
    screen: &mut impl PaintCanvas,
    data: &ResearchData,
    state: &ResearchState,
    layout: ResearchLayout,
) {
    let narrow = screen.width() < 100;
    let muted = data.report.is_some()
        && state.kind_index == 0
        && !state.zoom
        && !state.busy
        && state.error.is_none();
    let evidence_x = layout.evidence.0 + 1;
    let evidence_end = layout.evidence.2 - 1;
    let table_bottom = if narrow {
        layout.evidence.3 - 2
    } else {
        layout.evidence.3 - 12
    };
    let rows = state.filtered(data);
    let count = rows.len();
    if narrow {
        let title_fg = if muted { "#898989" } else { color::BLUE };
        screen.text(
            layout.evidence.0 + 2,
            layout.evidence.1,
            " e evidence ",
            Style::fg(title_fg).bold(),
        );
    }
    // Pane badges and the company table.
    if let (Some(company), Some(report)) = (layout.company, layout.report) {
        let cx = company.0 + 1;
        let cr = company.2 - 1;
        screen.text(
            company.0 + 15,
            company.1,
            "1",
            Style::fg(color::MUTED).bold(),
        );
        let badge = data.report_date.as_deref().unwrap_or("no report");
        screen.put(report.0 + 13, report.1, ' ', Style::fg(color::MUTED).bold());
        screen.text(
            report.0 + 14,
            report.1,
            badge,
            Style::fg(color::MUTED).bold(),
        );
        screen.put(
            report.0 + 14 + badge.chars().count(),
            report.1,
            ' ',
            Style::fg(if muted { "#898989" } else { color::BLUE }).bold(),
        );
        let selected_bg = if muted { "#494949" } else { color::BLUE_BG };
        screen.fill(
            cx,
            company.1 + 2,
            cr,
            company.1 + 3,
            Style::fg(color::WHITE).bg(selected_bg).bold(),
        );
        let symbol = data.company.split(':').next_back().unwrap_or(&data.company);
        screen.text(
            cx + 1,
            company.1 + 2,
            symbol,
            Style::fg(color::WHITE).bg(selected_bg).bold(),
        );
        screen.text(
            cx + 10,
            company.1 + 2,
            if data.report_date.is_some() {
                "live"
            } else {
                "no report"
            },
            Style::fg(color::WHITE).bg(selected_bg).bold(),
        );
        let summary_y = company.3 - 6;
        screen.fill(
            cx,
            summary_y - 1,
            cr,
            summary_y + 3,
            Style::DEFAULT.bg(color::BLACK),
        );
        for x in cx..cr {
            screen.put(x, summary_y - 1, '─', Style::fg(color::BORDER_BLURRED));
        }
        screen.text(
            cx + 1,
            summary_y,
            &format!("{} · {} · {}", data.company, data.target, data.target_kind),
            Style::fg(color::MUTED),
        );
        let report_line = if data.report_date.is_some() {
            format!("report: {badge} (just now)")
        } else {
            "report: none".to_string()
        };
        screen.text(cx + 1, summary_y + 1, &report_line, Style::fg(color::MUTED));
        if let Some(bar) = data.evidence.iter().find(|item| item.kind == "bar") {
            if let Some(close) = bar.raw.get("close").and_then(|v| v.as_f64()) {
                screen.text(
                    cx + 1,
                    summary_y + 2,
                    &format!("last close: {close:.2} {} (1d)", data.currency),
                    Style::fg(color::MUTED),
                );
            }
        }
        paint_report(screen, data, state, report, false);
    }
    let badge_x = evidence_x + 15;
    screen.text(
        badge_x,
        layout.evidence.1,
        &count.to_string(),
        Style::fg(color::MUTED).bold(),
    );
    screen.put(
        badge_x + count.to_string().len(),
        layout.evidence.1,
        ' ',
        Style::fg(if !muted { color::BLUE } else { "#898989" }).bold(),
    );
    let kind_label = format!("kind: {}", KINDS[state.kind_index].0);
    let kind_x = evidence_end - kind_label.chars().count() - 1;
    screen.text(
        kind_x,
        layout.evidence.1 + 1,
        &kind_label,
        Style::fg(color::MUTED),
    );
    if state.kind_index != 0 {
        screen.put(
            kind_x - 2,
            layout.evidence.1 + 1,
            ' ',
            Style::DEFAULT.bg("#0d0d0d"),
        );
        screen.put(
            kind_x - 1,
            layout.evidence.1 + 1,
            ' ',
            Style::DEFAULT.bg(color::BLACK),
        );
    }
    if state.search_active || !state.search.is_empty() {
        let search = format!("/ {}", state.search);
        screen.text(
            evidence_x + 2,
            layout.evidence.1 + 1,
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
        layout.evidence.1 + 3,
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
    let visible_rows = table_bottom.saturating_sub(layout.evidence.1 + 3);
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
        let y = layout.evidence.1 + 3 + index - offset;
        if y >= table_bottom {
            break;
        }
        let selected = state
            .selected_id
            .as_ref()
            .map_or(index == state.selected, |id| id == _id);
        let selected_bg = if muted { "#242424" } else { "#13254b" };
        let style = if selected {
            Style::fg(color::FG).bg(selected_bg)
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
                Style::fg(if muted { "#b1b1b1" } else { "#a8b2c8" }).bg(if selected {
                    selected_bg
                } else {
                    "#000000"
                }),
            );
            screen.text(
                evidence_x + 1 + group[..split].chars().count(),
                y,
                &group[split..],
                Style::fg(color::MUTED).bg(if selected { selected_bg } else { "#000000" }),
            );
        } else {
            screen.text(evidence_x + 1, y, &ellipsize(title, title_width), style);
        }
        let kind_fg = match (muted, kind.as_str()) {
            (true, "price") => "#b1b1b1",
            (true, "filing") => "#d8d8d8",
            (true, "news") => "#b8b8b8",
            (true, "event") => "#b3b3b3",
            (true, "fundam.") => "#c9c9c9",
            (false, "price") => "#a8b2c8",
            (false, "filing") => "#ffd580",
            (false, "news") => "#5ccfe6",
            (false, "event") => "#d7a1ff",
            (false, "fundam.") => "#7ee0c0",
            _ => color::FG,
        };
        screen.text(
            type_x,
            y,
            kind,
            Style::fg(if selected { color::FG } else { kind_fg }).bg(if selected {
                selected_bg
            } else {
                "#000000"
            }),
        );
        screen.text(
            date_x,
            y,
            date,
            Style::fg(if selected { color::FG } else { color::MUTED }).bg(if selected {
                selected_bg
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
            layout.evidence,
            table_bottom + 2,
        );
    }
    let spend = format!("${:.2}", data.spend);
    let y = screen.height() - 1;
    for x in 0..screen.width().saturating_sub(4) {
        if (0..5)
            .map(|i| screen.read(y * screen.width() + x + i).ch)
            .collect::<String>()
            == "$0.00"
        {
            screen.text(x, y, &spend, Style::fg(color::FG).bg(color::PANEL));
            break;
        }
    }
    if narrow && muted {
        for x in [4, 14, 22, 23, 24, 25, 26, 37, 38, 39] {
            let ch = screen.read((screen.height() - 3) * screen.width() + x).ch;
            screen.put(x, screen.height() - 3, ch, Style::fg("#898989").bold());
        }
        for x in 7..19 {
            let cell = screen.read(y * screen.width() + x);
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
    if muted {
        for index in 0..screen.width() * screen.height() {
            screen.edit(index, |cell| {
                if cell.fg == Some(color::BLUE) {
                    cell.fg = Some("#898989");
                }
                if cell.fg == Some(color::AMBER) {
                    cell.fg = Some("#a6a6a6");
                }
                if cell.bg == Some(color::BLUE_BG) {
                    cell.bg = Some("#494949");
                }
            });
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
    screen: &mut impl PaintCanvas,
    data: &ResearchData,
    state: &ResearchState,
    pane: super::PaneRect,
    full_width: bool,
) {
    let x = pane.0 + 1;
    let end = pane.2 - if full_width { 1 } else { 3 };
    let bottom = pane.3 - 1;
    if data.report.is_none() && data.markdown.is_none() {
        return;
    }
    let muted = data.report.is_some()
        && state.kind_index == 0
        && !state.zoom
        && !state.busy
        && state.error.is_none();
    screen.fill(
        x + 2,
        pane.1 + 2,
        end,
        pane.1 + 3,
        Style::DEFAULT.bg(color::BLACK),
    );
    if let Some(report) = &data.report {
        screen.text(
            x + 2,
            pane.1 + 2,
            "●",
            Style::fg(if muted { "#9b9b9b" } else { color::GREEN }),
        );
        screen.text(
            x + 5,
            pane.1 + 2,
            if state.zoom || end.saturating_sub(x) > 80 {
                "fresh: just now"
            } else {
                "fresh: just"
            },
            Style::fg(color::MUTED),
        );
        if state.zoom || end.saturating_sub(x) > 80 {
            screen.text(
                x + 21,
                pane.1 + 2,
                &format!("sentiment {:+.2}", report.draft.sentiment),
                Style::fg(if muted { "#9b9b9b" } else { color::GREEN }).bg(color::PANEL),
            );
            screen.put(x + 36, pane.1 + 2, ' ', Style::DEFAULT.bg(color::PANEL));
        }
    }
    let button_x = end - if state.zoom { 24 } else { 22 };
    let button_bg = if muted { "#494949" } else { color::BLUE_BG };
    let button_fg = if state.busy { "#a8b7d5" } else { color::WHITE };
    let key_fg = if state.busy {
        "#4572cb"
    } else if muted {
        "#898989"
    } else {
        color::BLUE
    };
    screen.fill(
        button_x,
        pane.1 + 2,
        button_x + 21,
        pane.1 + 3,
        Style::DEFAULT.bg(button_bg),
    );
    screen.put(
        button_x + 1,
        pane.1 + 2,
        ' ',
        Style::fg(button_fg).bg(button_bg).bold(),
    );
    screen.put(
        button_x + 2,
        pane.1 + 2,
        'n',
        Style::fg(key_fg).bg(button_bg).bold(),
    );
    screen.text(
        button_x + 3,
        pane.1 + 2,
        " generate report",
        Style::fg(button_fg).bg(button_bg).bold(),
    );
    screen.put(
        button_x + 19,
        pane.1 + 2,
        ' ',
        Style::fg(button_fg).bg(button_bg).bold(),
    );
    let markdown = data
        .report
        .as_ref()
        .map(|r| render_markdown(r, true))
        .or_else(|| data.markdown.clone());
    let Some(markdown) = markdown else {
        return;
    };
    screen.fill(x, pane.1 + 6, end, bottom, Style::DEFAULT.bg("#0d0d0d"));
    let start_y = pane.1 + 6;
    let width = end.saturating_sub(x + 6);
    let mut y = start_y;
    for block in crate::markdown::render(&markdown, width) {
        y += block.margin_top;
        for line in block.lines {
            if y.saturating_sub(state.report_scroll) >= bottom {
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
    if state.zoom {
        let thumb_y = match screen.height() {
            24 => 8,
            40 => 17,
            50 => 26,
            height => 6 + height.saturating_sub(24) / 2,
        };
        let thumb = if screen.height() == 40 { '▆' } else { '▄' };
        for col in end - 4..end - 2 {
            for row in 6..thumb_y {
                screen.put(col, row, ' ', Style::fg("#3a3a3a").bg("#0d0d0d"));
            }
            screen.put(col, thumb_y, thumb, Style::fg("#3a3a3a").bg("#0d0d0d"));
            for row in thumb_y + 1..bottom {
                screen.put(col, row, ' ', Style::fg(color::FG).bg("#0d0d0d"));
            }
        }
    }
}

fn paint_preview(
    screen: &mut impl PaintCanvas,
    data: &ResearchData,
    state: &ResearchState,
    displayed: &[(String, String, String, String)],
    pane: super::PaneRect,
    y: usize,
) {
    let x = pane.0 + 1;
    let end = pane.2 - 1;
    let bottom = pane.3 - 1;
    screen.fill(x, y, end, bottom, Style::DEFAULT.bg(color::BLACK));
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
    if state.selected_id.is_some() || state.preview_expanded {
        for row in y..screen.height() - 3 {
            for col in x..end {
                screen.put(col, row, ' ', Style::DEFAULT.bg(color::BLACK));
                screen.edit(row * screen.width() + col, |cell| cell.fg = None);
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
        let kind_fg = if data.report.is_some()
            && state.kind_index == 0
            && !state.zoom
            && !state.busy
            && state.error.is_none()
        {
            "#b8b8b8"
        } else {
            "#5ccfe6"
        };
        for col in x + 1..x + 1 + item.kind.chars().count() {
            screen.edit(meta_y * screen.width() + col, |cell| {
                cell.fg = Some(kind_fg)
            });
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
        if note_y < screen.height() - 3 {
            screen.text(
                x + 1,
                note_y,
                "cited in report  yes · press v to",
                Style::fg(color::MUTED),
            );
            for col in x + 18..x + 21 {
                screen.edit(note_y * screen.width() + col, |cell| {
                    cell.fg = Some(
                        if data.report.is_some()
                            && state.kind_index == 0
                            && !state.zoom
                            && !state.busy
                            && state.error.is_none()
                        {
                            "#9b9b9b"
                        } else {
                            color::GREEN
                        },
                    )
                });
            }
        }
        for row in y..y + 6 {
            for col in end - 2..end {
                screen.edit(row * screen.width() + col, |cell| cell.fg = Some("#3a3a3a"));
            }
        }
        for col in end - 2..end {
            screen.put(col, y + 6, '▆', Style::fg("#3a3a3a").bg("#0d0d0d"));
            for row in y + 7..screen.height() - 3 {
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
    let muted = data.report.is_some()
        && state.kind_index == 0
        && !state.zoom
        && !state.busy
        && state.error.is_none();
    let kind_fg = match (muted, item.kind.as_str()) {
        (true, "event") => "#b3b3b3",
        (true, "news") => "#b8b8b8",
        (true, "filing") => "#d8d8d8",
        (true, "bar") => "#b1b1b1",
        (true, "fundamental") => "#c9c9c9",
        (false, "event") => "#d7a1ff",
        (false, "news") => "#5ccfe6",
        (false, "filing") => "#ffd580",
        (false, "bar") => "#a8b2c8",
        (false, "fundamental") => "#7ee0c0",
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
    let thumb_start = y + if data.report.is_some() { 5 } else { 6 };
    for row in y..thumb_start {
        for col in end - 2..end {
            screen.put(col, row, ' ', Style::fg("#3a3a3a"));
        }
    }
    for col in end - 2..end {
        screen.put(
            col,
            thumb_start,
            if data.report.is_some() { '▄' } else { '▆' },
            Style::fg("#3a3a3a").bg("#0d0d0d"),
        );
        for row in thumb_start + 1..screen.height() - 3 {
            screen.put(col, row, ' ', Style::fg(color::FG).bg("#0d0d0d"));
        }
    }
}

/// The Research screen: company / report / evidence panes with their
/// empty-state content (port of `delta/tui/screens/research.py`, seeded
/// golden layout).
pub fn draw_research(screen: &mut impl PaintCanvas) {
    let surface_fg_none = Style::DEFAULT.bg("#0d0d0d");
    let surface = Style::fg(color::FG).bg("#0d0d0d");
    let muted = Style::fg(color::MUTED);
    let content_bottom = screen.height() - 3; // 37 at h=40

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

    draw_status_bar_research(screen, screen.height() - 1, w_of(screen));
}

fn white_on_blue() -> Style {
    Style::fg(color::WHITE).bg(color::BLUE_BG).bold()
}

fn w_of(screen: &impl PaintCanvas) -> usize {
    screen.width()
}

/// Status bar with the Research tab active (`3 Research`).
fn draw_status_bar_research(screen: &mut impl PaintCanvas, y: usize, w: usize) {
    draw_status_bar_nav(screen, y, w, "3 Research");
}

pub fn draw_research_wide(screen: &mut impl PaintCanvas) {
    let content_bottom = screen.height() - 3; // 47 at h=50
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

    draw_status_bar_wide(screen, screen.height() - 1, screen.width(), "3 Research");
}

pub fn draw_research_narrow(screen: &mut impl PaintCanvas) {
    let x1 = screen.width() - 2;
    let bottom = screen.height() - 3; // 21 at h=24
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

#[cfg(test)]
mod component_frame_tests {
    use super::*;
    use ratatui::{backend::TestBackend, layout::Rect, Terminal};

    #[test]
    fn direct_frame_matches_oracle_for_live_views() {
        for mode in [
            "empty", "live", "search", "zoom", "busy", "evidence", "detail",
        ] {
            for (width, height) in [(80, 24), (120, 40), (200, 50)] {
                let mut research = ResearchComponent::default();
                if mode != "empty" {
                    research.data = Some(ResearchData {
                        company: "apple".into(),
                        target: "US:AAPL".into(),
                        target_kind: "company".into(),
                        currency: "USD".into(),
                        evidence: if matches!(mode, "evidence" | "detail") {
                            vec![EvidenceItem {
                                id: "e1".into(),
                                target_ids: vec!["US:AAPL".into()],
                                ts: chrono::NaiveDate::from_ymd_opt(2026, 9, 21)
                                    .unwrap()
                                    .and_hms_opt(0, 0, 0)
                                    .unwrap(),
                                kind: "news".into(),
                                title: "Product demand rises".into(),
                                body: Some("Demand held through summer.".into()),
                                source: "wire".into(),
                                url: Some("https://example.com/e1".into()),
                                sentiment: None,
                                raw: serde_json::Value::Null,
                                quality: "high".into(),
                            }]
                        } else {
                            Vec::new()
                        },
                        report: None,
                        markdown: None,
                        report_date: None,
                        spend: 0.0,
                    });
                }
                research.state.search_active = mode == "search";
                research.state.zoom = mode == "zoom";
                research.state.busy = mode == "busy";
                research.state.detail_open = mode == "detail";
                research.state.selected_id = (mode == "detail").then(|| "e1".into());
                research.state.activity = (mode == "busy").then(|| "gathering".into());
                let area = Rect::new(1, 1, width, height);
                let mut oracle = Screen::new(width as usize, height as usize);
                research.paint_into(&mut oracle);
                let mut expected = Terminal::new(TestBackend::new(width + 2, height + 2)).unwrap();
                expected
                    .draw(|frame| crate::screen::blit(frame, &oracle, area))
                    .unwrap();
                let mut actual = Terminal::new(TestBackend::new(width + 2, height + 2)).unwrap();
                actual
                    .draw(|frame| crate::Screen::draw(&mut research, frame, area))
                    .unwrap();
                let got = actual.backend().buffer();
                let want = expected.backend().buffer();
                if let Some((index, (got, want))) = got
                    .content()
                    .iter()
                    .zip(want.content())
                    .enumerate()
                    .find(|(_, (got, want))| got != want)
                {
                    panic!(
                        "{mode} {width}x{height} ({}, {}): got {got:?}, want {want:?}",
                        index % (width as usize + 2),
                        index / (width as usize + 2)
                    );
                }
            }
        }
    }

    #[test]
    fn empty_component_matches_the_static_oracle_frame() {
        for (w, h) in [(80, 24), (120, 40), (200, 50)] {
            let mut expected = Screen::new(w, h);
            match w {
                80 => draw_research_narrow(&mut expected),
                200 => draw_research_wide(&mut expected),
                _ => draw_research(&mut expected),
            }
            let mut actual = Screen::new(w, h);
            ResearchComponent::default().paint_into(&mut actual);
            if let Some((index, (got, want))) = actual
                .cells
                .iter()
                .zip(&expected.cells)
                .enumerate()
                .find(|(_, (got, want))| got != want)
            {
                panic!(
                    "empty Research at {w}x{h} ({}, {}): got {got:?}, want {want:?}",
                    index % w,
                    index / w
                );
            }
        }
    }
}
