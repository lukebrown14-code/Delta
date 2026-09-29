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
