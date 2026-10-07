//! Desk-default table: zebra stripes and a row cursor
//! (port of `delta/tui/widgets.py::DeltaTable`, a `DataTable` with
//! `zebra_stripes = True` and `cursor_type = "row"`).
//!
//! ratatui's `Table` has `row_highlight_style`, so the port draws the
//! zebra/cursor pair by hand with the theme tokens.

use ratatui::style::{Modifier, Style};
use ratatui::widgets::{Row, Table as RatatuiTable, TableState};

use crate::theme::Theme;

/// Wrap plain rows into the desk table: zebra stripes on the even rows and
/// the `$primary` cursor row, selection via `TableState`.
pub fn desk_table<'a>(
    rows: Vec<Row<'a>>,
    widths: &[ratatui::layout::Constraint],
    state: &TableState,
) -> RatatuiTable<'a> {
    let zebra = Style::default().bg(Theme::SURFACE.color());
    let selected = Style::default()
        .bg(Theme::PRIMARY.color())
        .fg(Theme::FOREGROUND.color())
        .add_modifier(Modifier::BOLD);
    let rows: Vec<Row> = rows
        .into_iter()
        .enumerate()
        .map(|(i, row)| {
            if Some(i) == state.selected() {
                row.style(selected)
            } else if i % 2 == 1 {
                row.style(zebra)
            } else {
                row
            }
        })
        .collect();
    RatatuiTable::new(rows, widths.to_vec())
        .row_highlight_style(selected)
        .header(
            Row::new(Vec::<String>::new()).style(Style::default().fg(Theme::TEXT_MUTED.color())),
        )
        .column_spacing(2)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::layout::Constraint;
    use ratatui::text::Text;
    use ratatui::Terminal;

    fn render_rows(count: usize, selected: Option<usize>) -> Vec<Vec<ratatui::style::Color>> {
        let rows: Vec<Row> = (0..count)
            .map(|i| Row::new(Text::from(format!("row {i}"))))
            .collect();
        let table = desk_table(
            rows,
            &[Constraint::Length(8)],
            &TableState::default().with_selected(selected),
        );
        let mut terminal = Terminal::new(TestBackend::new(20, count as u16 + 1)).unwrap();
        terminal
            .draw(|f| {
                f.render_stateful_widget(
                    table,
                    f.area(),
                    &mut TableState::default().with_selected(selected),
                );
            })
            .unwrap();
        (0..count + 1)
            .map(|y| {
                (0..20u16)
                    .map(|x| terminal.backend().buffer()[(x, y as u16)].bg)
                    .collect()
            })
            .collect()
    }

    #[test]
    fn zebra_stripes_alternate() {
        // The header takes render row 0; data rows follow. Even-index data
        // rows stay plain, odd ones get the surface tint.
        let grid = render_rows(4, None);
        let bg = |y: usize| grid[y][0];
        assert_eq!(bg(1), bg(3), "even data rows stay plain");
        assert_eq!(bg(2), bg(4), "odd data rows share the zebra tint");
        assert_ne!(bg(1), bg(2), "zebra alternates");
    }

    #[test]
    fn cursor_row_uses_primary() {
        let grid = render_rows(3, Some(1));
        // Selected row background is the brand blue.
        assert_eq!(grid[2][0], ratatui::style::Color::Rgb(0x26, 0x4b, 0x96));
    }
}

/// The DataTable scroll state the screens own (`widgets.py::DeltaTable`
/// inherits Textual's scrolling; the Rust port keeps it in a `TableState`
/// with these desk-default moves: clamped cursor, offset following the
/// cursor, header row preserved).
#[derive(Debug, Clone, Default)]
pub struct TableScroll {
    pub state: TableState,
    pub len: usize,
}

impl TableScroll {
    pub fn new(len: usize) -> Self {
        Self {
            state: TableState::default(),
            len,
        }
    }

    pub fn selected(&self) -> Option<usize> {
        self.state.selected()
    }

    pub fn select(&mut self, index: usize) {
        self.state
            .select(Some(index.min(self.len.saturating_sub(1))));
    }

    /// Cursor down, clamped to the rows (a table with no rows stays
    /// unselected).
    pub fn next(&mut self) {
        if self.len == 0 {
            return;
        }
        let current = self.state.selected().map_or(0, |i| i + 1);
        self.state.select(Some(current.min(self.len - 1)));
    }

    pub fn previous(&mut self) {
        let current = self.state.selected().unwrap_or(0);
        self.state.select(Some(current.saturating_sub(1)));
    }

    /// Keep the viewport showing the cursor: ratatui scrolls the offset
    /// itself when rendering with the same `TableState`, so this is only
    /// needed for offset checks in tests.
    pub fn offset(&self) -> usize {
        self.state.offset()
    }
}

#[cfg(test)]
mod scroll_tests {
    use super::*;

    #[test]
    fn cursor_clamps_at_both_ends() {
        let mut table = TableScroll::new(3);
        table.next();
        table.next();
        table.next();
        table.next();
        assert_eq!(table.selected(), Some(2), "down clamps at the last row");
        table.previous();
        table.previous();
        table.previous();
        table.previous();
        assert_eq!(table.selected(), Some(0), "up clamps at the first row");
        assert_eq!(table.offset(), 0);
    }

    #[test]
    fn empty_table_stays_none() {
        let mut table = TableScroll::new(0);
        table.next();
        assert_eq!(table.selected(), None);
    }
}
