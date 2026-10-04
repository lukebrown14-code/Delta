//! Decision list and timeline, with a narrow drill-in view.
use crate::research::{truncate, wrap};
use crate::screen::{color, Screen, Style};
use delta_services::{Decision, DecisionReview};

pub struct DecisionView<'a> {
    pub decisions: &'a [Decision],
    pub indices: &'a [usize],
    pub selected: usize,
    pub reviews: &'a [DecisionReview],
    pub filter: &'a str,
    pub detail_open: bool,
    pub scroll: usize,
}

impl DecisionView<'_> {
    pub fn paint(&self, screen: &mut Screen) {
        let bottom = screen.h.saturating_sub(3);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        if screen.w < crate::NARROW_WIDTH as usize {
            if self.detail_open {
                self.timeline(screen);
            } else {
                self.list(screen);
            }
            return;
        }
        let split = screen.w * 2 / 5;
        let title = Style::fg(color::BLUE).bold();
        screen.pane(
            1,
            0,
            split - 1,
            bottom,
            !self.detail_open,
            &[("decisions", title)],
            &[],
        );
        screen.pane(
            split,
            0,
            screen.w - 2,
            bottom,
            self.detail_open,
            &[("timeline", title)],
            &[],
        );
        let mut list = Screen::new(split - 3, bottom.saturating_sub(1));
        self.list(&mut list);
        screen.blit_at(&list, 2, 1);
        let mut timeline = Screen::new(screen.w - split - 3, bottom.saturating_sub(1));
        self.timeline(&mut timeline);
        screen.blit_at(&timeline, split + 1, 1);
    }

    fn list(&self, screen: &mut Screen) {
        let width = screen.w.saturating_sub(2);
        screen.text(
            1,
            0,
            "n new · e edit · d delete · / filter",
            Style::fg(color::MUTED),
        );
        screen.text(
            1,
            1,
            &truncate(
                &format!("{} decisions · {}", self.indices.len(), self.filter),
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
            let decision = &self.decisions[*index];
            let style = if *index == self.selected {
                Style::fg(color::WHITE).bg(color::BLUE_BG)
            } else {
                Style::fg(color::FG)
            };
            screen.fill(0, row + 3, screen.w, row + 4, style);
            screen.text(
                1,
                row + 3,
                &truncate(
                    &format!(
                        "{} {} {}",
                        decision.status, decision.review_date, decision.instrument_id
                    ),
                    width,
                ),
                style,
            );
        }
        if self.indices.is_empty() {
            screen.text(1, 3, "No matching decisions", Style::fg(color::MUTED));
        }
    }

    fn timeline(&self, screen: &mut Screen) {
        let Some(decision) = self.decisions.get(self.selected) else {
            return;
        };
        screen.text(
            1,
            0,
            "r review · o research · Esc back",
            Style::fg(color::MUTED),
        );
        let mut text = format!("{} · {}\nCreated: {}\nReview: {}\n\nRationale\n{}\n\nValuation context\n{}\n\nHorizon\n{}\n\nInvalidation criteria\n{}", decision.instrument_id, decision.status, decision.created_at.date(), decision.review_date, decision.rationale, decision.valuation_context, decision.time_horizon, decision.invalidation_criteria);
        if let Some(claim) = &decision.thesis_claim_snapshot {
            text.push_str(&format!("\n\nOriginal thesis\n{claim}"));
        }
        if let Some(id) = &decision.thesis_id {
            text.push_str(&format!("\nCurrent thesis: {id}"));
        }
        for review in self.reviews {
            text.push_str(&format!(
                "\n\nReview {} · {}\n{}",
                review.created_at.date(),
                review.status.as_deref().unwrap_or("unchanged"),
                review.note
            ));
        }
        for (row, line) in wrap(&text, screen.w.saturating_sub(2))
            .iter()
            .skip(self.scroll)
            .take(screen.h.saturating_sub(2))
            .enumerate()
        {
            screen.text(1, row + 2, line, Style::fg(color::FG));
        }
    }
}
