//! Pipeline services shared by the TUI (port of `delta/services.py`).

pub mod analytics;
pub mod config_ops;
pub mod error;
pub mod fixture;
pub mod pipeline;
pub mod targets;

pub use analytics::{
    data_health, latest_headline, latest_report, llm_costs, pulse, recent_closes, total_spend,
    upcoming_events, CostRow, DataHealth, Headline, Pulse, ReportStamp, Upcoming, FILING_SOURCE,
};
pub use config_ops::{
    add_target, legacy_kind, market_profiles, remove_market, remove_target, save_market,
    set_plugin_enabled, target_specs,
};
pub use error::ServiceError;
pub use pipeline::{
    default_since, event_id, extract_events, gather, ingest, ExtractResult, IngestResult,
};
pub use targets::{target_from_spec, WatchTarget, DEFAULT_KIND, KNOWN_KINDS, LEGACY_KIND};
