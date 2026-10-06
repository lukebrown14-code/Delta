//! The seven panes (1-6 plus `c`): deterministic painters over the golden
//! [`crate::screen::Screen`] model, one module per screen plus the shared
//! status-bar painters. Port of `delta/tui/screens/`; the golden exporter
//! (`tests/export_golden.py`) captures these layouts at 80x24, 120x40 and
//! 200x50.

mod ask;
mod decisions;
mod home;
mod research;
mod settings;
mod status_bar;
mod theses;
mod watchlist;

pub use ask::{draw_ask, draw_ask_narrow, draw_ask_wide};
pub use decisions::{draw_decisions, draw_decisions_narrow, draw_decisions_wide};
pub use home::{draw_home, draw_home_narrow, draw_home_wide, feed_seed, HomeFeed, HomeState};
pub use research::{draw_research, draw_research_narrow, draw_research_wide};
pub use settings::{draw_settings, draw_settings_narrow, draw_settings_wide};
pub use status_bar::NarrowTab;
pub use theses::{draw_theses, draw_theses_narrow, draw_theses_wide};
pub use watchlist::{
    chart_window, draw_glossary_overlay, draw_watchlist, draw_watchlist_narrow,
    draw_watchlist_wide, friendly_date, friendly_date_range, grouped, range_label, range_window,
    MetricsData, WatchlistState, GLOSSARY_EQUITY, RANGES,
};
