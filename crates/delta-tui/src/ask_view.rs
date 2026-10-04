//! Ask scope selection and citation preview state.
use crate::research::{truncate, wrap};
use crate::screen::{color, Screen, Style};
use delta_services::{EvidenceItem, ServiceError, WatchTarget};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Default)]
pub struct AskState {
    pub targets: Vec<WatchTarget>,
    pub scope: BTreeSet<String>,
    pub target_selected: usize,
    pub targets_focus: bool,
    pub zoomed: bool,
    pub clear_pending: bool,
    pub citations: Vec<String>,
    pub citation_selected: usize,
    pub items: BTreeMap<String, EvidenceItem>,
    initialized: bool,
}

impl AskState {
    pub fn reload(&mut self, path: &Path) -> Result<(), ServiceError> {
        self.targets = delta_services::target_specs(path)?.into_values().collect();
        if !self.initialized {
            self.scope = self
                .targets
                .iter()
                .map(|target| target.id.clone())
                .collect();
            self.initialized = true;
        } else {
            self.scope
                .retain(|id| self.targets.iter().any(|target| &target.id == id));
        }
        self.target_selected = self
            .target_selected
            .min(self.targets.len().saturating_sub(1));
        Ok(())
    }

    pub fn instrument_ids(&self) -> Vec<String> {
        self.targets
            .iter()
            .filter(|target| self.scope.contains(&target.id))
            .flat_map(|target| target.instruments())
            .map(|instrument| instrument.id)
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub fn toggle_target(&mut self) {
        if let Some(target) = self.targets.get(self.target_selected) {
            if !self.scope.remove(&target.id) {
                self.scope.insert(target.id.clone());
            }
        }
    }

    pub fn toggle_all(&mut self) {
        if self.scope.len() == self.targets.len() {
            self.scope.clear();
        } else {
            self.scope = self
                .targets
                .iter()
                .map(|target| target.id.clone())
                .collect();
        }
    }

    pub fn paint_sidebar(&self, screen: &mut Screen) {
        let width = screen.w.saturating_sub(2);
        let mid = screen.h / 2;
        screen.text(
            1,
            0,
            "Targets · space toggle · a all",
            Style::fg(color::BLUE).bold(),
        );
        let visible = mid.saturating_sub(3).max(1);
        let start = self.target_selected.saturating_sub(visible - 1);
        for (row, (index, target)) in self
            .targets
            .iter()
            .enumerate()
            .skip(start)
            .take(visible)
            .enumerate()
        {
            let text = format!(
                "{} {} {}",
                if index == self.target_selected {
                    "›"
                } else {
                    " "
                },
                if self.scope.contains(&target.id) {
                    "●"
                } else {
                    "○"
                },
                target.name
            );
            screen.text(1, 2 + row, &truncate(&text, width), Style::fg(color::FG));
        }
        screen.text(
            1,
            mid,
            "Citations · ←→ select · o open",
            Style::fg(color::BLUE).bold(),
        );
        if let Some(id) = self.citations.get(self.citation_selected) {
            let text = self
                .items
                .get(id)
                .map(|item| {
                    format!(
                        "{}\n{}\n{}",
                        delta_services::cite(item),
                        item.body.as_deref().unwrap_or(""),
                        item.id
                    )
                })
                .unwrap_or_else(|| id.clone());
            for (row, line) in wrap(&text, width)
                .iter()
                .take(screen.h.saturating_sub(mid + 2))
                .enumerate()
            {
                screen.text(1, mid + 2 + row, line, Style::fg(color::FG));
            }
        } else {
            screen.text(1, mid + 2, "No citations yet", Style::fg(color::MUTED));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn selected_targets_expand_without_duplicate_instruments() {
        let mut state = AskState {
            targets: ["first", "second"]
                .iter()
                .map(|name| {
                    delta_services::target_from_spec(
                        name,
                        &json!({"market":"us", "tickers":["AAPL"]}),
                        false,
                    )
                    .unwrap()
                })
                .collect(),
            ..Default::default()
        };
        state.toggle_all();
        assert_eq!(state.instrument_ids(), ["US:AAPL"]);
        state.toggle_target();
        assert_eq!(state.instrument_ids(), ["US:AAPL"]);
        state.target_selected = 1;
        state.toggle_target();
        assert!(state.instrument_ids().is_empty());
        state.toggle_all();
        assert_eq!(state.scope.len(), 2);
    }
}
