use super::status_bar::{
    draw_status_bar_wide, pane_hints, status_bar_narrow, status_bar_tabs, NarrowTab,
};
use crate::screen::{color, Screen, Style};
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

/// Overlay real thesis rows on the three golden-layout painters. Empty states
/// retain the exact existing golden output.
pub fn draw_theses_live(screen: &mut Screen, state: &ThesesState) {
    if screen.w < 100 && state.view != ThesesPane::Claims {
        draw_theses_narrow_detail(screen, state);
        paint_spend(screen, &state.spend);
        return;
    }
    if screen.w < 100 {
        draw_theses_narrow(screen);
    } else if screen.w >= 160 {
        draw_theses_wide(screen);
    } else {
        draw_theses(screen);
    }
    if state.fleet.is_empty() {
        return;
    }
    paint_spend(screen, &state.spend);
    paint_populated_status(screen);
    let narrow = screen.w < 100;
    let bottom = screen.h - 3;
    let pending_total: usize = state.pending.values().sum();
    let badge = if pending_total == 0 {
        format!("· {}", state.fleet.len())
    } else {
        format!("· {} · {pending_total} pending", state.fleet.len())
    };
    screen.pane(
        1,
        0,
        if narrow { screen.w - 2 } else { 36 },
        bottom,
        true,
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
        let x = screen.w - 42;
        let health = state.fleet[state.selected].state().as_str();
        screen.pane(
            37,
            0,
            x,
            bottom,
            false,
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
            x + 1,
            0,
            screen.w - 2,
            bottom,
            false,
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
    if narrow {
        screen.fill(2, 2, screen.w - 2, bottom, Style::fg(color::FG));
        if state.filtering {
            screen.text(
                3,
                bottom - 2,
                &format!("/ {}", state.filter),
                Style::fg(color::MUTED),
            );
        }
    } else {
        screen.fill(2, 1, 36, bottom, Style::fg(color::FG));
        screen.fill(38, 1, screen.w - 42, bottom, Style::DEFAULT);
        screen.fill(screen.w - 40, 1, screen.w - 2, bottom, Style::DEFAULT);
    }
    let rows = state.fleet.iter().filter(|row| {
        row.thesis
            .claim
            .to_lowercase()
            .contains(&state.filter.to_lowercase())
    });
    for (i, row) in rows.enumerate().take(screen.h.saturating_sub(7)) {
        let y = if narrow { i + 2 } else { i + 1 };
        let health = row.state().as_str();
        let glyph = if row.thesis.status == "concluded" {
            "○"
        } else {
            mark(health)
        };
        let selected = row.thesis.id == state.fleet[state.selected].thesis.id;
        let style = if selected {
            Style::fg(color::WHITE).bg("#494949").bold()
        } else {
            Style::fg(color::FG)
        };
        if selected {
            screen.fill(2, y, if narrow { screen.w - 2 } else { 36 }, y + 1, style);
        }
        let max = if narrow { 44 } else { 32 };
        let text = format!("{glyph} {}", row.thesis.claim);
        let text = if text.chars().count() > max {
            format!("{}…", text.chars().take(max - 1).collect::<String>())
        } else {
            text
        };
        if selected {
            screen.text(3, y, &text, style);
        } else {
            screen.text(3, y, glyph, Style::fg(color::MUTED));
            screen.text(
                5,
                y,
                text.strip_prefix(&format!("{glyph} ")).unwrap_or(&text),
                style,
            );
        }
        if narrow {
            screen.text(
                49,
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
                    screen.text(62, y, &format!("{:+.2}", result.tilt), style);
                }
            }
            if let Some(pending) = state
                .pending
                .get(&row.thesis.id)
                .filter(|count| **count > 0)
            {
                screen.text(68, y, &format!("{pending} pending"), Style::fg("#a6a6a6"));
            }
        }
    }
    let Some(row) = state.fleet.get(state.selected) else {
        return;
    };
    screen.fill(
        2,
        bottom - 1,
        if narrow { screen.w - 2 } else { 36 },
        bottom,
        Style::DEFAULT,
    );
    screen.text(
        3,
        bottom - 1,
        &format!("{} theses", state.fleet.len()),
        Style::fg(color::MUTED),
    );
    let result = row.result.as_ref();
    let health = row.state().as_str();
    if narrow {
        recolor_claims_focus(screen, screen.w - 2, bottom);
        return;
    }
    let detail_x = 39;
    let detail_width = screen.w.saturating_sub(43 + detail_x);
    let mut y = paint_wrapped(
        screen,
        detail_x,
        1,
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
    screen.text(detail_x, y, "summary", Style::fg(color::MUTED).bold());
    let summary = state
        .summary
        .as_ref()
        .map_or("no summary yet — press s to summarise", |s| {
            s.summary.as_str()
        });
    paint_wrapped(
        screen,
        detail_x,
        y + 1,
        detail_width,
        summary,
        Style::fg(color::MUTED),
    );
    let evidence_x = screen.w.saturating_sub(40);
    let counts = result.map_or((0, 0, 0), |r| (r.support, r.against, r.neutral));
    let separator = bottom - 8;
    screen.fill(evidence_x, 3, screen.w - 2, separator, Style::fg(color::FG));
    let x = screen.text(
        evidence_x + 1,
        1,
        &format!("+{}", counts.0),
        Style::fg("#9b9b9b"),
    );
    let x = screen.text(x, 1, " support", Style::fg(color::MUTED));
    let x = screen.text(x, 1, "  ", Style::fg(color::FG));
    let x = screen.text(x, 1, &format!("−{}", counts.1), Style::fg("#8e8e8e"));
    let x = screen.text(x, 1, " against", Style::fg(color::MUTED));
    let x = screen.text(x, 1, "  ", Style::fg(color::FG));
    let x = screen.text(x, 1, &format!("?{}", counts.2), Style::fg("#a6a6a6"));
    screen.text(x, 1, " neutral", Style::fg(color::MUTED));
    screen.fill(
        evidence_x,
        2,
        screen.w - 2,
        3,
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    screen.text(
        evidence_x + 1,
        2,
        "   ±  age   note",
        Style::fg(color::FG).bold().bg(color::PANEL),
    );
    for (i, link) in state
        .links
        .iter()
        .enumerate()
        .take(screen.h.saturating_sub(14))
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
            Style::fg(color::FG).bg("#242424")
        } else {
            Style::fg(color::FG)
        };
        if i == state.evidence_selected {
            screen.fill(evidence_x, i + 3, screen.w - 2, i + 4, style);
        }
        let note = if link.note.is_empty() {
            &link.evidence_id
        } else {
            &link.note
        };
        screen.text(
            evidence_x + 1,
            i + 3,
            &format!("{glyph}  {side}  {age:<4}  {}", truncate(note, 24)),
            style,
        );
    }
    for x in evidence_x..screen.w - 2 {
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
            Style::fg("#9b9b9b").bold(),
        );
        let x = screen.text(x, separator + 1, " · ", Style::fg(color::MUTED));
        screen.text(x, separator + 1, side, Style::fg("#9b9b9b"));
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
                36,
                &delta_services::cite(item),
                Style::fg(color::MUTED),
            );
        }
    }
    recolor_claims_focus(screen, 36, bottom);
    recolor_live_pane_chrome(screen, bottom);
}

fn recolor_live_pane_chrome(screen: &mut Screen, bottom: usize) {
    for y in [0, bottom] {
        for x in 37..screen.w - 1 {
            let cell = &mut screen.cells[y * screen.w + x];
            if cell.fg == Some(color::BLUE) {
                cell.fg = Some("#898989");
            }
        }
    }
}

fn draw_theses_narrow_detail(screen: &mut Screen, state: &ThesesState) {
    let bottom = screen.h - 3;
    let Some(row) = state.fleet.get(state.selected) else {
        return;
    };
    let (title, hints) = if state.view == ThesesPane::Evidence {
        (
            "e evidence",
            pane_hints(&[
                ("a", "accept"),
                ("x", "reject"),
                ("u", "un-accept"),
                ("esc", "back"),
            ]),
        )
    } else {
        (
            "t thesis",
            pane_hints(&[
                ("s", "summarise"),
                ("d", "edit"),
                ("e", "evidence"),
                ("esc", "back"),
            ]),
        )
    };
    screen.pane(
        1,
        0,
        screen.w - 2,
        bottom,
        true,
        &[(title, Style::fg(color::MUTED).bold())],
        &hints,
    );
    screen.fill(2, 1, screen.w - 2, bottom, Style::fg(color::FG));
    if state.view == ThesesPane::Detail {
        screen.text(3, 1, &row.thesis.claim, Style::fg(color::FG).bold());
        screen.text(
            3,
            3,
            &format!("{} {}", mark(row.state().as_str()), row.state().as_str()),
            Style::fg(color::MUTED),
        );
        screen.text(
            3,
            5,
            &format!(
                "{} · horizon {}",
                row.thesis.status, row.thesis.time_horizon
            ),
            Style::fg(color::MUTED),
        );
        screen.text(
            3,
            7,
            &format!("targets   {}", row.thesis.targets.join(", ")),
            Style::fg(color::FG),
        );
        screen.text(3, 9, "summary", Style::fg(color::MUTED).bold());
        screen.text(
            3,
            10,
            state
                .summary
                .as_ref()
                .map_or("no summary yet — press s to summarise", |s| {
                    s.summary.as_str()
                }),
            Style::fg(color::FG),
        );
    } else {
        for (i, link) in state
            .links
            .iter()
            .enumerate()
            .take(bottom.saturating_sub(7))
        {
            let style = if i == state.evidence_selected {
                Style::fg(color::WHITE).bg("#494949").bold()
            } else {
                Style::fg(color::FG)
            };
            let glyph = if link.accepted { "✓" } else { "•" };
            screen.text(
                3,
                i + 1,
                &format!("{glyph} {}  {}", link.side, link.note),
                style,
            );
        }
        if let Some(link) = state.selected_link() {
            screen.text(3, bottom - 3, &link.note, Style::fg(color::FG));
        }
    }
    recolor_claims_focus(screen, screen.w - 2, bottom);
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

fn recolor_claims_focus(screen: &mut Screen, right: usize, bottom: usize) {
    for y in 0..=bottom {
        for x in 1..=right {
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
