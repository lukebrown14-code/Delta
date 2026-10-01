//! Pipeline services shared by the TUI (port of `delta/services.py`).

pub mod analytics;
pub mod brief;
pub mod config_ops;
pub mod error;
pub mod fixture;
pub mod pipeline;
pub mod sentiment;
pub mod targets;
pub mod thesis_health;

pub use analytics::{
    data_health, latest_headline, latest_report, llm_costs, pulse, recent_closes, total_spend,
    upcoming_events, CostRow, DataHealth, Headline, Pulse, ReportStamp, Upcoming, FILING_SOURCE,
};
pub use brief::{brief_for, build_brief, Brief, Section, PRIMARY_FILING_SOURCES};
pub use config_ops::{
    add_target, legacy_kind, market_profiles, remove_market, remove_target, save_market,
    set_plugin_enabled, target_specs,
};
pub use error::ServiceError;
pub use pipeline::{
    default_since, event_id, extract_events, gather, ingest, ExtractResult, IngestResult,
};
pub use sentiment::{
    classify_news, judgment_id, stance_of, stock_sentiment, SentimentRow, SentimentSummary,
    DEFAULT_SINCE_DAYS, HALF_LIFE_DAYS, SENTIMENT_TASK,
};
pub use targets::{target_from_spec, WatchTarget, DEFAULT_KIND, KNOWN_KINDS, LEGACY_KIND};
pub use thesis_health::{
    badge_text, compute_health, evidence_by_ids, state_style, thesis_fleet, EvidenceItem,
    EvidenceSide, HealthResult, HealthState, Thesis, ThesisHealth, RECENT_DAYS,
};
