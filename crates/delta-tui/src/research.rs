//! Research evidence browser state; storage reads go through services.

use std::path::Path;

use delta_core::db::Db;
use delta_services::{EvidenceItem, ServiceError};

use crate::screen::{color, Screen, Style};
use unicode_width::UnicodeWidthChar;

const KINDS: &[Option<&str>] = &[
    None,
    Some("news"),
    Some("filing"),
    Some("event"),
    Some("fundamental"),
    Some("bar"),
];

pub struct ResearchBrowser {
    pub evidence_open: bool,
    pub detail_open: bool,
    pub preview_scroll: usize,
    pub company_open: bool,
    pub zoomed: bool,
    pub search_editing: bool,
    pub search: String,
    pub selected: usize,
    pub items: Vec<EvidenceItem>,
    pub limit: usize,
    pub history_index: usize,
    kind_index: usize,
}

impl Default for ResearchBrowser {
    fn default() -> Self {
        Self {
            evidence_open: false,
            detail_open: false,
            preview_scroll: 0,
            company_open: true,
            zoomed: false,
            search_editing: false,
            search: String::new(),
            selected: 0,
            items: Vec::new(),
            limit: 80,
            history_index: 0,
            kind_index: 0,
        }
    }
}

impl ResearchBrowser {
    pub fn reload(&mut self, db_path: &Path, target: &str) -> Result<(), ServiceError> {
        let db = Db::open(db_path)?;
        self.items = delta_services::evidence_filtered(
            &db,
            Some(target),
            None,
            KINDS[self.kind_index],
            self.limit.max(80),
            Some(&self.search),
        )?;
        self.selected = self.selected.min(self.items.len().saturating_sub(1));
        Ok(())
    }

    pub fn cycle_kind(&mut self) {
        self.kind_index = (self.kind_index + 1) % KINDS.len();
        self.selected = 0;
    }

    pub fn paint(&self, screen: &mut Screen, target: &str) {
        let bottom = screen.h.saturating_sub(2);
        screen.fill(0, 0, screen.w, bottom, Style::DEFAULT);
        screen.text(
            2,
            0,
            &format!("Evidence · {target}"),
            Style::fg(color::BLUE).bold(),
        );
        screen.text(
            2,
            1,
            "r report · / search · k kind · l more · ↑↓ select · Esc back",
            Style::fg(color::MUTED),
        );
        let search = if self.search_editing { "›" } else { " " };
        screen.text(
            2,
            3,
            &format!(
                "{search} Search: {}  Kind: {}  {} items",
                self.search,
                KINDS[self.kind_index].unwrap_or("all"),
                self.items.len()
            ),
            Style::fg(color::MUTED),
        );
        if self.detail_open {
            if let Some(item) = self.items.get(self.selected) {
                let text = format!(
                    "{}\n{}\n\n{}",
                    delta_services::cite(item),
                    item.id,
                    item.body.as_deref().unwrap_or(&item.title)
                );
                for (row, line) in wrap(&text, screen.w.saturating_sub(4))
                    .iter()
                    .skip(self.preview_scroll)
                    .take(bottom.saturating_sub(5))
                    .enumerate()
                {
                    screen.text(2, row + 5, line, Style::fg(color::FG));
                }
            }
            return;
        }
        let list_height = bottom.saturating_sub(13).max(1);
        let start = self.selected.saturating_sub(list_height.saturating_sub(1));
        let width = screen.w.saturating_sub(4);
        for (row, (index, item)) in self
            .items
            .iter()
            .enumerate()
            .skip(start)
            .take(list_height)
            .enumerate()
        {
            let marker = if index == self.selected { "›" } else { " " };
            let label = format!(
                "{marker} {} {:<12} {}",
                item.ts.date(),
                item.kind,
                item.title
            );
            screen.text(
                2,
                5 + row,
                &truncate(&label, width),
                if index == self.selected {
                    Style::fg(color::BLUE).bold()
                } else {
                    Style::fg(color::FG)
                },
            );
        }
        if let Some(item) = self.items.get(self.selected) {
            let top = 6 + list_height;
            screen.text(
                2,
                top,
                &truncate(&delta_services::cite(item), width),
                Style::fg(color::BLUE),
            );
            screen.text(
                2,
                top + 1,
                &truncate(&format!("Evidence id: {}", item.id), width),
                Style::fg(color::MUTED),
            );
            let body = item.body.as_deref().unwrap_or(&item.title);
            for (row, line) in wrap(body, width)
                .iter()
                .take(bottom.saturating_sub(top + 2))
                .enumerate()
            {
                screen.text(2, top + 2 + row, line, Style::fg(color::FG));
            }
        } else {
            screen.text(
                2,
                5,
                "No matching evidence. Press U to gather.",
                Style::fg(color::MUTED),
            );
        }
    }
}

pub fn truncate(text: &str, width: usize) -> String {
    let mut used = 0;
    text.chars()
        .take_while(|ch| {
            used += ch.width().unwrap_or(0);
            used <= width
        })
        .collect()
}

pub fn wrap(text: &str, width: usize) -> Vec<String> {
    let width = width.max(1);
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        let mut used = 0;
        for word in paragraph.split_whitespace() {
            let word_width: usize = word.chars().map(|ch| ch.width().unwrap_or(0)).sum();
            if used > 0 && used + 1 + word_width > width {
                lines.push(std::mem::take(&mut line));
                used = 0;
            }
            if used > 0 {
                line.push(' ');
                used += 1;
            }
            for ch in word.chars() {
                let size = ch.width().unwrap_or(0);
                if used + size > width && !line.is_empty() {
                    lines.push(std::mem::take(&mut line));
                    used = 0;
                }
                line.push(ch);
                used += size;
            }
        }
        lines.push(line);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wrapping_keeps_words_and_accounts_for_wide_characters() {
        assert_eq!(wrap("one two three", 7), ["one two", "three"]);
        assert_eq!(wrap("東京大阪", 4), ["東京", "大阪"]);
        assert_eq!(truncate("東京大阪", 5), "東京");
        assert_eq!(wrap("a\n\nb", 5), ["a", "", "b"]);
    }
}
