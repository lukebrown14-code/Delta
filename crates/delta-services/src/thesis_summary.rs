//! Cited prose around deterministic thesis health, using accepted evidence.

use crate::{
    cite, compute_health, evidence_by_ids, get_thesis, thesis_evidence, HealthState, ServiceError,
};
use chrono::{NaiveDateTime, Utc};
use delta_core::{config::AppConfig, db::Db};
use delta_llm::{client::LlmClient, router::model_for};
use serde::Deserialize;
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Deserialize)]
pub struct SummaryDraft {
    pub summary: String,
    #[serde(default)]
    pub strongest_support: String,
    #[serde(default)]
    pub strongest_counter: String,
    #[serde(default)]
    pub unknowns: Vec<String>,
    #[serde(default)]
    pub citations: Vec<String>,
}

#[derive(Debug, Clone)]
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
    thesis_id: &str,
) -> Result<ThesisSummary, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let links = thesis_evidence(db, &thesis.id, true)?;
    let mut has_evidence = false;
    let ids = links
        .into_iter()
        .map(|link| link.evidence_id)
        .collect::<Vec<_>>();
    for chunk in ids.chunks(400) {
        if !evidence_by_ids(db, chunk)?.is_empty() {
            has_evidence = true;
            break;
        }
    }
    if !has_evidence {
        return Ok(empty_summary(thesis));
    }
    let client = delta_llm::client::build_client(
        &cfg.llm_provider,
        &|name| {
            delta_core::config::read_env_value_named(
                name,
                std::path::Path::new(delta_core::config::ENV_PATH),
            )
        },
        std::time::Duration::from_secs(60),
        Some(cfg.llm_max_output_tokens),
        &cfg.llm_base_url,
        &cfg.llm_api_key_env,
    )
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    summarize_thesis(db, &client, cfg, thesis_id).await
}

fn empty_summary(thesis: crate::Thesis) -> ThesisSummary {
    ThesisSummary {
        thesis_id: thesis.id,
        claim: thesis.claim,
        state: HealthState::Emerging,
        summary: "No accepted evidence yet — accept candidate evidence to populate this summary."
            .into(),
        strongest_support: String::new(),
        strongest_counter: String::new(),
        unknowns: vec![],
        citations: vec![],
        as_of: Utc::now().naive_utc(),
    }
}

pub async fn summarize_thesis(
    db: &mut Db,
    client: &LlmClient,
    cfg: &AppConfig,
    thesis_id: &str,
) -> Result<ThesisSummary, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let links = thesis_evidence(db, &thesis.id, true)?;
    let ids = links
        .iter()
        .map(|link| link.evidence_id.clone())
        .collect::<Vec<_>>();
    let mut by_id = BTreeMap::new();
    for chunk in ids.chunks(400) {
        for item in evidence_by_ids(db, chunk)? {
            by_id.insert(item.id.clone(), item);
        }
    }
    let linked = links
        .iter()
        .filter_map(|link| {
            by_id
                .get(&link.evidence_id)
                .map(|item| (item.clone(), link.side))
        })
        .collect::<Vec<_>>();
    let now = Utc::now().naive_utc();
    if linked.is_empty() {
        return Ok(empty_summary(thesis));
    }
    let health = compute_health(&thesis, &linked, now, 14, 3);
    let items=links.iter().filter_map(|link|by_id.get(&link.evidence_id).map(|item|json!({"id":item.id,"side":link.side.as_str(),"note":link.note,"cite":cite(item)}))).collect::<Vec<_>>();
    let vars = json!({"claim":thesis.claim,"scope":thesis.scope,"assumptions":thesis.assumptions,"falsifiers":thesis.falsifiers,"time_horizon":thesis.time_horizon,"state":health.state.as_str(),"tilt":health.tilt,"support":health.support,"against":health.against,"neutral":health.neutral,"items":items});
    let model =
        model_for(cfg, "thesis_summary", None).map_err(|e| ServiceError::invalid(e.to_string()))?;
    let schema = crate::schemas::summary_draft();
    let (draft, _) = delta_llm::structured::structured::<SummaryDraft>(
        client,
        db,
        "thesis_summary",
        &model,
        "thesis_summary_v1.j2",
        &vars,
        Some(&schema),
    )
    .await
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let mut seen = BTreeSet::new();
    let citations = draft
        .citations
        .into_iter()
        .filter(|id| by_id.contains_key(id) && seen.insert(id.clone()))
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
