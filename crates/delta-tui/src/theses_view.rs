//! Live thesis claims, framing and evidence panes.
use crate::research::{truncate, wrap};
use crate::screen::{color, Screen, Style};
use delta_services::{EvidenceItem, HealthResult, HealthState, Thesis, ThesisEvidence};
use std::collections::BTreeMap;

#[derive(Default, Clone, Copy, PartialEq, Eq)]
pub enum ThesisFocus {
    #[default]
    Claims,
    Framing,
    Evidence,
}

pub struct ThesisView<'a> {
    pub theses: &'a [Thesis],
    pub indices: &'a [usize],
    pub selected: usize,
    pub links: &'a [ThesisEvidence],
    pub evidence_selected: usize,
    pub framing_scroll: usize,
    pub note_scroll: usize,
    pub items: &'a BTreeMap<String, EvidenceItem>,
    pub health: &'a BTreeMap<String, HealthResult>,
    pub filter: &'a str,
    pub focus: ThesisFocus,
}

impl ThesisView<'_> {
    pub fn paint(&self, screen: &mut Screen) {
        let bottom = screen.h.saturating_sub(3);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        if screen.w < crate::NARROW_WIDTH as usize {
            match self.focus {
                ThesisFocus::Claims => self.claims(screen),
                ThesisFocus::Framing => self.framing(screen),
                ThesisFocus::Evidence => self.evidence(screen),
            }
            return;
        }
        let right = screen.w - 42;
        let evidence_left = screen.w - 41;
        let title = Style::fg(color::BLUE).bold();
        screen.pane(
            1,
            0,
            36,
            bottom,
            self.focus == ThesisFocus::Claims,
            &[("theses", title)],
            &[],
        );
        screen.pane(
            37,
            0,
            right,
            bottom,
            self.focus == ThesisFocus::Framing,
            &[("t thesis", title)],
            &[],
        );
        screen.pane(
            evidence_left,
            0,
            screen.w - 2,
            bottom,
            self.focus == ThesisFocus::Evidence,
            &[("e evidence", title)],
            &[],
        );
        let mut claims = Screen::new(34, bottom.saturating_sub(1));
        self.claims(&mut claims);
        screen.blit_at(&claims, 2, 1);
        let mut framing = Screen::new(right - 38, bottom.saturating_sub(1));
        self.framing(&mut framing);
        screen.blit_at(&framing, 38, 1);
        let mut evidence = Screen::new(38, bottom.saturating_sub(1));
        self.evidence(&mut evidence);
        screen.blit_at(&evidence, evidence_left + 1, 1);
    }

    fn claims(&self, screen: &mut Screen) {
        let width = screen.w.saturating_sub(2);
        screen.text(1, 0, "n new · d edit · / filter", Style::fg(color::MUTED));
        screen.text(
            1,
            1,
            &truncate(
                &format!("{} claims · {}", self.indices.len(), self.filter),
                width,
            ),
            Style::fg(color::MUTED),
        );
        let visible = screen.h.saturating_sub(4).max(1);
        let start = self
            .indices
            .iter()
            .position(|index| *index == self.selected)
            .unwrap_or(0)
            .saturating_sub(visible - 1);
        for (row, index) in self.indices.iter().skip(start).take(visible).enumerate() {
            let thesis = &self.theses[*index];
            let state = self
                .health
                .get(&thesis.id)
                .map_or(HealthState::Emerging, |health| health.state);
            let glyph = if thesis.status == "concluded" {
                "○"
            } else {
                match state {
                    HealthState::Building => "▲",
                    HealthState::Weakening => "▼",
                    HealthState::Mixed => "◆",
                    HealthState::Challenged => "✕",
                    HealthState::Emerging => "○",
                    HealthState::Idle => "◌",
                }
            };
            let style = if *index == self.selected {
                Style::fg(color::WHITE).bg(color::BLUE_BG)
            } else {
                Style::fg(color::FG)
            };
            screen.fill(0, row + 3, screen.w, row + 4, style);
            screen.text(
                1,
                row + 3,
                &truncate(&format!("{glyph} {}", thesis.claim), width),
                style,
            );
        }
        if self.indices.is_empty() {
            screen.text(1, 3, "No matching claims", Style::fg(color::MUTED));
        }
    }

    fn framing(&self, screen: &mut Screen) {
        let Some(thesis) = self.theses.get(self.selected) else {
            return;
        };
        let health = self
            .health
            .get(&thesis.id)
            .map(delta_services::badge_text)
            .unwrap_or_else(|| "emerging · no accepted evidence".into());
        let text = format!("{}\n\n{}\n{}\nScope: {}\nHorizon: {}\nTargets: {}\n\nAssumptions\n{}\n\nFalsifiers\n{}", thesis.claim, thesis.status, health, thesis.scope, thesis.time_horizon, thesis.targets.join(", "), thesis.assumptions.join("\n"), thesis.falsifiers.join("\n"));
        for (row, line) in wrap(&text, screen.w.saturating_sub(2))
            .iter()
            .skip(self.framing_scroll)
            .take(screen.h)
            .enumerate()
        {
            screen.text(
                1,
                row,
                line,
                if row == 0 {
                    Style::fg(color::FG).bold()
                } else {
                    Style::fg(color::FG)
                },
            );
        }
    }

    fn evidence(&self, screen: &mut Screen) {
        let width = screen.w.saturating_sub(2);
        screen.text(
            1,
            0,
            "f find · a accept · x reject · u undo",
            Style::fg(color::MUTED),
        );
        let visible = screen.h.saturating_sub(10).max(1);
        let start = self.evidence_selected.saturating_sub(visible - 1);
        for (row, (index, link)) in self
            .links
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let side = match link.side {
                delta_services::EvidenceSide::Support => "+",
                delta_services::EvidenceSide::Against => "−",
                delta_services::EvidenceSide::Neutral => "?",
            };
            let item = self.items.get(&link.evidence_id);
            let label = item.map_or(link.evidence_id.as_str(), |item| item.title.as_str());
            let text = format!(
                "{} {side} {} {label}",
                if link.accepted { "✓" } else { "○" },
                if index == self.evidence_selected {
                    "›"
                } else {
                    " "
                }
            );
            screen.text(
                1,
                row + 2,
                &truncate(&text, width),
                if index == self.evidence_selected {
                    Style::fg(color::BLUE).bold()
                } else {
                    Style::fg(color::FG)
                },
            );
        }
        if let Some(link) = self.links.get(self.evidence_selected) {
            let top = visible + 3;
            let citation = self
                .items
                .get(&link.evidence_id)
                .map(delta_services::cite)
                .unwrap_or_else(|| link.evidence_id.clone());
            let text = format!("{}\n{}\n{}", link.note, citation, link.evidence_id);
            for (row, line) in wrap(&text, width)
                .iter()
                .skip(self.note_scroll)
                .take(screen.h.saturating_sub(top))
                .enumerate()
            {
                screen.text(1, top + row, line, Style::fg(color::FG));
            }
        } else {
            screen.text(1, 2, "No evidence yet · f find", Style::fg(color::MUTED));
        }
    }
}
