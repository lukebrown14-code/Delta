//! User-owned thesis records and evidence links, compatible with `delta/theses.py`.

use chrono::Utc;
use delta_core::{db::Db, ids::stable_id, json::to_json};
use rusqlite::{params, OptionalExtension};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

use delta_core::config::{read_env_value_named, AppConfig, ENV_PATH};
use delta_llm::{client::LlmClient, router::model_for};
use std::path::Path;

use crate::{
    error::ServiceError,
    evidence::{cite, evidence},
    thesis_health::{self, EvidenceSide, Thesis},
};

#[derive(Debug, Deserialize)]
struct CandidateDraft {
    evidence_id: String,
    side: EvidenceSide,
    note: String,
}

#[derive(Debug, Deserialize)]
struct ThesisDraft {
    #[serde(default)]
    candidates: Vec<CandidateDraft>,
}

/// Propose candidate links from gathered evidence; none are accepted by the model.
pub async fn propose_thesis_evidence(
    db: &mut Db,
    client: &LlmClient,
    cfg: &AppConfig,
    thesis_id: &str,
    limit: usize,
) -> Result<Vec<ThesisEvidence>, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let linked = thesis_evidence(db, &thesis.id, false)?
        .into_iter()
        .map(|link| link.evidence_id)
        .collect::<BTreeSet<_>>();
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    if thesis.targets.is_empty() {
        for item in evidence(db, None, limit)? {
            if seen.insert(item.id.clone()) {
                items.push(item);
            }
        }
    } else {
        for target in &thesis.targets {
            for item in evidence(db, Some(target), limit)? {
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
        }
    }
    items.sort_by(|a, b| b.ts.cmp(&a.ts).then_with(|| a.id.cmp(&b.id)));
    items.truncate(limit);
    items.retain(|item| !linked.contains(&item.id));
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let model = model_for(cfg, "thesis", None).map_err(|e| ServiceError::invalid(e.to_string()))?;
    let vars = json!({"claim":thesis.claim,"scope":thesis.scope,"assumptions":thesis.assumptions,"falsifiers":thesis.falsifiers,"time_horizon":thesis.time_horizon,"items":items.iter().map(|item|json!({"id":item.id,"cite":cite(item)})).collect::<Vec<_>>()});
    let schema = crate::schemas::thesis_draft();
    let (draft, _) = delta_llm::structured::structured::<ThesisDraft>(
        client,
        db,
        "thesis",
        &model,
        "thesis_v1.j2",
        &vars,
        Some(&schema),
    )
    .await
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    let available = items
        .into_iter()
        .map(|item| item.id)
        .collect::<BTreeSet<_>>();
    let mut proposed = Vec::new();
    let mut accepted_ids = BTreeSet::new();
    for candidate in draft.candidates {
        if available.contains(&candidate.evidence_id)
            && accepted_ids.insert(candidate.evidence_id.clone())
        {
            proposed.push(add_thesis_evidence(
                db,
                &thesis.id,
                &candidate.evidence_id,
                candidate.side,
                &candidate.note,
                Some(false),
            )?);
        }
    }
    Ok(proposed)
}

pub async fn propose_thesis_evidence_configured(
    db: &mut Db,
    cfg: &AppConfig,
    thesis_id: &str,
    limit: usize,
) -> Result<Vec<ThesisEvidence>, ServiceError> {
    let client = delta_llm::client::build_client(
        &cfg.llm_provider,
        &|name| read_env_value_named(name, Path::new(ENV_PATH)),
        std::time::Duration::from_secs(60),
        Some(cfg.llm_max_output_tokens),
        &cfg.llm_base_url,
        &cfg.llm_api_key_env,
    )
    .map_err(|e| ServiceError::invalid(e.to_string()))?;
    propose_thesis_evidence(db, &client, cfg, thesis_id, limit).await
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThesisEvidence {
    pub thesis_id: String,
    pub evidence_id: String,
    pub side: EvidenceSide,
    pub note: String,
    pub accepted: bool,
}

#[derive(Debug, Clone)]
pub struct ThesisEdit {
    pub claim: String,
    pub scope: String,
    pub assumptions: Vec<String>,
    pub falsifiers: Vec<String>,
    pub targets: Vec<String>,
    pub time_horizon: String,
    pub status: String,
}

pub fn thesis_id(claim: &str, scope: &str) -> String {
    stable_id(&["thesis", claim, scope])
}

fn required(name: &str, value: &str) -> Result<String, ServiceError> {
    let value = value.trim();
    if value.is_empty() {
        return Err(ServiceError::invalid(format!("{name} is required")));
    }
    Ok(value.to_string())
}

pub fn create_thesis(
    db: &Db,
    claim: &str,
    scope: &str,
    targets: &[String],
    time_horizon: &str,
) -> Result<Thesis, ServiceError> {
    create_thesis_with_fields(db, claim, scope, &[], &[], targets, time_horizon)
}

pub fn create_thesis_with_fields(
    db: &Db,
    claim: &str,
    scope: &str,
    assumptions: &[String],
    falsifiers: &[String],
    targets: &[String],
    time_horizon: &str,
) -> Result<Thesis, ServiceError> {
    let claim = required("claim", claim)?;
    thesis_health::ensure_tables(db)?;
    let id = thesis_id(&claim, scope);
    if db
        .conn()
        .query_row("SELECT 1 FROM thesis WHERE id=?1", [&id], |_| Ok(()))
        .optional()?
        .is_some()
    {
        return Err(ServiceError::invalid(format!("thesis {id} already exists")));
    }
    let now = Utc::now().naive_utc();
    db.conn().execute("INSERT INTO thesis (id,claim,scope,assumptions,falsifiers,targets,time_horizon,created_at,status) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,'active')",
        params![id,claim,scope,to_json(assumptions),to_json(falsifiers),to_json(targets),time_horizon,now.format("%Y-%m-%d %H:%M:%S%.6f").to_string()])?;
    get_thesis(db, &id)
}

pub fn list_theses(db: &Db) -> Result<Vec<Thesis>, ServiceError> {
    Ok(thesis_health::list_theses(db)?)
}

pub fn get_thesis(db: &Db, id: &str) -> Result<Thesis, ServiceError> {
    let found: Vec<_> = list_theses(db)?
        .into_iter()
        .filter(|t| t.id.starts_with(id))
        .collect();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(ServiceError::invalid(format!("unknown thesis: {id}"))),
        _ => Err(ServiceError::invalid(format!(
            "ambiguous thesis id prefix: {id}"
        ))),
    }
}

pub fn set_thesis_status(db: &Db, id: &str, status: &str) -> Result<Thesis, ServiceError> {
    if !thesis_health::STATUSES.contains(&status) {
        return Err(ServiceError::invalid(format!(
            "invalid thesis status: {status}"
        )));
    }
    let thesis = get_thesis(db, id)?;
    db.conn().execute(
        "UPDATE thesis SET status=?1 WHERE id=?2",
        params![status, thesis.id],
    )?;
    get_thesis(db, &thesis.id)
}

pub fn update_thesis(db: &mut Db, id: &str, edit: &ThesisEdit) -> Result<Thesis, ServiceError> {
    let old = get_thesis(db, id)?;
    let claim = required("claim", &edit.claim)?;
    if !thesis_health::STATUSES.contains(&edit.status.as_str()) {
        return Err(ServiceError::invalid(format!(
            "invalid thesis status: {}",
            edit.status
        )));
    }
    let new_id = thesis_id(&claim, &edit.scope);
    if new_id != old.id
        && db
            .conn()
            .query_row("SELECT 1 FROM thesis WHERE id=?1", [&new_id], |_| Ok(()))
            .optional()?
            .is_some()
    {
        return Err(ServiceError::invalid(format!(
            "thesis {new_id} already exists"
        )));
    }
    // A claim/scope edit changes the stable id. Move linked evidence and
    // current decision links together; historical claim snapshots stay put.
    crate::decisions::ensure_decision_tables(db)?;
    let tx = db.conn_mut().transaction()?;
    if new_id != old.id {
        tx.execute("INSERT INTO thesis (id,claim,scope,assumptions,falsifiers,targets,time_horizon,created_at,status) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![new_id,claim,edit.scope,to_json(&edit.assumptions),to_json(&edit.falsifiers),to_json(&edit.targets),edit.time_horizon,old.created_at.format("%Y-%m-%d %H:%M:%S%.6f").to_string(),edit.status])?;
        tx.execute(
            "UPDATE thesis_evidence SET thesis_id=?1 WHERE thesis_id=?2",
            params![new_id, old.id],
        )?;
        tx.execute(
            "UPDATE decision SET thesis_id=?1 WHERE thesis_id=?2",
            params![new_id, old.id],
        )?;
        tx.execute("DELETE FROM thesis WHERE id=?1", [&old.id])?;
    } else {
        tx.execute("UPDATE thesis SET claim=?1,scope=?2,assumptions=?3,falsifiers=?4,targets=?5,time_horizon=?6,status=?7 WHERE id=?8",
            params![claim,edit.scope,to_json(&edit.assumptions),to_json(&edit.falsifiers),to_json(&edit.targets),edit.time_horizon,edit.status,old.id])?;
    }
    tx.commit()?;
    get_thesis(db, &new_id)
}

pub fn add_thesis_evidence(
    db: &Db,
    thesis_id: &str,
    evidence_id: &str,
    side: EvidenceSide,
    note: &str,
    accepted: Option<bool>,
) -> Result<ThesisEvidence, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let evidence_id = required("evidence_id", evidence_id)?;
    if thesis_health::evidence_by_ids(db, std::slice::from_ref(&evidence_id))?.is_empty() {
        return Err(ServiceError::invalid(format!(
            "unknown evidence: {evidence_id}"
        )));
    }
    let previous: Option<bool> = db
        .conn()
        .query_row(
            "SELECT accepted FROM thesis_evidence WHERE thesis_id=?1 AND evidence_id=?2",
            params![thesis.id, evidence_id],
            |row| row.get(0),
        )
        .optional()?;
    let accepted = accepted.unwrap_or(previous.unwrap_or(false));
    db.conn().execute("INSERT INTO thesis_evidence (thesis_id,evidence_id,side,note,accepted) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(thesis_id,evidence_id) DO UPDATE SET side=excluded.side,note=excluded.note,accepted=excluded.accepted",
        params![thesis.id,evidence_id,side.as_str(),note,accepted])?;
    Ok(ThesisEvidence {
        thesis_id: thesis.id,
        evidence_id,
        side,
        note: note.to_string(),
        accepted,
    })
}

pub fn thesis_evidence(
    db: &Db,
    thesis_id: &str,
    accepted_only: bool,
) -> Result<Vec<ThesisEvidence>, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let mut stmt = db.conn().prepare("SELECT evidence_id,side,note,accepted FROM thesis_evidence WHERE thesis_id=?1 AND (?2=0 OR accepted=1) ORDER BY evidence_id")?;
    let rows = stmt.query_map(params![thesis.id, accepted_only], |row| {
        let side: String = row.get(1)?;
        Ok(ThesisEvidence {
            thesis_id: thesis.id.clone(),
            evidence_id: row.get(0)?,
            side: EvidenceSide::parse(&side).unwrap_or(EvidenceSide::Neutral),
            note: row.get(2)?,
            accepted: row.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn set_thesis_evidence_accepted(
    db: &Db,
    thesis_id: &str,
    evidence_id: &str,
    accepted: bool,
) -> Result<ThesisEvidence, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let changed = db.conn().execute(
        "UPDATE thesis_evidence SET accepted=?1 WHERE thesis_id=?2 AND evidence_id=?3",
        params![accepted, thesis.id, evidence_id],
    )?;
    if changed == 0 {
        return Err(ServiceError::invalid(format!(
            "unknown thesis evidence: {thesis_id}/{evidence_id}"
        )));
    }
    thesis_evidence(db, &thesis.id, false)?
        .into_iter()
        .find(|e| e.evidence_id == evidence_id)
        .ok_or_else(|| ServiceError::invalid("evidence update failed"))
}

pub fn remove_thesis_evidence(
    db: &Db,
    thesis_id: &str,
    evidence_id: &str,
) -> Result<(), ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let changed = db.conn().execute(
        "DELETE FROM thesis_evidence WHERE thesis_id=?1 AND evidence_id=?2",
        params![thesis.id, evidence_id],
    )?;
    if changed == 0 {
        return Err(ServiceError::invalid(format!(
            "unknown thesis evidence: {thesis_id}/{evidence_id}"
        )));
    }
    Ok(())
}
