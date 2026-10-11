//! Pipeline services shared by the TUI (port of `delta/services.py`).

pub mod analytics;
pub mod asset_metrics;
pub mod brief;
pub mod chat;
pub mod config_ops;
pub mod decisions;
pub mod error;
pub mod evidence;
pub mod fixture;
pub mod pipeline;
pub mod reports;
pub mod review;
pub mod schemas;
pub mod sentiment;
pub mod setup;
pub mod targets;
pub mod theses;
pub mod thesis_health;
pub mod thesis_summary;

pub use analytics::{
    data_health, latest_headline, latest_report, llm_costs, pulse, recent_closes, total_spend,
    upcoming_events, CostRow, DataHealth, Headline, Pulse, ReportStamp, Upcoming, FILING_SOURCE,
};
pub use asset_metrics::{
    fetch_asset_metrics, fetch_asset_metrics_configured, group_values, groups_for, metric_help,
    normalize_asset_metrics, profile_for, range_spec, AssetMetrics, MetricGroups,
};
pub use brief::{brief_for, build_brief, Brief, Section, PRIMARY_FILING_SOURCES};
pub use config_ops::{
    add_target, configured_universe, legacy_kind, market_profiles, remove_market, remove_target,
    save_market, set_plugin_enabled, target_specs,
};
pub use decisions::{
    append_review, create_decision, delete_decision, due_reviews, get_decision, list_decisions,
    new_decision_id, relink_thesis, review_history, update_decision, Decision, DecisionReview,
    DECISION_STATUSES,
};
pub use error::ServiceError;
pub use evidence::{
    cite, evidence, evidence_by_ids, falsifier_hit, source_quality, EvidenceItem,
    PRIMARY_DISCLOSURE_SOURCES,
};
pub use pipeline::{
    default_since, event_id, extract_events, gather, gather_configured, ingest, ExtractResult,
    GatherResult, IngestResult,
};
pub use reports::{
    build_report, generate_report_configured, read_report, render_markdown, report_history,
    write_report, Claim, Report, ReportDraft,
};
pub use review::{
    evidence_audit, primary_sources, primary_sources_for, review_queue, EvidenceAudit, PluginInfo,
    ReviewItem, EVIDENCE_WINDOW, MIN_NON_PRICE_ITEMS, MIN_SOURCES, NEWS_STALE_AFTER,
    PRICE_STALE_AFTER, PRIMARY_STALE_AFTER,
};
pub use sentiment::{
    classify_news, judgment_id, stance_of, stock_sentiment, SentimentRow, SentimentSummary,
    DEFAULT_SINCE_DAYS, HALF_LIFE_DAYS, SENTIMENT_TASK,
};
pub use setup::{
    configure_data_provider, data_provider_status, setup_checks, Check, DataProviderStatus,
};
pub use targets::{target_from_spec, WatchTarget, DEFAULT_KIND, KNOWN_KINDS, LEGACY_KIND};
pub use theses::{
    accepted_items, add_evidence, create_thesis, ensure_tables, evidence_for, get_thesis,
    list_theses, propose_evidence, propose_evidence_configured, remove_evidence, set_accepted,
    set_status, thesis_id, update_thesis, ThesisEvidence, SIDES,
};
pub use thesis_health::{
    badge_text, compute_health, state_style, thesis_fleet, EvidenceSide, HealthResult, HealthState,
    Thesis, ThesisHealth, RECENT_DAYS,
};
pub use thesis_summary::{summarize_thesis, summarize_thesis_configured, ThesisSummary};
