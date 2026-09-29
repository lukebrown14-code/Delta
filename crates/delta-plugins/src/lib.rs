//! Delta data plugins: RSS feeds, ASX announcements, SEC EDGAR, Yahoo bars
//! and quotes (R1b).

pub mod asx;
pub mod http;
pub mod markets;
pub mod plugin;
pub mod rss;
pub mod sec;
pub mod yahoo;

pub use asx::{announcement_id, AsxAnnouncements};
pub use markets::{AsxMarket, UsMarket};
pub use plugin::{default_plugins, parse_scope, DataPlugin, PluginError, Rows, Scope};
pub use rss::{news_id, parse_feed, strip_html, Matcher, RssData};
pub use sec::{facts_to_fundamentals, filings_to_news, SecEdgar};
pub use yahoo::{
    canonical_symbol, chart_bars, classify_yahoo_asset, parse_quote, parse_search, yf_symbol,
    YahooClient, YahooQuotes, YfinanceBars,
};
