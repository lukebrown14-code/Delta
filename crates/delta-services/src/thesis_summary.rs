//! AI running prose for a thesis, grounded in accepted evidence only.

use std::collections::BTreeSet;

use chrono::{NaiveDateTime, Utc};
use delta_core::config::AppConfig;
use delta_core::db::Db;
use delta_llm::client::LlmClient;
use delta_llm::router::model_for;
use serde::Deserialize;
use serde_json::json;
use std::path::Path;

use crate::error::ServiceError;
use crate::evidence::cite;
use crate::theses::{accepted_items, get_thesis};
use crate::thesis_health::{compute_health, EvidenceSide, HealthState};

#[derive(Debug, Deserialize)]
struct SummaryDraft {
    summary: String,
    #[serde(default)]
    strongest_support: String,
    #[serde(default)]
    strongest_counter: String,
    #[serde(default)]
    unknowns: Vec<String>,
    #[serde(default)]
    citations: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThesisSummary {
    pub thesis_id: String,
    pub claim: String,
    pub state: HealthState,
    pub summary: String,
    pub strongest_support: String,
    pub strongest_counter: String,
    pub unknowns: Vec<String>,
    pub citations: Vec<String>,
    pub as_of: NaiveDateTime,
}

pub async fn summarize_thesis_configured(
    db: &mut Db,
    cfg: &AppConfig,
    env_path: &Path,
    thesis_id: &str,
) -> Result<ThesisSummary, ServiceError> {
    let client = crate::theses::configured_client(cfg, env_path)?;
    summarize_thesis(db, &client, cfg, thesis_id).await
}

/// Draft prose about accepted evidence. The health state comes from the pure
/// health function; model citations outside the accepted set are dropped.
pub async fn summarize_thesis(
    db: &mut Db,
    client: &LlmClient,
    cfg: &AppConfig,
    thesis_id: &str,
) -> Result<ThesisSummary, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let linked = accepted_items(db, thesis_id, 10_000)?;
    let now = Utc::now().naive_utc();
    if linked.is_empty() {
        return Ok(ThesisSummary {
            thesis_id: thesis.id,
            claim: thesis.claim,
            state: HealthState::Emerging,
            summary:
                "No accepted evidence yet — accept candidate evidence to populate this summary."
                    .to_string(),
            strongest_support: String::new(),
            strongest_counter: String::new(),
            unknowns: Vec::new(),
            citations: Vec::new(),
            as_of: now,
        });
    }
    let paired = linked
        .iter()
        .filter_map(|(item, row)| EvidenceSide::parse(&row.side).map(|side| (item.clone(), side)))
        .collect::<Vec<_>>();
    let health = compute_health(&thesis, &paired, now, 14, 3);
    let model = model_for(cfg, "thesis_summary", None)
        .map_err(|err| ServiceError::invalid(err.to_string()))?;
    let vars = json!({
        "claim": thesis.claim, "scope": thesis.scope,
        "assumptions": thesis.assumptions, "falsifiers": thesis.falsifiers,
        "time_horizon": thesis.time_horizon,
        "state": health.state.as_str(), "tilt": health.tilt,
        "support": health.support, "against": health.against, "neutral": health.neutral,
        "items": linked.iter().map(|(item, row)| json!({
            "id": item.id, "side": row.side, "note": row.note, "cite": cite(item),
        })).collect::<Vec<_>>(),
    });
    let (draft, _) = delta_llm::structured::structured::<SummaryDraft>(
        client,
        db,
        "thesis_summary",
        &model,
        "thesis_summary_v1.j2",
        &vars,
        Some(&crate::schemas::summary_draft()),
    )
    .await
    .map_err(|err| ServiceError::invalid(err.to_string()))?;
    let allowed: BTreeSet<&str> = linked.iter().map(|(item, _)| item.id.as_str()).collect();
    let mut seen = BTreeSet::new();
    let citations = draft
        .citations
        .into_iter()
        .filter(|id| allowed.contains(id.as_str()) && seen.insert(id.clone()))
        .collect();
    Ok(ThesisSummary {
        thesis_id: thesis.id,
        claim: thesis.claim,
        state: health.state,
        summary: draft.summary,
        strongest_support: draft.strongest_support,
        strongest_counter: draft.strongest_counter,
        unknowns: draft.unknowns,
        citations,
        as_of: health.computed_at,
    })
}
