//! Target list selection, grouping and filtering (the Python Targets view).
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use delta_services::{ServiceError, WatchTarget};

#[derive(Clone)]
pub enum WatchRow {
    Group { asset_class: String, count: usize },
    Target(WatchTarget),
}

#[derive(Default)]
pub struct WatchlistBrowser {
    pub search: String,
    pub editing: bool,
    pub detail_open: bool,
    pub rows: Vec<WatchRow>,
    pub selected: usize,
    pub member: usize,
    targets: BTreeMap<String, WatchTarget>,
    collapsed: BTreeSet<String>,
}

impl WatchlistBrowser {
    pub fn reload(&mut self, config_path: &Path) -> Result<(), ServiceError> {
        self.targets = delta_services::target_specs(config_path)?;
        self.rebuild();
        Ok(())
    }

    pub fn rebuild(&mut self) {
        let selected = self.target().map(|target| target.id.clone());
        let query = self.search.trim().to_lowercase();
        let mut groups: BTreeMap<String, Vec<WatchTarget>> = BTreeMap::new();
        for target in self.targets.values() {
            let haystack = format!(
                "{} {} {} {} {} {}",
                target.id,
                target.name,
                target.kind,
                target.markets.join(" "),
                target.tickers.join(" "),
                target.tags.iter().cloned().collect::<Vec<_>>().join(" ")
            )
            .to_lowercase();
            if !haystack.contains(&query) {
                continue;
            }
            groups
                .entry(target.asset_class.as_str().to_string())
                .or_default()
                .push(target.clone());
        }
        self.rows.clear();
        for class in [
            "equity",
            "etf",
            "index",
            "crypto",
            "fx",
            "commodity",
            "bond",
            "other",
        ] {
            if let Some(entries) = groups.remove(class) {
                self.rows.push(WatchRow::Group {
                    asset_class: class.into(),
                    count: entries.len(),
                });
                if !self.collapsed.contains(class) {
                    self.rows.extend(entries.into_iter().map(WatchRow::Target));
                }
            }
        }
        for (class, entries) in groups {
            self.rows.push(WatchRow::Group {
                asset_class: class.clone(),
                count: entries.len(),
            });
            if !self.collapsed.contains(&class) {
                self.rows.extend(entries.into_iter().map(WatchRow::Target));
            }
        }
        self.selected = selected
            .and_then(|id| {
                self.rows
                    .iter()
                    .position(|row| matches!(row, WatchRow::Target(target) if target.id == id))
            })
            .unwrap_or_else(|| self.selected.min(self.rows.len().saturating_sub(1)));
    }

    pub fn target(&self) -> Option<&WatchTarget> {
        match self.rows.get(self.selected) {
            Some(WatchRow::Target(target)) => Some(target),
            _ => None,
        }
    }

    pub fn instrument_id(&self) -> Option<String> {
        let target = self.target()?;
        let symbol = target
            .tickers
            .get(self.member % target.tickers.len().max(1))?;
        Some(format!(
            "{}:{symbol}",
            target.markets.first()?.to_uppercase()
        ))
    }

    pub fn move_selection(&mut self, step: i32) {
        self.selected = self
            .selected
            .saturating_add_signed(step as isize)
            .min(self.rows.len().saturating_sub(1));
        self.member = 0;
    }

    pub fn cycle_member(&mut self, step: i32) {
        let count = self.target().map_or(0, |target| target.tickers.len());
        if count > 0 {
            self.member = (self.member as i32 + step).rem_euclid(count as i32) as usize;
        }
    }

    pub fn toggle_group(&mut self) {
        let class = match self.rows.get(self.selected) {
            Some(WatchRow::Group { asset_class, .. }) => asset_class.clone(),
            Some(WatchRow::Target(target)) => target.asset_class.as_str().to_string(),
            None => return,
        };
        if !self.collapsed.remove(&class) {
            self.collapsed.insert(class);
        }
        self.rebuild();
    }

    pub fn is_collapsed(&self, class: &str) -> bool {
        self.collapsed.contains(class)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn target_selection_filters_groups_and_cycles_multiple_members() {
        let mut browser = WatchlistBrowser::default();
        browser.targets.insert("tech".into(), delta_services::target_from_spec("tech", &json!({"kind":"theme", "market":"us", "tickers":["AAPL","MSFT"], "tags":["growth"]}), false).unwrap());
        browser.rebuild();
        assert_eq!(browser.rows.len(), 2);
        browser.move_selection(1);
        assert_eq!(browser.instrument_id().as_deref(), Some("US:AAPL"));
        browser.cycle_member(-1);
        assert_eq!(browser.instrument_id().as_deref(), Some("US:MSFT"));
        browser.toggle_group();
        assert_eq!(browser.rows.len(), 1);
        browser.toggle_group();
        assert_eq!(browser.rows.len(), 2);
        browser.search = "GROWTH".into();
        browser.rebuild();
        assert_eq!(browser.rows.len(), 2);
        browser.search = "missing".into();
        browser.rebuild();
        assert!(browser.rows.is_empty());
    }
}
