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
pub use chat::{
    chat, chat_configured, ChatDraft, ChatMessage, OfflineSearchTool, SearchTool, WebHit,
};
pub use config_ops::{
    add_target, configure_data_provider, configured_universe, connect_provider, legacy_kind,
    market_profiles, model_catalog, remove_market, remove_target, save_llm_settings, save_market,
    set_plugin_enabled, target_specs, ProviderSetup,
};
pub use decisions::{
    append_review, create_decision, delete_decision, due_reviews, get_decision, list_decisions,
    relink_thesis, review_history, update_decision, Decision, DecisionInput, DecisionReview,
};
pub use error::ServiceError;
pub use evidence::{cite, evidence, evidence_filtered, source_url};
pub use pipeline::{
    default_since, event_id, extract_events, gather, gather_configured, ingest, ExtractResult,
    GatherResult, IngestResult,
};
pub use reports::{
    build_report, generate_report_configured, read_report, render_markdown, report_history,
    write_report, Claim, Report, ReportDraft,
};
pub use review::{
    audit_items, evidence_audit, primary_sources_for, review_queue, EvidenceAudit, ReviewItem,
};
pub use sentiment::{
    classify_news, judgment_id, stance_of, stock_sentiment, SentimentRow, SentimentSummary,
    DEFAULT_SINCE_DAYS, HALF_LIFE_DAYS, SENTIMENT_TASK,
};
pub use targets::{target_from_spec, WatchTarget, DEFAULT_KIND, KNOWN_KINDS, LEGACY_KIND};
pub use theses::{
    add_thesis_evidence, create_thesis, create_thesis_with_fields, get_thesis, list_theses,
    propose_thesis_evidence, propose_thesis_evidence_configured, remove_thesis_evidence,
    set_thesis_evidence_accepted, set_thesis_status, thesis_evidence, thesis_id, update_thesis,
    ThesisEdit, ThesisEvidence,
};
pub use thesis_health::{
    badge_text, compute_health, evidence_by_ids, state_style, thesis_fleet, EvidenceItem,
    EvidenceSide, HealthResult, HealthState, Thesis, ThesisHealth, RECENT_DAYS,
};
pub use thesis_summary::{
    summarize_thesis, summarize_thesis_configured, SummaryDraft, ThesisSummary,
};
