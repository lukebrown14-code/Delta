//! The seven panes (1-6 plus `c`): deterministic painters over the golden
//! [`crate::screen::Screen`] model, one module per screen plus the shared
//! status-bar painters. Port of `delta/tui/screens/`; the golden exporter
//! (`tests/export_golden.py`) captures these layouts at 80x24, 120x40 and
//! 200x50.

mod ask;
pub mod decisions;
mod home;
pub mod live;
mod research;
mod settings;
mod status_bar;
mod theses;
mod watchlist;

pub use ask::{draw_ask, draw_ask_live, draw_ask_narrow, draw_ask_wide, paint_ask_state, AskState};
pub use decisions::{
    draw_decisions, draw_decisions_live, draw_decisions_narrow, draw_decisions_wide, DecisionsData,
};
pub use home::{
    draw_home, draw_home_live, draw_home_narrow, draw_home_setup, draw_home_wide, feed_seed,
    HomeCommand, HomeComponent, HomeFeed, HomeLiveData, HomeSetupData, HomeState,
};
pub use research::{
    draw_research, draw_research_live, draw_research_narrow, draw_research_wide, ResearchComponent,
    ResearchData, ResearchState, ResearchView,
};
pub use settings::{
    comma, draw_settings, draw_settings_narrow, draw_settings_wide, exporter_world, human_size,
    stamp, SettingsCommand, SettingsComponent, SettingsData, SettingsDiagnostics, SettingsFocus,
    SettingsMarket, SettingsSource, SettingsState, SettingsView,
};
pub use status_bar::{Footer, NarrowTab};
pub use theses::{
    draw_theses, draw_theses_live, draw_theses_narrow, draw_theses_wide, draw_thesis_form_runtime,
    EvidenceDecision, ThesesCommand, ThesesComponent, ThesesPane, ThesesState,
};
pub use watchlist::{
    chart_window, draw_glossary_overlay, draw_watchlist, draw_watchlist_add,
    draw_watchlist_add_results, draw_watchlist_filter, draw_watchlist_narrow,
    draw_watchlist_narrow_list, draw_watchlist_wide, friendly_date, friendly_date_range, grouped,
    paint_watchlist_add_overlay, range_label, range_window, AddSearchState, MetricsData,
    WatchEntry, WatchlistCommand, WatchlistComponent, WatchlistState, GLOSSARY_EQUITY, RANGES,
};

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// A pane rectangle in golden-Screen coordinates: `(x0, y0, x1, y1)` with
/// `x1`/`y1` exclusive.
pub type PaneRect = (usize, usize, usize, usize);

/// Split a content area into a grid of pane rects: `row_heights` and
/// `col_widths` in cells (sums less than the area leave the remainder on
/// the last row/column, matching Textual's `1fr` stacking). The zoom
/// helper picks one rect for a full-screen pane.
///
/// The screen painters use this adapter while moving to `Rect`. Ratatui
/// determines the pane geometry; the returned coordinates preserve the
/// existing exclusive-end convention used by the golden painters.
pub fn pane_layout(
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
    row_heights: &[usize],
    col_widths: &[usize],
) -> Vec<Vec<PaneRect>> {
    if row_heights.is_empty() || col_widths.is_empty() {
        return Vec::new();
    }
    let area = Rect::new(
        x0 as u16,
        y0 as u16,
        x1.saturating_sub(x0) as u16,
        y1.saturating_sub(y0) as u16,
    );
    let constraints = |sizes: &[usize]| {
        sizes
            .iter()
            .enumerate()
            .map(|(index, size)| {
                if index + 1 == sizes.len() {
                    Constraint::Min(0)
                } else {
                    Constraint::Length((*size).min(u16::MAX as usize) as u16)
                }
            })
            .collect::<Vec<_>>()
    };
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints(constraints(row_heights))
        .split(area);
    rows.iter()
        .map(|row| {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints(constraints(col_widths))
                .split(*row)
                .iter()
                .map(|rect| {
                    (
                        rect.x as usize,
                        rect.y as usize,
                        rect.right() as usize,
                        rect.bottom() as usize,
                    )
                })
                .collect()
        })
        .collect()
}

/// The zoom state: `None` shows the grid; `Some(index)` (row-major) blows
/// one pane up to the full content area (`z` in Python's research/chat).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Zoom {
    #[default]
    None,
    Pane(usize),
}

/// The rect a pane paints into, honouring the zoom: `Some(_)` collapses the
/// grid to one rect covering the whole area (the zoomed pane is maximized;
/// which pane is zoomed stays in the screen's own state).
pub fn zoom_rect(rects: &[Vec<PaneRect>], zoom: Zoom) -> Vec<Vec<PaneRect>> {
    match zoom {
        Zoom::None => rects.to_vec(),
        Zoom::Pane(_) => {
            let flat: Vec<PaneRect> = rects.iter().flatten().copied().collect();
            let area = match (flat.first(), flat.last()) {
                (Some(&(x0, y0, _, _)), Some(&(_, _, x1, y1))) => (x0, y0, x1, y1),
                _ => (0, 0, 0, 0),
            };
            vec![vec![area]]
        }
    }
}

#[cfg(test)]
mod layout_tests {
    use super::*;

    #[test]
    fn pane_layout_splits_evenly_and_keeps_remainders() {
        let rects = pane_layout(1, 1, 101, 41, &[30, 10], &[58, 42]);
        assert_eq!(rects.len(), 2);
        assert_eq!(rects[0][0], (1, 1, 59, 31));
        assert_eq!(rects[0][1], (59, 1, 101, 31));
        // The last row absorbs the height remainder (30 + 10 = 40 < 40? no:
        // exactly 40 of 40), the last column the width remainder.
        assert_eq!(rects[1][0], (1, 31, 59, 41));
    }

    #[test]
    fn zoom_blows_one_pane_full() {
        let rects = pane_layout(0, 0, 100, 40, &[20, 20], &[50, 50]);
        let zoomed = zoom_rect(&rects, Zoom::Pane(0));
        assert_eq!(zoomed.len(), 1);
        assert_eq!(zoomed[0][0], (0, 0, 100, 40));
        let plain = zoom_rect(&rects, Zoom::None);
        assert_eq!(plain[1][1], (50, 20, 100, 40));
    }
}
