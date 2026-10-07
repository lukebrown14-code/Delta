//! Optional long-horizon theses: a claim plus evidence for and against.
//! Port of `delta/theses.py`.
//!
//! A thesis is a claim the user wants researched. Evidence for its targets is
//! gathered from the unified evidence pool and a structured call proposes
//! *candidate* evidence (side + note), stored unaccepted: only the user
//! accepts candidates, so discovery never auto-accepts. The tables live in
//! this module rather than core/db so the stage ships without touching shared
//! files; every public function creates them first, idempotently.
//!
//! Not ported here: `propose_evidence` (the LLM discovery call and its
//! `ThesisDraft` payload). It lands with the theses screen stream, which owns
//! the prompt/structured-call wiring.

use chrono::{NaiveDateTime, Timelike, Utc};

use delta_core::db::Db;
use delta_core::ids::stable_id;

use crate::error::ServiceError;

/// Truncate sub-microsecond precision so in-memory timestamps equal their
/// `%.6f` SQLite round-trip (chrono's `round` feature is not enabled).
pub(crate) fn truncate_to_micros(ts: NaiveDateTime) -> NaiveDateTime {
    ts.with_nanosecond(ts.nanosecond() / 1_000 * 1_000)
        .unwrap_or(ts)
}
use crate::evidence::{evidence, EvidenceItem};
use crate::thesis_health::Thesis;

/// Sorted for stable "side must be one of ..." error messages.
pub const SIDES: [&str; 3] = ["against", "neutral", "support"];

/// Sorted for stable "status must be one of ..." error messages.
pub const STATUSES: [&str; 3] = ["active", "concluded", "paused"];

/// `rusqlite::Error` -> `ServiceError` (one hop through `DbError`).
pub(crate) fn db_err(error: rusqlite::Error) -> ServiceError {
    ServiceError::Db(error.into())
}

/// One evidence item linked to a thesis; a candidate until the user accepts
/// it (`ThesisEvidence`).
#[derive(Debug, Clone, PartialEq)]
pub struct ThesisEvidence {
    pub thesis_id: String,
    pub evidence_id: String,
    pub side: String,
    pub note: String,
    pub accepted: bool,
}

/// Stable id: the same claim with the same scope is the same thesis
/// (`thesis_id`).
pub fn thesis_id(claim: &str, scope: &str) -> String {
    stable_id(&["thesis", claim, scope])
}

/// The thesis-stage tables, created when missing (SQLModel's `create_all`
/// checks first, so repeat-safe). Column-for-column with `delta/theses.py`.
pub fn ensure_tables(db: &Db) -> Result<(), delta_core::db::DbError> {
    db.conn().execute_batch(
        "CREATE TABLE IF NOT EXISTS thesis (
            id VARCHAR NOT NULL PRIMARY KEY,
            claim VARCHAR NOT NULL,
            scope VARCHAR NOT NULL,
            assumptions VARCHAR NOT NULL,
            falsifiers VARCHAR NOT NULL,
            targets VARCHAR NOT NULL,
            time_horizon VARCHAR NOT NULL,
            created_at DATETIME NOT NULL,
            status VARCHAR NOT NULL
        );
        CREATE TABLE IF NOT EXISTS thesis_evidence (
            thesis_id VARCHAR NOT NULL,
            evidence_id VARCHAR NOT NULL,
            side VARCHAR NOT NULL,
            note VARCHAR NOT NULL,
            accepted BOOLEAN NOT NULL,
            PRIMARY KEY (thesis_id, evidence_id)
        );",
    )?;
    Ok(())
}

const THESIS_COLUMNS: &str =
    "id, claim, scope, assumptions, falsifiers, targets, time_horizon, created_at, status";

fn row_to_thesis(row: &rusqlite::Row<'_>) -> Result<Thesis, rusqlite::Error> {
    Ok(Thesis {
        id: row.get(0)?,
        claim: row.get(1)?,
        scope: row.get(2)?,
        assumptions: delta_core::json::from_json(Some(&row.get::<_, String>(3)?)),
        falsifiers: delta_core::json::from_json(Some(&row.get::<_, String>(4)?)),
        targets: delta_core::json::from_json(Some(&row.get::<_, String>(5)?)),
        time_horizon: row.get(6)?,
        created_at: delta_core::db::decode_ts(&row.get::<_, String>(7)?).unwrap_or_default(),
        status: row.get(8)?,
    })
}

/// Store a new active thesis; the same claim and scope twice is an error
/// (`create_thesis`).
///
/// `created_at` is a test seam (the Python tests freeze the clock); `None`
/// means now.
#[allow(clippy::too_many_arguments)]
pub fn create_thesis(
    db: &Db,
    claim: &str,
    scope: &str,
    assumptions: &[String],
    falsifiers: &[String],
    targets: &[String],
    time_horizon: &str,
    created_at: Option<NaiveDateTime>,
) -> Result<Thesis, ServiceError> {
    let tid = thesis_id(claim, scope);
    // Python clocks carry microsecond precision at most and SQLite stores
    // `%.6f`; truncate so the returned struct equals its stored round-trip
    // (nanosecond system clocks made `get_thesis` != the created value).
    let created_at = created_at
        .map(truncate_to_micros)
        .unwrap_or_else(|| truncate_to_micros(Utc::now().naive_utc()));
    ensure_tables(db)?;
    let exists: bool = db
        .conn()
        .query_row("SELECT 1 FROM thesis WHERE id = ?1", [&tid], |_| Ok(()))
        .is_ok();
    if exists {
        return Err(ServiceError::invalid(format!(
            "thesis {tid} already exists"
        )));
    }
    db.conn().execute(
        "INSERT INTO thesis (id, claim, scope, assumptions, falsifiers, targets, time_horizon, created_at, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'active')",
        rusqlite::params![
            tid,
            claim,
            scope,
            delta_core::json::to_json(assumptions),
            delta_core::json::to_json(falsifiers),
            delta_core::json::to_json(targets),
            time_horizon,
            encode_ts(created_at),
        ],
    ).map_err(db_err)?;
    Ok(Thesis {
        id: tid,
        claim: claim.to_string(),
        scope: scope.to_string(),
        assumptions: assumptions.to_vec(),
        falsifiers: falsifiers.to_vec(),
        targets: targets.to_vec(),
        time_horizon: time_horizon.to_string(),
        created_at,
        status: "active".to_string(),
    })
}

/// SQLModel's datetime storage format (`YYYY-MM-DD HH:MM:SS.ffffff`).
pub(crate) fn encode_ts(ts: NaiveDateTime) -> String {
    ts.format("%Y-%m-%d %H:%M:%S%.6f").to_string()
}

/// All theses, oldest first (`list_theses`).
pub fn list_theses(db: &Db) -> Result<Vec<Thesis>, ServiceError> {
    ensure_tables(db)?;
    let mut stmt = db
        .conn()
        .prepare(&format!(
            "SELECT {THESIS_COLUMNS} FROM thesis ORDER BY created_at ASC, id ASC"
        ))
        .map_err(db_err)?;
    let rows = stmt.query_map([], row_to_thesis).map_err(db_err)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// One thesis by id or unique id prefix; error when unknown (`get_thesis`).
///
/// The TUI shows ids truncated, so a prefix is what a user has to hand.
pub fn get_thesis(db: &Db, id: &str) -> Result<Thesis, ServiceError> {
    ensure_tables(db)?;
    let exact = db
        .conn()
        .query_row(
            &format!("SELECT {THESIS_COLUMNS} FROM thesis WHERE id = ?1"),
            [id],
            row_to_thesis,
        )
        .ok();
    if let Some(thesis) = exact {
        return Ok(thesis);
    }
    let mut stmt = db
        .conn()
        .prepare(&format!("SELECT {THESIS_COLUMNS} FROM thesis"))
        .map_err(db_err)?;
    // Python's `startswith` is literal; filter in Rust so `%`/`_` in a
    // user-typed prefix cannot act as LIKE wildcards.
    let matches: Vec<Thesis> = stmt
        .query_map([], row_to_thesis)
        .map_err(db_err)?
        .filter_map(|r| r.ok())
        .filter(|thesis| thesis.id.starts_with(id))
        .collect();
    match matches.len() {
        0 => Err(ServiceError::invalid(format!("unknown thesis: {id}"))),
        1 => Ok(matches.into_iter().next().expect("one match")),
        _ => Err(ServiceError::invalid(format!(
            "ambiguous thesis id prefix: {id}"
        ))),
    }
}

/// Pause, conclude, or reactivate a thesis; returns the updated model
/// (`set_status`).
pub fn set_status(db: &Db, id: &str, status: &str) -> Result<Thesis, ServiceError> {
    if !STATUSES.contains(&status) {
        return Err(ServiceError::invalid(format!(
            "status must be one of {}; got '{status}'",
            STATUSES.join(", ")
        )));
    }
    ensure_tables(db)?;
    let updated = db
        .conn()
        .execute(
            "UPDATE thesis SET status = ?2 WHERE id = ?1",
            rusqlite::params![id, status],
        )
        .map_err(db_err)?;
    if updated == 0 {
        return Err(ServiceError::invalid(format!("unknown thesis: {id}")));
    }
    get_thesis(db, id)
}

/// Update a thesis and retain its linked evidence if its identity changes
/// (`update_thesis`).
///
/// A thesis id is derived from its claim and scope. Editing either therefore
/// changes the id; evidence rows are moved in the same transaction so editing
/// does not silently discard a review history.
///
/// `scope`, `assumptions` and `falsifiers` default to `None`, meaning "leave
/// as stored" — a caller that does not collect a field cannot erase it.
#[allow(clippy::too_many_arguments)]
pub fn update_thesis(
    db: &Db,
    id: &str,
    claim: &str,
    targets: &[String],
    time_horizon: &str,
    status: &str,
    scope: Option<&str>,
    assumptions: Option<&[String]>,
    falsifiers: Option<&[String]>,
) -> Result<Thesis, ServiceError> {
    let claim = claim.trim();
    if claim.is_empty() {
        return Err(ServiceError::invalid("claim is required"));
    }
    if !STATUSES.contains(&status) {
        return Err(ServiceError::invalid(format!(
            "status must be one of {}; got '{status}'",
            STATUSES.join(", ")
        )));
    }
    ensure_tables(db)?;
    let existing = get_thesis_exact(db, id)?;
    let new_scope = match scope {
        Some(s) => s.trim().to_string(),
        None => existing.scope.clone(),
    };
    let new_assumptions = match assumptions {
        Some(list) => delta_core::json::to_json(list),
        None => delta_core::json::to_json(&existing.assumptions),
    };
    let new_falsifiers = match falsifiers {
        Some(list) => delta_core::json::to_json(list),
        None => delta_core::json::to_json(&existing.falsifiers),
    };
    let new_id = thesis_id(claim, &new_scope);
    let renamed = new_id != existing.id;
    if renamed {
        let taken: bool = db
            .conn()
            .query_row("SELECT 1 FROM thesis WHERE id = ?1", [&new_id], |_| Ok(()))
            .is_ok();
        if taken {
            return Err(ServiceError::invalid(format!(
                "thesis {new_id} already exists"
            )));
        }
    }
    let tx = db.conn().unchecked_transaction().map_err(db_err)?;
    if renamed {
        // Move every evidence row to the new identity, preserving side, note
        // and accepted flags.
        tx.execute(
            "UPDATE thesis_evidence SET thesis_id = ?2 WHERE thesis_id = ?1",
            rusqlite::params![existing.id, new_id],
        )
        .map_err(db_err)?;
        tx.execute(
            "DELETE FROM thesis WHERE id = ?1",
            rusqlite::params![existing.id],
        )
        .map_err(db_err)?;
        tx.execute(
            "INSERT INTO thesis (id, claim, scope, assumptions, falsifiers, targets, time_horizon, created_at, status) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                new_id,
                claim,
                new_scope,
                new_assumptions,
                new_falsifiers,
                delta_core::json::to_json(targets),
                time_horizon,
                encode_ts(existing.created_at),
                status,
            ],
        ).map_err(db_err)?;
    } else {
        tx.execute(
            "UPDATE thesis SET claim = ?2, scope = ?3, assumptions = ?4, falsifiers = ?5, \
             targets = ?6, time_horizon = ?7, status = ?8 WHERE id = ?1",
            rusqlite::params![
                id,
                claim,
                new_scope,
                new_assumptions,
                new_falsifiers,
                delta_core::json::to_json(targets),
                time_horizon,
                status,
            ],
        )
        .map_err(db_err)?;
    }
    tx.commit().map_err(db_err)?;
    let thesis = get_thesis(db, &new_id)?;
    if renamed {
        // Keep optional decision-journal links current while retaining each
        // decision's original claim snapshot for its historical rationale.
        crate::decisions::relink_thesis(db, id, &thesis.id, &thesis.claim)?;
    }
    Ok(thesis)
}

fn get_thesis_exact(db: &Db, id: &str) -> Result<Thesis, ServiceError> {
    db.conn()
        .query_row(
            &format!("SELECT {THESIS_COLUMNS} FROM thesis WHERE id = ?1"),
            [id],
            row_to_thesis,
        )
        .map_err(|_| ServiceError::invalid(format!("unknown thesis: {id}")))
}

/// Link evidence to a thesis, inserting or updating the (thesis, evidence)
/// row (`add_evidence`).
///
/// `accepted=None` keeps an existing link's accepted flag, so re-linking to
/// correct a side or note does not silently drop the item out of the thesis.
/// New links default to not accepted.
pub fn add_evidence(
    db: &Db,
    thesis_id: &str,
    evidence_id: &str,
    side: &str,
    note: &str,
    accepted: Option<bool>,
) -> Result<ThesisEvidence, ServiceError> {
    if !SIDES.contains(&side) {
        return Err(ServiceError::invalid(format!(
            "side must be one of {}; got '{side}'",
            SIDES.join(", ")
        )));
    }
    ensure_tables(db)?;
    let known: bool = db
        .conn()
        .query_row(
            "SELECT 1 FROM thesis WHERE id = ?1",
            [thesis_id],
            |_| Ok(()),
        )
        .is_ok();
    if !known {
        return Err(ServiceError::invalid(format!(
            "unknown thesis: {thesis_id}"
        )));
    }
    let existing: Option<(String, String, bool)> = db
        .conn()
        .query_row(
            "SELECT side, note, accepted FROM thesis_evidence WHERE thesis_id = ?1 AND evidence_id = ?2",
            rusqlite::params![thesis_id, evidence_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, i64>(2)? != 0)),
        )
        .ok();
    let effective = match (&existing, accepted) {
        (None, requested) => requested.unwrap_or(false),
        (Some((_, _, kept)), None) => *kept,
        (Some(_), Some(flag)) => flag,
    };
    if existing.is_none() {
        db.conn()
            .execute(
                "INSERT INTO thesis_evidence (thesis_id, evidence_id, side, note, accepted) \
             VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![thesis_id, evidence_id, side, note, effective],
            )
            .map_err(db_err)?;
    } else {
        db.conn()
            .execute(
                "UPDATE thesis_evidence SET side = ?3, note = ?4, accepted = ?5 \
             WHERE thesis_id = ?1 AND evidence_id = ?2",
                rusqlite::params![thesis_id, evidence_id, side, note, effective],
            )
            .map_err(db_err)?;
    }
    Ok(ThesisEvidence {
        thesis_id: thesis_id.to_string(),
        evidence_id: evidence_id.to_string(),
        side: side.to_string(),
        note: note.to_string(),
        accepted: effective,
    })
}

/// Confirm or un-confirm a candidate; only accepted evidence feeds the thesis
/// (`set_accepted`).
pub fn set_accepted(
    db: &Db,
    thesis_id: &str,
    evidence_id: &str,
    accepted: bool,
) -> Result<ThesisEvidence, ServiceError> {
    ensure_tables(db)?;
    let updated = db
        .conn()
        .execute(
            "UPDATE thesis_evidence SET accepted = ?3 WHERE thesis_id = ?1 AND evidence_id = ?2",
            rusqlite::params![thesis_id, evidence_id, accepted],
        )
        .map_err(db_err)?;
    if updated == 0 {
        return Err(ServiceError::invalid(format!(
            "unknown thesis evidence: {thesis_id}/{evidence_id}"
        )));
    }
    db.conn()
        .query_row(
            "SELECT thesis_id, evidence_id, side, note, accepted FROM thesis_evidence \
             WHERE thesis_id = ?1 AND evidence_id = ?2",
            rusqlite::params![thesis_id, evidence_id],
            |row| {
                Ok(ThesisEvidence {
                    thesis_id: row.get(0)?,
                    evidence_id: row.get(1)?,
                    side: row.get(2)?,
                    note: row.get(3)?,
                    accepted: row.get::<_, i64>(4)? != 0,
                })
            },
        )
        .map_err(|e| ServiceError::invalid(e.to_string()))
}

/// Drop a rejected candidate from the thesis (`remove_evidence`).
pub fn remove_evidence(db: &Db, thesis_id: &str, evidence_id: &str) -> Result<(), ServiceError> {
    ensure_tables(db)?;
    let updated = db
        .conn()
        .execute(
            "DELETE FROM thesis_evidence WHERE thesis_id = ?1 AND evidence_id = ?2",
            rusqlite::params![thesis_id, evidence_id],
        )
        .map_err(db_err)?;
    if updated == 0 {
        return Err(ServiceError::invalid(format!(
            "unknown thesis evidence: {thesis_id}/{evidence_id}"
        )));
    }
    Ok(())
}

/// Evidence linked to a thesis, id-ordered; candidates hidden unless asked
/// for (`evidence_for`).
pub fn evidence_for(
    db: &Db,
    thesis_id: &str,
    accepted_only: bool,
) -> Result<Vec<ThesisEvidence>, ServiceError> {
    ensure_tables(db)?;
    let sql = format!(
        "SELECT thesis_id, evidence_id, side, note, accepted FROM thesis_evidence \
         WHERE thesis_id = ?1{} ORDER BY evidence_id",
        if accepted_only {
            " AND accepted = 1"
        } else {
            ""
        }
    );
    let mut stmt = db.conn().prepare(&sql).map_err(db_err)?;
    let rows = stmt
        .query_map([thesis_id], |row| {
            Ok(ThesisEvidence {
                thesis_id: row.get(0)?,
                evidence_id: row.get(1)?,
                side: row.get(2)?,
                note: row.get(3)?,
                accepted: row.get::<_, i64>(4)? != 0,
            })
        })
        .map_err(db_err)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Accepted evidence resolved to `(EvidenceItem, ThesisEvidence)` pairs
/// (`accepted_items`).
///
/// Resolution runs over the thesis targets (all evidence when no targets),
/// the same pool discovery proposes from, so accepted ids align with stored
/// rows.
pub fn accepted_items(
    db: &Db,
    thesis_id: &str,
    limit: usize,
) -> Result<Vec<(EvidenceItem, ThesisEvidence)>, ServiceError> {
    let thesis = get_thesis(db, thesis_id)?;
    let rows = evidence_for(db, thesis_id, true)?;
    let targets: Vec<String> = thesis.targets.clone();
    let by_id: std::collections::BTreeMap<String, EvidenceItem> =
        gather(db, &targets, None, limit)?
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();
    Ok(rows
        .into_iter()
        .filter_map(|row| by_id.get(&row.evidence_id).cloned().map(|item| (item, row)))
        .collect())
}

/// Evidence across the thesis targets (all evidence when no targets), newest
/// first (`_gather`).
///
/// Per-target reads are deduped by id, re-ordered `ts` desc then `id` asc,
/// and capped at `limit` overall.
pub fn gather(
    db: &Db,
    targets: &[String],
    since: Option<&str>,
    limit: usize,
) -> Result<Vec<EvidenceItem>, ServiceError> {
    let mut items: Vec<EvidenceItem> = Vec::new();
    if !targets.is_empty() {
        for target in targets {
            items.extend(evidence(db, Some(target), since, None, limit, None)?);
        }
    } else {
        items.extend(evidence(db, None, since, None, limit, None)?);
    }
    let mut by_id: std::collections::BTreeMap<String, EvidenceItem> = Default::default();
    for item in items {
        by_id.entry(item.id.clone()).or_insert(item);
    }
    let mut merged: Vec<EvidenceItem> = by_id.into_values().collect();
    merged.sort_by(|a, b| a.id.cmp(&b.id));
    merged.sort_by(|a, b| b.ts.cmp(&a.ts));
    merged.truncate(limit);
    Ok(merged)
}
