use super::pane_layout;
use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};
use crate::{Action, Component};
use crossterm::event::{KeyCode, KeyEvent};
use delta_core::db::Db;
use delta_services::theses::{evidence_for, ThesisEvidence};
use delta_services::thesis_health::ThesisHealth;
use delta_services::{evidence_by_ids, EvidenceItem};
use std::collections::BTreeMap;

/// Read model for the live desk. Reload after each candidate decision so the
/// badge, queue and detail all come from the same accepted-evidence snapshot.
#[derive(Default)]
pub struct ThesesState {
    pub fleet: Vec<ThesisHealth>,
    pub selected: usize,
    pub evidence_selected: usize,
    pub links: Vec<ThesisEvidence>,
    pub items: BTreeMap<String, EvidenceItem>,
    pub pending: BTreeMap<String, usize>,
    pub filter: String,
    pub filtering: bool,
    pub focus_active: bool,
    pub view: ThesesPane,
    pub summary: Option<delta_services::ThesisSummary>,
    pub spend: String,
    pub now: Option<chrono::NaiveDateTime>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ThesesPane {
    #[default]
    Claims,
    Detail,
    Evidence,
}

/// Service and modal requests produced by the Theses screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ThesesCommand {
    New,
    Edit,
    Select,
    Find(String),
    Summarize(String),
    Review {
        thesis_id: String,
        evidence_id: String,
        decision: EvidenceDecision,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceDecision {
    Accept,
    Reject,
    Unaccept,
}

/// Owns the fleet read model and its interaction state. The app supplies DB
/// refreshes and executes the typed service requests returned by keys.
#[derive(Default)]
pub struct ThesesComponent {
    pub state: ThesesState,
    viewport_width: usize,
}

impl ThesesComponent {
    pub fn set_viewport_width(&mut self, width: usize) {
        self.viewport_width = width;
    }

    pub fn paint_into(&self, screen: &mut Screen) {
        draw_theses_live(screen, &self.state);
    }
}

impl crate::Screen for ThesesComponent {
    fn name(&self) -> &'static str {
        "theses"
    }

    fn paint_into(&self, screen: &mut Screen) {
        ThesesComponent::paint_into(self, screen);
    }

    fn handle_key(&mut self, key: KeyEvent) -> Option<Action> {
        let state = &mut self.state;
        state.focus_active = true;
        if state.filtering {
            match key.code {
                KeyCode::Esc | KeyCode::Enter => state.filtering = false,
                KeyCode::Backspace => {
                    state.filter.pop();
                }
                KeyCode::Char(ch) => state.filter.push(ch),
                _ => {}
            }
            return Some(Action::Noop);
        }
        match key.code {
            KeyCode::Esc if state.view != ThesesPane::Claims => {
                state.view = ThesesPane::Claims;
                Some(Action::Noop)
            }
            KeyCode::Char('/') => {
                state.filtering = true;
                state.view = ThesesPane::Claims;
                Some(Action::Noop)
            }
            KeyCode::Char('t') | KeyCode::Enter => {
                state.view = ThesesPane::Detail;
                Some(Action::Noop)
            }
            KeyCode::Char('e') => {
                state.view = ThesesPane::Evidence;
                Some(Action::Noop)
            }
            KeyCode::Char('n') => Some(Action::Theses(ThesesCommand::New)),
            KeyCode::Char('d') if !state.fleet.is_empty() => {
                Some(Action::Theses(ThesesCommand::Edit))
            }
            KeyCode::Up | KeyCode::Down => {
                let delta = if key.code == KeyCode::Up { -1 } else { 1 };
                if state.view == ThesesPane::Evidence {
                    state.evidence_selected = state
                        .evidence_selected
                        .saturating_add_signed(delta)
                        .min(state.links.len().saturating_sub(1));
                    Some(Action::Noop)
                } else {
                    state.selected = state
                        .selected
                        .saturating_add_signed(delta)
                        .min(state.fleet.len().saturating_sub(1));
                    Some(Action::Theses(ThesesCommand::Select))
                }
            }
            KeyCode::Char('f') => Some(state.selected_id().map_or_else(
                || Action::Status("select a thesis first".to_string()),
                |id| Action::Theses(ThesesCommand::Find(id.to_string())),
            )),
            KeyCode::Char('s') => {
                let id = state.selected_id().map(str::to_string);
                state.view = ThesesPane::Detail;
                Some(id.map_or_else(
                    || Action::Status("select a thesis first".to_string()),
                    |id| Action::Theses(ThesesCommand::Summarize(id)),
                ))
            }
            KeyCode::Char('a' | 'x' | 'u') => {
                if self.viewport_width < 100 && state.view != ThesesPane::Evidence {
                    return Some(Action::Status(
                        "press e to review evidence first".to_string(),
                    ));
                }
                let Some(link) = state.selected_link() else {
                    return Some(Action::Status("no evidence to review".to_string()));
                };
                let decision = match key.code {
                    KeyCode::Char('a') if !link.accepted => EvidenceDecision::Accept,
                    KeyCode::Char('u') if link.accepted => EvidenceDecision::Unaccept,
                    KeyCode::Char('x') if !link.accepted => EvidenceDecision::Reject,
                    _ => {
                        return Some(Action::Status(
                            "review that item's current state first".to_string(),
                        ))
                    }
                };
                Some(Action::Theses(ThesesCommand::Review {
                    thesis_id: link.thesis_id.clone(),
                    evidence_id: link.evidence_id.clone(),
                    decision,
                }))
            }
            _ => None,
        }
    }

    fn update(&mut self, action: Action) {
        if let Action::ThesisSummary(summary) = action {
            if self.state.selected_id() == Some(summary.thesis_id.as_str()) {
                self.state.summary = Some(*summary);
            }
        }
    }

    fn draw(&mut self, frame: &mut ratatui::Frame, area: ratatui::layout::Rect) {
        self.viewport_width = area.width as usize;
        let mut screen = Screen::new(area.width as usize, area.height as usize);
        self.paint_into(&mut screen);
        crate::screen::blit(frame, &screen, area);
    }
}

impl Component for ThesesComponent {
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

impl ThesesState {
    pub fn load(&mut self, db: &Db) -> Result<(), delta_services::ServiceError> {
        self.load_at(db, None)
    }

    pub fn load_at(
        &mut self,
        db: &Db,
        now: Option<chrono::NaiveDateTime>,
    ) -> Result<(), delta_services::ServiceError> {
        let selected_id = self
            .fleet
            .get(self.selected)
            .map(|row| row.thesis.id.clone());
        let by_risk = delta_services::thesis_fleet(db, now)?;
        let by_id: BTreeMap<String, ThesisHealth> = by_risk
            .into_iter()
            .map(|row| (row.thesis.id.clone(), row))
            .collect();
        self.fleet = delta_services::list_theses(db)?
            .into_iter()
            .filter_map(|thesis| by_id.get(&thesis.id).cloned())
            .collect();
        self.spend = format!("${:.2}", delta_services::total_spend(db, None));
        self.now = now;
        self.pending.clear();
        for row in &self.fleet {
            let count = evidence_for(db, &row.thesis.id, false)?
                .iter()
                .filter(|link| !link.accepted)
                .count();
            self.pending.insert(row.thesis.id.clone(), count);
        }
        self.selected = selected_id
            .and_then(|id| self.fleet.iter().position(|row| row.thesis.id == id))
            .unwrap_or(0)
            .min(self.fleet.len().saturating_sub(1));
        self.select(db)
    }

    pub fn select(&mut self, db: &Db) -> Result<(), delta_services::ServiceError> {
        self.links = if let Some(row) = self.fleet.get(self.selected) {
            evidence_for(db, &row.thesis.id, false)?
        } else {
            Vec::new()
        };
        self.links.sort_by_key(|row| row.accepted);
        self.items = evidence_by_ids(
            db,
            &self
                .links
                .iter()
                .map(|row| row.evidence_id.clone())
                .collect::<Vec<_>>(),
        )?
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
        self.evidence_selected = self
            .evidence_selected
            .min(self.links.len().saturating_sub(1));
        self.summary = None;
        Ok(())
    }

    pub fn selected_id(&self) -> Option<&str> {
        self.fleet
            .get(self.selected)
            .map(|row| row.thesis.id.as_str())
    }
    pub fn selected_link(&self) -> Option<&ThesisEvidence> {
        self.links.get(self.evidence_selected)
    }
}

fn mark(state: &str) -> &'static str {
    match state {
        "building" => "▲",
        "weakening" => "▼",
        "mixed" => "◆",
        "challenged" => "✕",
        "idle" => "◌",
        _ => "○",
    }
}

/// Textual's compact thesis dialog, seeded with the selected thesis for edit.
pub fn draw_thesis_form_live(screen: &mut Screen, state: &ThesesState, editing: bool) {
    let thesis = editing.then(|| &state.fleet[state.selected].thesis);
    let values = [
        thesis.map_or("Claim".to_string(), |t| t.claim.clone()),
        thesis.map_or("Targets (US:AAPL)".to_string(), |t| t.targets.join(", ")),
        thesis.map_or("Horizon (5y)".to_string(), |t| t.time_horizon.clone()),
        thesis.map_or("Scope (what the claim is about)".to_string(), |t| {
            t.scope.clone()
        }),
        thesis.map_or(
            "Holds if — assumptions, comma separated".to_string(),
            |t| t.assumptions.join(", "),
        ),
        thesis.map_or("Breaks if — what would disprove it".to_string(), |t| {
            t.falsifiers.join(", ")
        }),
    ];

    draw_thesis_form_values(screen, &values, editing, 0, &[!editing; 6], None);
}

/// Render the compact dialog for the interactive form using its current values.
pub fn draw_thesis_form_runtime(screen: &mut Screen, form: &crate::form::Form) {
    let editing = form.title == "edit thesis";
    let values: [String; 6] = std::array::from_fn(|index| {
        let field = &form.fields[index];
        if field.value().is_empty() {
            field.input.placeholder().to_string()
        } else {
            field.value().to_string()
        }
    });
    let placeholders: [bool; 6] =
        std::array::from_fn(|index| form.fields[index].value().is_empty());
    let status = form.fields.get(6).map(|field| field.value());
    draw_thesis_form_values(screen, &values, editing, form.focus, &placeholders, status);
}

fn draw_thesis_form_values(
    screen: &mut Screen,
    values: &[String; 6],
    editing: bool,
    focus: usize,
    placeholders: &[bool; 6],
    status: Option<&str>,
) {
    use crate::dialog::{centred_line, dialog_frame, hint_markup};

    screen.dim();
    let (x, y, width, _) = dialog_frame(screen, 64, 19);
    let title = if editing { "edit thesis" } else { "new thesis" };
    let mut cx = x;
    for (part, style) in centred_line(
        &[(title, Style::fg(color::BLUE).bold())],
        width,
        Style::fg(color::BLUE).bold(),
    ) {
        cx = screen.text(cx, y, &part, style.bg(color::SURFACE));
    }

    let input = |screen: &mut Screen,
                 x: usize,
                 y: usize,
                 width: usize,
                 value: &str,
                 focused: bool,
                 placeholder: bool| {
        let bg = if focused { "#161616" } else { color::SURFACE };
        screen.fill(x, y, x + width, y + 1, Style::fg(color::FG).bg(bg));
        screen.put(
            x,
            y,
            '█',
            Style::fg(if focused {
                color::BLUE
            } else {
                color::BORDER_BLURRED
            })
            .bg(bg),
        );
        screen.put(x + 1, y, ' ', Style::DEFAULT.bg(bg));
        let text_style = Style::fg(if placeholder {
            color::DISABLED
        } else {
            color::FG
        })
        .bg(bg);
        screen.text(
            x + 2,
            y,
            &truncate(value, width.saturating_sub(3)),
            text_style,
        );
        if width > 28 {
            screen.put(x + width - 1, y, ' ', Style::DEFAULT.bg(bg));
        }
        if focused && placeholder {
            if let Some(ch) = value.chars().next() {
                screen.put(x + 2, y, ch, Style::fg(color::BLACK).bg(color::FG));
            }
        }
    };
    input(
        screen,
        x,
        y + 2,
        width,
        &values[0],
        focus == 0,
        placeholders[0],
    );
    if editing && focus == 0 {
        screen.text(x + 2, y + 2, &values[0], Style::fg(color::FG).bg("#263251"));
        if screen.w != 120 {
            screen.put(
                x + 2 + values[0].chars().count(),
                y + 2,
                ' ',
                Style::fg(color::BLACK).bg(color::FG),
            );
        }
    }
    input(
        screen,
        x,
        y + 4,
        28,
        &values[1],
        focus == 1,
        placeholders[1],
    );
    input(
        screen,
        x + 29,
        y + 4,
        width - 29,
        &values[2],
        focus == 2,
        placeholders[2],
    );
    input(
        screen,
        x,
        y + 6,
        if editing { 28 } else { width },
        &values[3],
        focus == 3,
        placeholders[3],
    );
    if editing {
        screen.fill(
            x + 29,
            y + 6,
            x + width,
            y + 7,
            Style::DEFAULT.bg(color::SURFACE),
        );
        screen.put(x + 29, y + 6, '█', Style::fg(color::PANEL).bg(color::PANEL));
        screen.put(x + 30, y + 6, ' ', Style::DEFAULT.bg(color::PANEL));
        screen.text(
            x + 31,
            y + 6,
            status.unwrap_or("Active"),
            Style::fg(color::FG).bg(color::SURFACE),
        );
        screen.put(
            x + width - 2,
            y + 6,
            '▼',
            Style::fg(color::TAB_INACTIVE_FG).bg(color::SURFACE),
        );
        screen.put(x + width - 1, y + 6, ' ', Style::DEFAULT.bg(color::PANEL));
    }
    input(
        screen,
        x,
        y + 8,
        width,
        &values[4],
        focus == 4,
        placeholders[4],
    );
    input(
        screen,
        x,
        y + 10,
        width,
        &values[5],
        focus == 5,
        placeholders[5],
    );
    let button = if editing {
        "Save changes"
    } else {
        "Create thesis"
    };
    screen.fill(
        x,
        y + 12,
        x + button.len() + 4,
        y + 13,
        Style::DEFAULT.bg(color::BLUE_BG),
    );
    screen.text(
        x + 2,
        y + 12,
        button,
        Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
    );
    screen.put(
        x + 1,
        y + 12,
        ' ',
        Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
    );
    screen.put(
        x + button.len() + 2,
        y + 12,
        ' ',
        Style::fg(color::WHITE).bold().bg(color::BLUE_BG),
    );
    let hint = hint_markup(&[("tab", "next field"), ("enter", "save"), ("esc", "cancel")]);
    let len: usize = hint.iter().map(|(part, _)| part.chars().count()).sum();
    screen.fill(
        x,
        y + 14,
        x + width,
        y + 15,
        Style::fg(color::MUTED).bg(color::SURFACE),
    );
    let mut hx = x + (width - len) / 2;
    for (part, style) in hint {
        hx = screen.text(hx, y + 14, &part, style.bg(color::SURFACE));
    }
}

/// Paint the live Theses base from the pane grammar at each breakpoint. The
/// populated read model draws into these same panes below.
fn paint_theses_base(screen: &mut Screen) {
    let bottom = screen.h - 3;
    let narrow = screen.w < 100;
    let wide = screen.w >= 160;
    let columns = if narrow {
        pane_layout(
            1,
            0,
            screen.w - 1,
            bottom + 1,
            &[bottom + 1],
            &[screen.w - 2],
        )
    } else {
        pane_layout(
            1,
            0,
            screen.w - 1,
            bottom + 1,
            &[bottom + 1],
            &[36, screen.w - 78, 40],
        )
    };
    let claims = columns[0][0];
    screen.pane(
        claims.0,
        claims.1,
        claims.2 - 1,
        claims.3 - 1,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(if narrow {
            &[
                ("n", "new"),
                ("d", "edit"),
                ("/", "filter"),
                ("enter", "thesis"),
                ("e", "evidence"),
            ]
        } else {
            &[("n", "new"), ("d", "edit"), ("/", "filter")]
        }),
    );
    if narrow {
        paint_narrow_header(screen, claims, claims.1 + 1, "#232323");
        screen.fill(
            claims.0 + 1,
            claims.1 + 2,
            claims.2 - 1,
            claims.3 - 2,
            Style::fg(color::FG),
        );
        screen.text(
            claims.0 + 2,
            claims.3 - 2,
            "no theses yet — press n to create one",
            Style::fg(color::MUTED),
        );
        status_bar_narrow(screen, NarrowTab::Theses);
        return;
    }
    let detail = columns[0][1];
    let evidence = columns[0][2];
    let wide_title = [("t thesis", Style::fg(color::BLUE).bold())];
    let compact_title = [
        ("t ", Style::fg(color::BLUE).bold()),
        ("thesis", Style::fg(color::BLUE).bold()),
    ];
    screen.pane(
        detail.0,
        detail.1,
        detail.2 - 1,
        detail.3 - 1,
        false,
        if wide { &wide_title } else { &compact_title },
        &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
    );
    screen.pane(
        evidence.0,
        evidence.1,
        evidence.2 - 1,
        evidence.3 - 1,
        false,
        &[
            ("e ", Style::fg(color::BLUE).bold()),
            ("evidence ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("f", "find")]),
    );
    screen.fill(
        claims.0 + 1,
        claims.1 + 1,
        claims.2 - 1,
        claims.3 - 2,
        Style::fg(color::FG),
    );
    screen.text(
        detail.0 + 2,
        detail.1 + 1,
        "no theses yet — press n to create one",
        Style::fg(color::FG),
    );
    screen.text(
        evidence.0 + 2,
        evidence.1 + 1,
        "no evidence yet",
        Style::fg(color::FG),
    );
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(
        evidence.0 + 1,
        evidence.1 + 2,
        evidence.2 - 1,
        evidence.1 + 3,
        header_style,
    );
    screen.fill(
        evidence.0 + 1,
        evidence.1 + 3,
        evidence.2 - 1,
        if wide {
            evidence.3 - 8
        } else {
            evidence.1 + 29
        },
        Style::fg(color::FG),
    );
    screen.text(
        evidence.0 + if wide { 1 } else { 2 },
        evidence.1 + 2,
        if wide {
            "    ±  age   note"
        } else {
            "   ±  age   note"
        },
        header_style,
    );
    let separator = if wide {
        evidence.3 - 9
    } else {
        evidence.1 + 29
    };
    for x in evidence.0 + 1..evidence.2 - 1 {
        screen.put(
            x,
            separator,
            '─',
            Style::fg(if wide {
                "#333333"
            } else {
                color::BORDER_BLURRED
            }),
        );
    }
    screen.text(
        evidence.0 + 2,
        separator + 1,
        "no evidence yet — press f to find",
        Style::fg(color::FG),
    );
    screen.text(
        evidence.0 + 2,
        separator + 2,
        "candidates",
        Style::fg(color::FG),
    );
    if wide {
        draw_status_bar_wide(screen, screen.h - 1, screen.w, "4 Theses");
    } else {
        draw_status_bar_theses(screen, screen.h - 1, screen.w);
    }
}

fn paint_narrow_header(
    screen: &mut Screen,
    pane: super::PaneRect,
    y: usize,
    background: &'static str,
) {
    let style = Style::fg(color::FG).bold().bg(background);
    screen.fill(pane.0 + 1, y, pane.2 - 1, y + 1, style);
    screen.text(pane.0 + 1, y, "   Claim", style);
    screen.text(pane.0 + 48, y, "Health", style);
    screen.text(pane.0 + 61, y, "Tilt", style);
    screen.text(pane.0 + 71, y, "Queue", style);
}

/// Overlay real thesis rows on the layout-driven base.
pub fn draw_theses_live(screen: &mut Screen, state: &ThesesState) {
    if screen.w < 100 && state.view != ThesesPane::Claims {
        draw_theses_narrow_detail(screen, state);
        status_bar_narrow(screen, NarrowTab::Theses);
        paint_spend(screen, &state.spend);
        return;
    }
    paint_theses_base(screen);
    if state.fleet.is_empty() {
        paint_spend(screen, &state.spend);
        return;
    }
    paint_spend(screen, &state.spend);
    if !state.focus_active {
        paint_populated_status(screen);
    }
    let narrow = screen.w < 100;
    let bottom = screen.h - 3;
    let pending_total: usize = state.pending.values().sum();
    let badge = if pending_total == 0 {
        format!("· {}", state.fleet.len())
    } else {
        format!("· {} · {pending_total} pending", state.fleet.len())
    };
    let claims_focused = !state.focus_active
        || state.view == ThesesPane::Claims
        || (state.view == ThesesPane::Detail && screen.w < 160);
    let detail_focused = state.focus_active && state.view == ThesesPane::Detail && screen.w >= 160;
    let evidence_focused = state.focus_active && state.view == ThesesPane::Evidence;
    let evidence_toned = state.focus_active;
    let columns = if narrow {
        pane_layout(
            1,
            0,
            screen.w - 1,
            bottom + 1,
            &[bottom + 1],
            &[screen.w - 2],
        )
    } else {
        pane_layout(
            1,
            0,
            screen.w - 1,
            bottom + 1,
            &[bottom + 1],
            &[36, screen.w - 78, 40],
        )
    };
    let claims = columns[0][0];
    screen.pane(
        claims.0,
        claims.1,
        claims.2 - 1,
        claims.3 - 1,
        claims_focused,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            (&badge, Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(if narrow {
            &[
                ("n", "new"),
                ("d", "edit"),
                ("/", "filter"),
                ("enter", "thesis"),
                ("e", "evidence"),
            ]
        } else {
            &[("n", "new"), ("d", "edit"), ("/", "filter")]
        }),
    );
    if !narrow {
        let detail = columns[0][1];
        let evidence = columns[0][2];
        let health = state.fleet[state.selected].state().as_str();
        screen.pane(
            detail.0,
            detail.1,
            detail.2 - 1,
            detail.3 - 1,
            detail_focused,
            &[
                ("t thesis ", Style::fg(color::BLUE).bold()),
                (
                    &format!("· {} {health}", mark(health)),
                    Style::fg(color::MUTED).bold(),
                ),
            ],
            &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
        );
        let pending = state.links.iter().filter(|link| !link.accepted).count();
        screen.pane(
            evidence.0,
            evidence.1,
            evidence.2 - 1,
            evidence.3 - 1,
            evidence_focused,
            &[
                ("e evidence ", Style::fg(color::BLUE).bold()),
                (
                    &format!("· {pending} pending / {}", state.links.len()),
                    Style::fg(color::MUTED).bold(),
                ),
            ],
            &pane_hints(if state.selected_link().is_some_and(|link| link.accepted) {
                &[("u", "un-accept"), ("f", "find")]
            } else {
                &[("a", "accept"), ("x", "reject"), ("f", "find")]
            }),
        );
    }
    let list_top = if state.filtering {
        if narrow {
            paint_narrow_header(screen, claims, claims.1 + 2, color::PANEL);
        }
        let right = claims.2 - 1;
        screen.fill(
            claims.0 + 1,
            claims.1 + 1,
            right,
            claims.1 + 2,
            Style::fg(color::FG).bg("#161616"),
        );
        screen.put(
            claims.0 + 1,
            claims.1 + 1,
            '█',
            Style::fg(color::BLUE).bg("#161616"),
        );
        screen.put(
            claims.0 + 2,
            claims.1 + 1,
            ' ',
            Style::DEFAULT.bg("#161616"),
        );
        screen.text(
            claims.0 + 3,
            claims.1 + 1,
            &state.filter,
            Style::fg(color::FG).bg("#161616"),
        );
        screen.put(
            claims.0 + 3 + state.filter.chars().count(),
            claims.1 + 1,
            ' ',
            Style::fg(color::BLACK).bg(color::FG),
        );
        screen.put(right - 1, claims.1 + 1, ' ', Style::DEFAULT.bg("#161616"));
        if narrow {
            claims.1 + 3
        } else {
            claims.1 + 2
        }
    } else if narrow {
        claims.1 + 2
    } else {
        claims.1 + 1
    };
    screen.fill(
        claims.0 + 1,
        list_top,
        claims.2 - 1,
        bottom,
        Style::fg(color::FG),
    );
    if !narrow {
        screen.fill(
            columns[0][1].0 + 1,
            columns[0][1].1 + 1,
            columns[0][1].2 - 1,
            bottom,
            Style::DEFAULT,
        );
        screen.fill(
            columns[0][2].0 + 1,
            columns[0][2].1 + 1,
            columns[0][2].2 - 1,
            bottom,
            Style::DEFAULT,
        );
    }
    let rows = state.fleet.iter().filter(|row| {
        row.thesis
            .claim
            .to_lowercase()
            .contains(&state.filter.to_lowercase())
    });
    for (i, row) in rows.enumerate().take(claims.3.saturating_sub(claims.1 + 5)) {
        let y = i + list_top;
        let health = row.state().as_str();
        let glyph = if row.thesis.status == "concluded" {
            "○"
        } else {
            mark(health)
        };
        let selected = row.thesis.id == state.fleet[state.selected].thesis.id;
        let style = if selected && (state.filtering || !claims_focused) {
            Style::fg(color::FG).bg("#13254b")
        } else if selected {
            Style::fg(color::WHITE)
                .bg(if state.focus_active {
                    color::BLUE_BG
                } else {
                    "#494949"
                })
                .bold()
        } else {
            Style::fg(color::FG)
        };
        if selected {
            screen.fill(claims.0 + 1, y, claims.2 - 1, y + 1, style);
        }
        let max = if narrow { 44 } else { 32 };
        let text = format!("{glyph} {}", row.thesis.claim);
        let text = if text.chars().count() > max {
            format!("{}…", text.chars().take(max - 1).collect::<String>())
        } else {
            text
        };
        if selected {
            screen.text(claims.0 + 2, y, &text, style);
        } else {
            screen.text(claims.0 + 2, y, glyph, Style::fg(color::MUTED));
            screen.text(
                claims.0 + 4,
                y,
                text.strip_prefix(&format!("{glyph} ")).unwrap_or(&text),
                style,
            );
        }
        if narrow {
            screen.text(
                claims.0 + 48,
                y,
                health,
                if selected {
                    style
                } else {
                    Style::fg(color::MUTED)
                },
            );
            if let Some(result) = &row.result {
                if health != "emerging" {
                    screen.text(claims.0 + 61, y, &format!("{:+.2}", result.tilt), style);
                }
            }
            if let Some(pending) = state
                .pending
                .get(&row.thesis.id)
                .filter(|count| **count > 0)
            {
                screen.text(
                    claims.0 + 67,
                    y,
                    &format!("{pending} pending"),
                    Style::fg(if state.focus_active {
                        color::AMBER
                    } else {
                        "#a6a6a6"
                    }),
                );
            }
        }
    }
    let Some(row) = state.fleet.get(state.selected) else {
        return;
    };
    screen.fill(
        claims.0 + 1,
        bottom - 1,
        claims.2 - 1,
        bottom,
        Style::DEFAULT,
    );
    screen.text(
        claims.0 + 2,
        bottom - 1,
        &if state.filtering
            || (state.focus_active && state.view == ThesesPane::Detail && screen.w == 120)
        {
            format!("{} theses · of {}", state.fleet.len(), state.fleet.len())
        } else {
            format!("{} theses", state.fleet.len())
        },
        Style::fg(color::MUTED),
    );
    let result = row.result.as_ref();
    let health = row.state().as_str();
    if narrow {
        if !state.focus_active {
            recolor_claims_focus(screen, claims);
        }
        return;
    }
    let detail = columns[0][1];
    let evidence = columns[0][2];
    let detail_x = detail.0 + 2;
    let detail_width = detail.2.saturating_sub(detail_x + 2);
    let mut y = paint_wrapped(
        screen,
        detail_x,
        detail.1 + 1,
        detail_width,
        &row.thesis.claim,
        Style::fg(color::FG).bold(),
    ) + 1;
    let accepted = state.links.iter().filter(|link| link.accepted).count();
    let badge = format!(
        " {} {health}{} ",
        mark(health),
        result.map_or(String::new(), |r| format!(" · tilt {:+.2}", r.tilt))
    );
    screen.fill(
        detail_x,
        y,
        detail_x + badge.chars().count(),
        y + 1,
        Style::fg(color::MUTED).bold().bg(color::PANEL),
    );
    screen.text(
        detail_x,
        y,
        &badge,
        Style::fg(color::MUTED).bold().bg(color::PANEL),
    );
    screen.text(
        detail_x + badge.chars().count(),
        y,
        &if accepted == 0 {
            "  no accepted evidence".to_string()
        } else {
            format!("  {accepted} accepted")
        },
        Style::fg(color::MUTED).bold(),
    );
    y += 1;
    let meta = format!(
        "{}{} · opened {}",
        row.thesis.status,
        if row.thesis.time_horizon.is_empty() {
            String::new()
        } else {
            format!(" · horizon {}", row.thesis.time_horizon)
        },
        row.thesis.created_at.format("%d %b %Y")
    );
    y = paint_wrapped(
        screen,
        detail_x,
        y,
        detail_width,
        &meta,
        Style::fg(color::MUTED),
    );
    if let Some(result) = result {
        if !result.drivers.is_empty() {
            let names = result
                .drivers
                .iter()
                .take(2)
                .map(|id| {
                    state
                        .items
                        .get(id)
                        .map_or(id.as_str(), |item| item.title.as_str())
                })
                .collect::<Vec<_>>()
                .join(", ");
            screen.text(
                detail_x,
                y,
                &truncate(&format!("moved by  {names}"), detail_width),
                Style::fg(color::MUTED),
            );
            y += 1;
        }
    }
    y += 1;
    let targets = if row.thesis.targets.is_empty() {
        "all evidence".to_string()
    } else {
        row.thesis.targets.join(", ")
    };
    y = paint_field(screen, detail_x, y, detail_width, "targets", &targets);
    if !row.thesis.scope.is_empty() {
        y = paint_field(
            screen,
            detail_x,
            y,
            detail_width,
            "scope",
            &row.thesis.scope,
        );
    }
    if !row.thesis.assumptions.is_empty() {
        y = paint_field(
            screen,
            detail_x,
            y,
            detail_width,
            "holds if",
            &row.thesis.assumptions.join("; "),
        );
    }
    if !row.thesis.falsifiers.is_empty() {
        y = paint_field(
            screen,
            detail_x,
            y,
            detail_width,
            "breaks if",
            &row.thesis.falsifiers.join("; "),
        );
    } else {
        y = paint_wrapped(
            screen,
            detail_x,
            y,
            detail_width,
            "breaks if  not set — press d to say what would disprove it",
            Style::fg(color::MUTED),
        );
    }
    y += 1;
    if let Some(summary) = &state.summary {
        let age = state.now.map_or("live".to_string(), |now| {
            let elapsed = now - summary.as_of;
            if elapsed.num_minutes() < 1 {
                "live".to_string()
            } else if elapsed.num_hours() < 1 {
                format!("{}m", elapsed.num_minutes())
            } else if elapsed.num_days() < 1 {
                format!("{}h", elapsed.num_hours())
            } else {
                format!("{}d", elapsed.num_days())
            }
        });
        let heading = format!(
            "summary          {age} old · {} citations",
            summary.citations.len()
        );
        y = paint_wrapped(
            screen,
            detail_x,
            y,
            detail_width,
            &heading,
            Style::fg(color::MUTED).bold(),
        );
        y = paint_wrapped(
            screen,
            detail_x,
            y,
            detail_width,
            &summary.summary,
            Style::fg(color::FG),
        ) + 1;
        for (label, tint, value) in [
            ("support", color::GREEN, summary.strongest_support.as_str()),
            ("counter", color::RED, summary.strongest_counter.as_str()),
        ] {
            if !value.is_empty() {
                let start = y;
                y = paint_wrapped(
                    screen,
                    detail_x,
                    y,
                    detail_width,
                    &format!("{label:<8} {value}"),
                    Style::fg(color::MUTED),
                );
                screen.text(detail_x, start, &format!("{label:<8} "), Style::fg(tint));
            }
        }
        for value in &summary.unknowns {
            let start = y;
            y = paint_wrapped(
                screen,
                detail_x,
                y,
                detail_width,
                &format!("{:<8} {value}", "unknown"),
                Style::fg(color::MUTED),
            );
            screen.text(detail_x, start, "unknown  ", Style::fg(color::AMBER));
        }
    } else {
        screen.text(detail_x, y, "summary", Style::fg(color::MUTED).bold());
        paint_wrapped(
            screen,
            detail_x,
            y + 1,
            detail_width,
            "no summary yet — press s to summarise",
            Style::fg(color::MUTED),
        );
    }
    let evidence_x = evidence.0 + 1;
    let counts = result.map_or((0, 0, 0), |r| (r.support, r.against, r.neutral));
    let separator = evidence.3 - 9;
    screen.fill(
        evidence_x,
        evidence.1 + 3,
        evidence.2 - 1,
        separator,
        Style::fg(color::FG),
    );
    let x = screen.text(
        evidence_x + 1,
        evidence.1 + 1,
        &format!("+{}", counts.0),
        Style::fg(if evidence_toned {
            color::GREEN
        } else {
            "#9b9b9b"
        }),
    );
    let x = screen.text(x, evidence.1 + 1, " support", Style::fg(color::MUTED));
    let x = screen.text(x, evidence.1 + 1, "  ", Style::fg(color::FG));
    let x = screen.text(
        x,
        evidence.1 + 1,
        &format!("−{}", counts.1),
        Style::fg(if evidence_toned {
            color::RED
        } else {
            "#8e8e8e"
        }),
    );
    let x = screen.text(x, evidence.1 + 1, " against", Style::fg(color::MUTED));
    let x = screen.text(x, evidence.1 + 1, "  ", Style::fg(color::FG));
    let x = screen.text(
        x,
        evidence.1 + 1,
        &format!("?{}", counts.2),
        Style::fg(if evidence_toned {
            color::AMBER
        } else {
            "#a6a6a6"
        }),
    );
    screen.text(x, evidence.1 + 1, " neutral", Style::fg(color::MUTED));
    screen.fill(
        evidence_x,
        evidence.1 + 2,
        evidence.2 - 1,
        evidence.1 + 3,
        Style::fg(color::FG).bold().bg(if evidence_focused {
            "#232323"
        } else {
            color::PANEL
        }),
    );
    screen.text(
        evidence_x + 1,
        evidence.1 + 2,
        "   ±  age   note",
        Style::fg(color::FG).bold().bg(if evidence_focused {
            "#232323"
        } else {
            color::PANEL
        }),
    );
    for (i, link) in state
        .links
        .iter()
        .enumerate()
        .take(evidence.3.saturating_sub(evidence.1 + 12))
    {
        let glyph = if link.accepted { "✓" } else { "•" };
        let side = match link.side.as_str() {
            "support" => "+",
            "against" => "−",
            _ => "?",
        };
        let age = state
            .items
            .get(&link.evidence_id)
            .map_or("—".to_string(), |item| {
                let days = (state.now.unwrap_or_else(|| chrono::Utc::now().naive_utc()) - item.ts)
                    .num_days();
                format!("{days}d")
            });
        let style = if i == state.evidence_selected {
            if evidence_focused {
                Style::fg("#ffffff").bg("#264b96").bold()
            } else if state.focus_active {
                Style::fg(color::FG).bg("#13254b")
            } else {
                Style::fg(color::FG).bg("#242424")
            }
        } else {
            Style::fg(color::FG)
        };
        if i == state.evidence_selected {
            screen.fill(
                evidence_x,
                evidence.1 + i + 3,
                evidence.2 - 1,
                evidence.1 + i + 4,
                style,
            );
        }
        let note = if link.note.is_empty() {
            &link.evidence_id
        } else {
            &link.note
        };
        screen.text(
            evidence_x + 1,
            evidence.1 + i + 3,
            &format!("{glyph}  {side}  {age:<4}  {}", truncate(note, 24)),
            style,
        );
    }
    for x in evidence_x..evidence.2 - 1 {
        screen.put(x, separator, '─', Style::fg(color::BORDER_BLURRED));
    }
    if let Some(link) = state.selected_link() {
        let side = match link.side.as_str() {
            "support" => "Supporting",
            "against" => "Against",
            _ => "Neutral",
        };
        let x = screen.text(
            evidence_x + 1,
            separator + 1,
            if link.accepted {
                "accepted"
            } else {
                "candidate"
            },
            Style::fg(if evidence_toned {
                color::GREEN
            } else {
                "#9b9b9b"
            })
            .bold(),
        );
        let x = screen.text(x, separator + 1, " · ", Style::fg(color::MUTED));
        screen.text(
            x,
            separator + 1,
            side,
            Style::fg(if evidence_toned {
                color::GREEN
            } else {
                "#9b9b9b"
            }),
        );
        screen.text(
            evidence_x + 1,
            separator + 2,
            &link.note,
            Style::fg(color::FG),
        );
        if let Some(item) = state.items.get(&link.evidence_id) {
            paint_wrapped(
                screen,
                evidence_x + 1,
                separator + 4,
                evidence.2.saturating_sub(evidence_x + 3),
                &delta_services::cite(item),
                Style::fg(color::MUTED),
            );
        }
    }
    if !state.focus_active {
        recolor_claims_focus(screen, claims);
        recolor_live_pane_chrome(screen, claims.2, evidence.2, bottom);
    }
}

fn recolor_live_pane_chrome(screen: &mut Screen, start: usize, end: usize, bottom: usize) {
    for y in [0, bottom] {
        for x in start..end {
            let cell = &mut screen.cells[y * screen.w + x];
            if cell.fg == Some(color::BLUE) {
                cell.fg = Some("#898989");
            }
        }
    }
}

fn draw_theses_narrow_detail(screen: &mut Screen, state: &ThesesState) {
    let bottom = screen.h - 3;
    let pane = pane_layout(
        1,
        0,
        screen.w - 1,
        bottom + 1,
        &[bottom + 1],
        &[screen.w - 2],
    )[0][0];
    let Some(row) = state.fleet.get(state.selected) else {
        return;
    };
    let evidence = state.view == ThesesPane::Evidence;
    let health = row.state().as_str();
    let result = row.result.as_ref();
    let accepted = state.links.iter().filter(|link| link.accepted).count();
    let pending = state.links.len() - accepted;
    let title = if evidence { "e evidence " } else { "t thesis " };
    let badge = if evidence {
        format!("· {pending} pending / {}", state.links.len())
    } else {
        format!("· {} {health}", mark(health))
    };
    let hints = if evidence {
        pane_hints(&[("u", "un-accept"), ("f", "find"), ("esc", "back")])
    } else {
        pane_hints(&[
            ("s", "summarise"),
            ("d", "edit"),
            ("↑↓", "scroll"),
            ("esc", "back"),
        ])
    };
    screen.pane(
        pane.0,
        pane.1,
        pane.2 - 1,
        pane.3 - 1,
        true,
        &[
            (title, Style::fg(color::BLUE).bold()),
            (&badge, Style::fg(color::MUTED).bold()),
        ],
        &hints,
    );
    let content_x = pane.0 + 2;
    let content_width = pane.2.saturating_sub(content_x + 2);
    screen.fill(pane.0 + 1, pane.1 + 1, pane.2 - 1, bottom, Style::DEFAULT);
    if !evidence {
        let width = content_width;
        let mut y = paint_wrapped(
            screen,
            content_x,
            pane.1 + 1,
            width,
            &row.thesis.claim,
            Style::fg(color::FG).bold(),
        ) + 1;
        let tilt = result.map_or(String::new(), |r| format!(" · tilt {:+.2}", r.tilt));
        screen.text(
            content_x,
            y,
            &format!(" {} {health}{tilt} ", mark(health)),
            Style::fg(color::MUTED).bold().bg(color::PANEL),
        );
        screen.text(
            content_x + 3 + health.len() + tilt.len(),
            y,
            &format!("  {accepted} accepted"),
            Style::fg(color::MUTED).bold(),
        );
        y += 1;
        screen.text(
            content_x,
            y,
            &format!(
                "{} · horizon {} · opened {}",
                row.thesis.status,
                row.thesis.time_horizon,
                row.thesis.created_at.format("%d %b %Y")
            ),
            Style::fg(color::MUTED),
        );
        y += 1;
        if let Some(result) = result {
            if !result.drivers.is_empty() {
                let names = result
                    .drivers
                    .iter()
                    .take(2)
                    .map(|id| {
                        state
                            .items
                            .get(id)
                            .map_or(id.as_str(), |item| item.title.as_str())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                screen.text(
                    content_x,
                    y,
                    &truncate(&format!("moved by  {names}"), width),
                    Style::fg(color::MUTED),
                );
                y += 1;
            }
        }
        y += 1;
        y = paint_field(
            screen,
            content_x,
            y,
            width,
            "targets",
            &row.thesis.targets.join(", "),
        );
        if !row.thesis.scope.is_empty() {
            y = paint_field(screen, content_x, y, width, "scope", &row.thesis.scope);
        }
        if !row.thesis.assumptions.is_empty() {
            y = paint_field(
                screen,
                content_x,
                y,
                width,
                "holds if",
                &row.thesis.assumptions.join("; "),
            );
        }
        if !row.thesis.falsifiers.is_empty() {
            y = paint_field(
                screen,
                content_x,
                y,
                width,
                "breaks if",
                &row.thesis.falsifiers.join("; "),
            );
        }
        y += 1;
        screen.text(content_x, y, "summary", Style::fg(color::MUTED).bold());
        let summary = state
            .summary
            .as_ref()
            .map_or("no summary yet — press s to summarise", |s| {
                s.summary.as_str()
            });
        paint_wrapped(
            screen,
            content_x,
            y + 1,
            width,
            summary,
            Style::fg(color::MUTED),
        );
    } else {
        let (support, against, neutral) =
            result.map_or((0, 0, 0), |r| (r.support, r.against, r.neutral));
        let x = screen.text(
            content_x,
            pane.1 + 1,
            &format!("+{support}"),
            Style::fg(color::GREEN),
        );
        let x = screen.text(x, pane.1 + 1, " support", Style::fg(color::MUTED));
        let x = screen.text(x, pane.1 + 1, "  ", Style::fg(color::FG));
        let x = screen.text(x, pane.1 + 1, &format!("−{against}"), Style::fg(color::RED));
        let x = screen.text(x, pane.1 + 1, " against", Style::fg(color::MUTED));
        let x = screen.text(x, pane.1 + 1, "  ", Style::fg(color::FG));
        let x = screen.text(
            x,
            pane.1 + 1,
            &format!("?{neutral}"),
            Style::fg(color::AMBER),
        );
        screen.text(x, pane.1 + 1, " neutral", Style::fg(color::MUTED));
        screen.fill(
            pane.0 + 1,
            pane.1 + 2,
            pane.2 - 1,
            pane.1 + 3,
            Style::fg(color::FG).bold().bg("#232323"),
        );
        screen.text(
            content_x,
            pane.1 + 2,
            "   ±  age   note",
            Style::fg(color::FG).bold().bg("#232323"),
        );
        let separator = pane.3 - 9;
        screen.fill(
            pane.0 + 1,
            pane.1 + 4,
            pane.2 - 1,
            separator,
            Style::fg(color::FG),
        );
        for (i, link) in state
            .links
            .iter()
            .enumerate()
            .take(bottom.saturating_sub(9))
        {
            let glyph = if link.accepted { "✓" } else { "•" };
            let side = match link.side.as_str() {
                "support" => "+",
                "against" => "−",
                _ => "?",
            };
            let age = state
                .items
                .get(&link.evidence_id)
                .map_or("—".to_string(), |item| {
                    let days = (state.now.unwrap_or_else(|| chrono::Utc::now().naive_utc())
                        - item.ts)
                        .num_days();
                    format!("{days}d")
                });
            let style = if i == state.evidence_selected {
                if state.focus_active {
                    Style::fg("#ffffff").bg("#264b96").bold()
                } else {
                    Style::fg(color::FG).bg("#242424")
                }
            } else {
                Style::fg(color::FG)
            };
            if i == state.evidence_selected {
                screen.fill(
                    pane.0 + 1,
                    pane.1 + i + 3,
                    pane.2 - 1,
                    pane.1 + i + 4,
                    style,
                );
            }
            let note = if link.note.is_empty() {
                &link.evidence_id
            } else {
                &link.note
            };
            screen.text(
                content_x,
                pane.1 + i + 3,
                &format!(
                    "{glyph}  {side}  {age:<4}  {}",
                    truncate(note, pane.2 - pane.0 - 22)
                ),
                style,
            );
        }
        for x in pane.0 + 1..pane.2 - 1 {
            screen.put(x, separator, '─', Style::fg(color::BORDER_BLURRED));
        }
        if let Some(link) = state.selected_link() {
            let side = match link.side.as_str() {
                "support" => "Supporting",
                "against" => "Against",
                _ => "Neutral",
            };
            let acceptance = if link.accepted {
                "accepted"
            } else {
                "candidate"
            };
            let tone = if link.accepted {
                color::GREEN
            } else {
                color::AMBER
            };
            let x = screen.text(content_x, separator + 1, acceptance, Style::fg(tone).bold());
            let x = screen.text(x, separator + 1, " · ", Style::fg(color::MUTED));
            screen.text(x, separator + 1, side, Style::fg(tone));
            screen.text(content_x, separator + 2, &link.note, Style::fg(color::FG));
            if let Some(item) = state.items.get(&link.evidence_id) {
                paint_wrapped(
                    screen,
                    content_x,
                    separator + 4,
                    content_width,
                    &delta_services::cite(item),
                    Style::fg(color::MUTED),
                );
            }
        }
    }
    // Focus colours are retained for the active narrow detail and ledger panes.
}

fn truncate(text: &str, width: usize) -> String {
    if text.chars().count() <= width {
        text.to_string()
    } else {
        format!(
            "{}…",
            text.chars()
                .take(width.saturating_sub(1))
                .collect::<String>()
        )
    }
}

fn paint_field(
    screen: &mut Screen,
    x: usize,
    y: usize,
    width: usize,
    label: &str,
    value: &str,
) -> usize {
    let line = format!("{label:<10}{value}");
    paint_wrapped(screen, x, y, width, &line, Style::fg(color::MUTED))
}

fn paint_wrapped(
    screen: &mut Screen,
    x: usize,
    y: usize,
    width: usize,
    text: &str,
    style: Style,
) -> usize {
    let lines = crate::wrap::wrap_text(text, width);
    for (offset, line) in lines.iter().enumerate() {
        screen.text(x, y + offset, line.trim_end(), style);
    }
    y + lines.len()
}

fn recolor_claims_focus(screen: &mut Screen, pane: super::PaneRect) {
    for y in pane.1..pane.3 {
        for x in pane.0..pane.2 {
            let cell = &mut screen.cells[y * screen.w + x];
            if cell.fg == Some(color::BLUE) {
                cell.fg = Some("#898989");
            }
        }
    }
}

fn paint_spend(screen: &mut Screen, spend: &str) {
    if spend.is_empty() {
        return;
    }
    let y = screen.h - 1;
    let amount = "$0.00";
    if let Some(start) = (0..screen.w.saturating_sub(amount.len())).find(|&x| {
        (0..amount.len())
            .all(|i| screen.cells[y * screen.w + x + i].ch == amount.as_bytes()[i] as char)
    }) {
        for (i, ch) in spend.chars().enumerate().take(amount.len()) {
            screen.cells[y * screen.w + start + i].ch = ch;
        }
    }
}

fn paint_populated_status(screen: &mut Screen) {
    let y = screen.h - 1;
    for x in 0..screen.w {
        let cell = &mut screen.cells[y * screen.w + x];
        if cell.bg == Some(color::BLUE_BG) {
            cell.bg = Some("#494949");
        }
        if cell.ch == '●' {
            cell.fg = Some("#a6a6a6");
        }
    }
}

/// The Theses screen: fleet list, thesis detail, evidence table (all empty
/// states; port of `delta/tui/screens/theses.py`, seeded golden layout).
pub fn draw_theses(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 37 at h=40
    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("n", "new"), ("d", "edit"), ("/", "filter")]),
    );
    screen.pane(
        37,
        0,
        78,
        content_bottom,
        false,
        &[
            ("t ", Style::fg(color::BLUE).bold()),
            ("thesis", Style::fg(color::BLUE).bold()),
        ],
        &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
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
        &pane_hints(&[("f", "find")]),
    );

    screen.fill(2, 1, 36, content_bottom - 1, Style::fg(color::FG));
    screen.text(
        39,
        1,
        "no theses yet — press n to create one",
        Style::fg(color::FG),
    );

    // Evidence table: header row on the panel, blank rows, separator, note.
    screen.fill(80, 2, 118, 3, Style::fg(color::FG).bold().bg(color::PANEL));
    screen.text(
        81,
        2,
        "   ±  age   note",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(81, 1, "no evidence yet", Style::fg(color::FG));
    screen.fill(80, 3, 118, 29, Style::fg(color::FG));
    for x in 80..118 {
        screen.put(x, 29, '─', Style::fg(color::BORDER_BLURRED));
    }
    screen.text(
        81,
        30,
        "no evidence yet — press f to find",
        Style::fg(color::FG),
    );
    screen.text(81, 31, "candidates", Style::fg(color::FG));

    draw_status_bar_theses(screen, screen.h - 1, screen.w);
}

/// Status bar with the Theses tab active (`4 Theses`).
fn draw_status_bar_theses(screen: &mut Screen, y: usize, w: usize) {
    status_bar_tabs(
        screen,
        y,
        w,
        10,
        "4 Theses",
        &[(2, "1"), (5, "2"), (8, "3")],
        &[(21, "5"), (24, "6")],
    );
}

/// Theses at 200x50: thesis list, detail, evidence panes.
pub fn draw_theses_wide(screen: &mut Screen) {
    let content_bottom = screen.h - 3; // 47 at h=50
    let mid_bottom = screen.h - 10; // 40 at h=50
    screen.pane(
        1,
        0,
        36,
        content_bottom,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[("n", "new"), ("d", "edit"), ("/", "filter")]),
    );
    screen.pane(
        37,
        0,
        158,
        content_bottom,
        false,
        &[("t thesis", Style::fg(color::BLUE).bold())],
        &pane_hints(&[("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]),
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
        &pane_hints(&[("f", "find")]),
    );
    screen.fill(2, 1, 36, content_bottom - 1, Style::fg(color::FG));
    screen.text(
        39,
        1,
        "no theses yet — press n to create one",
        Style::fg(color::FG),
    );
    screen.text(161, 1, "no evidence yet", Style::fg(color::FG));
    let header_style = Style::fg(color::FG).bold().bg(color::PANEL);
    screen.fill(160, 2, 198, 3, header_style);
    screen.fill(160, 3, 198, mid_bottom, Style::fg(color::FG));
    screen.text(160, 2, "    ±  age   note", header_style);
    // Input separator above the evidence prompt line.
    for x in 160..198 {
        screen.put(x, mid_bottom - 1, '─', Style::fg("#333333"));
    }
    let fg = Style::fg(color::FG);
    screen.text(161, mid_bottom, "no evidence yet — press f to find", fg);
    screen.text(161, mid_bottom + 1, "candidates", fg);
    draw_status_bar_wide(screen, screen.h - 1, screen.w, "4 Theses");
}

/// Theses at 80x24: one focused full-width pane.
pub fn draw_theses_narrow(screen: &mut Screen) {
    let x1 = screen.w - 2;
    let bottom = screen.h - 3; // 21 at h=24
    screen.pane(
        1,
        0,
        x1,
        bottom,
        true,
        &[
            ("theses ", Style::fg(color::BLUE).bold()),
            ("· 0", Style::fg(color::MUTED).bold()),
        ],
        &pane_hints(&[
            ("n", "new"),
            ("d", "edit"),
            ("/", "filter"),
            ("enter", "thesis"),
            ("e", "evidence"),
        ]),
    );
    let header_style = Style::fg(color::FG).bold().bg("#232323");
    screen.fill(2, 1, x1, 2, header_style);
    screen.text(2, 1, "   Claim", header_style);
    screen.text(49, 1, "Health", header_style);
    screen.text(62, 1, "Tilt", header_style);
    screen.text(72, 1, "Queue", header_style);
    // The focused table's empty rows still carry the foreground default.
    screen.fill(2, 2, x1, bottom - 1, Style::fg(color::FG));
    screen.text(
        3,
        bottom - 1,
        "no theses yet — press n to create one",
        Style::fg(color::MUTED),
    );
    status_bar_narrow(screen, NarrowTab::Theses);
}

#[cfg(test)]
mod component_tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    fn press(component: &mut ThesesComponent, code: KeyCode) -> Option<Action> {
        crate::Screen::handle_key(component, KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn owns_navigation_filter_and_service_requests() {
        let mut component = ThesesComponent::default();
        component.set_viewport_width(80);
        assert_eq!(
            press(&mut component, KeyCode::Char('n')),
            Some(Action::Theses(ThesesCommand::New))
        );
        assert_eq!(
            press(&mut component, KeyCode::Char('e')),
            Some(Action::Noop)
        );
        assert_eq!(component.state.view, ThesesPane::Evidence);
        assert_eq!(press(&mut component, KeyCode::Esc), Some(Action::Noop));
        assert_eq!(component.state.view, ThesesPane::Claims);
        assert_eq!(
            press(&mut component, KeyCode::Char('a')),
            Some(Action::Status(
                "press e to review evidence first".to_string()
            ))
        );
        press(&mut component, KeyCode::Char('/'));
        press(&mut component, KeyCode::Char('A'));
        assert_eq!(component.state.filter, "A");
        press(&mut component, KeyCode::Enter);
        assert!(!component.state.filtering);
        assert_eq!(component.state.view, ThesesPane::Claims);
    }
}
