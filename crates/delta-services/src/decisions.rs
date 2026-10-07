//! A user-owned, append-only investment decision journal.
//! Port of `delta/decisions.py`.
//!
//! The journal intentionally stores the investor's original framing separately
//! from later reviews. It is a record of process, not a recommendation
//! engine. Like [`crate::theses`], this module owns its tables and creates
//! them idempotently so existing databases need no migration step.

use std::sync::atomic::{AtomicU64, Ordering};

use chrono::{NaiveDate, NaiveDateTime, Utc};

use delta_core::db::Db;

use crate::error::ServiceError;
use crate::theses::db_err;

pub const DECISION_STATUSES: [&str; 3] = ["open", "retired", "reviewed"];

/// The immutable original framing plus its current review status (`Decision`).
#[derive(Debug, Clone)]
pub struct Decision {
    pub id: String,
    pub instrument_id: String,
    pub rationale: String,
    pub valuation_context: String,
    pub time_horizon: String,
    pub review_date: NaiveDate,
    pub invalidation_criteria: String,
    pub thesis_id: Option<String>,
    pub thesis_claim_snapshot: Option<String>,
    pub created_at: NaiveDateTime,
    pub status: String,
}

/// One dated, append-only follow-up to a decision (`DecisionReview`).
#[derive(Debug, Clone)]
pub struct DecisionReview {
    pub id: i64,
    pub decision_id: String,
    pub note: String,
    pub created_at: NaiveDateTime,
    pub status: Option<String>,
}

/// The journal tables, created when missing (SQLModel's `create_all` checks
/// first, so repeat-safe). Column-for-column with `delta/decisions.py`.
pub fn ensure_tables(db: &Db) -> Result<(), delta_core::db::DbError> {
    db.conn().execute_batch(
        "CREATE TABLE IF NOT EXISTS decision (
            id VARCHAR NOT NULL PRIMARY KEY,
            instrument_id VARCHAR NOT NULL,
            rationale VARCHAR NOT NULL,
            valuation_context VARCHAR NOT NULL,
            time_horizon VARCHAR NOT NULL,
            review_date DATE NOT NULL,
            invalidation_criteria VARCHAR NOT NULL,
            thesis_id VARCHAR,
            thesis_claim_snapshot VARCHAR,
            created_at DATETIME NOT NULL,
            status VARCHAR NOT NULL
        );
        CREATE INDEX IF NOT EXISTS ix_decision_instrument_id ON decision (instrument_id);
        CREATE INDEX IF NOT EXISTS ix_decision_review_date ON decision (review_date);
        CREATE INDEX IF NOT EXISTS ix_decision_thesis_id ON decision (thesis_id);
        CREATE INDEX IF NOT EXISTS ix_decision_status ON decision (status);
        CREATE TABLE IF NOT EXISTS decision_review (
            id INTEGER PRIMARY KEY,
            decision_id VARCHAR NOT NULL,
            note VARCHAR NOT NULL,
            created_at DATETIME NOT NULL,
            status VARCHAR
        );
        CREATE INDEX IF NOT EXISTS ix_decision_review_decision_id ON decision_review (decision_id);
        CREATE INDEX IF NOT EXISTS ix_decision_review_created_at ON decision_review (created_at);",
    )?;
    Ok(())
}

const DECISION_COLUMNS: &str =
    "id, instrument_id, rationale, valuation_context, time_horizon, review_date, \
     invalidation_criteria, thesis_id, thesis_claim_snapshot, created_at, status";

fn row_to_decision(row: &rusqlite::Row<'_>) -> Result<Decision, rusqlite::Error> {
    Ok(Decision {
        id: row.get(0)?,
        instrument_id: row.get(1)?,
        rationale: row.get(2)?,
        valuation_context: row.get(3)?,
        time_horizon: row.get(4)?,
        review_date: decode_date(&row.get::<_, String>(5)?),
        invalidation_criteria: row.get(6)?,
        thesis_id: row.get(7)?,
        thesis_claim_snapshot: row.get(8)?,
        created_at: delta_core::db::decode_ts(&row.get::<_, String>(9)?).unwrap_or_default(),
        status: row.get(10)?,
    })
}

fn decode_date(raw: &str) -> NaiveDate {
    NaiveDate::parse_from_str(raw, "%Y-%m-%d").unwrap_or_default()
}

fn row_to_review(row: &rusqlite::Row<'_>) -> Result<DecisionReview, rusqlite::Error> {
    Ok(DecisionReview {
        id: row.get(0)?,
        decision_id: row.get(1)?,
        note: row.get(2)?,
        created_at: delta_core::db::decode_ts(&row.get::<_, String>(3)?).unwrap_or_default(),
        status: row.get(4)?,
    })
}

/// `uuid4().hex`-shaped id: 32 lowercase hex chars, unique per call.
///
/// Finding (deviation): Rust has no approved uuid crate, so the id is the
/// first 32 hex chars of sha256 over wall-clock nanoseconds, the process id
/// and a process-lifetime counter. Same format and uniqueness contract; not a
/// v4-layout UUID. Nothing in Delta parses the id, so this is a safe
/// substitution.
pub fn new_decision_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = Utc::now()
        .timestamp_nanos_opt()
        .unwrap_or_default()
        .to_string();
    let pid = std::process::id().to_string();
    let hash = delta_core::ids::stable_id(&["decision", &nanos, &pid, &n.to_string()]);
    hash[..32].to_string()
}

fn required(name: &str, value: &str) -> Result<String, ServiceError> {
    let result = value.trim();
    if result.is_empty() {
        return Err(ServiceError::invalid(format!("{name} is required")));
    }
    Ok(result.to_string())
}

fn validate_status(status: Option<&str>) -> Result<Option<String>, ServiceError> {
    match status {
        None => Ok(None),
        Some(s) if DECISION_STATUSES.contains(&s) => Ok(Some(s.to_string())),
        Some(s) => Err(ServiceError::invalid(format!(
            "status must be one of {}; got '{s}'",
            DECISION_STATUSES.join(", ")
        ))),
    }
}

/// Return the current claim, rejecting a thesis scoped to another target
/// (`_linked_thesis`).
fn linked_thesis(db: &Db, thesis_id: &str, instrument_id: &str) -> Result<String, ServiceError> {
    let thesis = crate::theses::get_thesis(db, thesis_id)?;
    if !thesis.targets.is_empty() && !thesis.targets.iter().any(|t| t == instrument_id) {
        return Err(ServiceError::invalid(format!(
            "thesis {thesis_id} does not target {instrument_id}"
        )));
    }
    Ok(thesis.claim)
}

/// Create an open decision with immutable original framing
/// (`create_decision`).
///
/// If a thesis is supplied it must cover the instrument (unless it is a
/// global thesis); its claim is snapshotted for historical display.
#[allow(clippy::too_many_arguments)]
pub fn create_decision(
    db: &Db,
    instrument_id: &str,
    rationale: &str,
    valuation_context: &str,
    time_horizon: &str,
    review_date: NaiveDate,
    invalidation_criteria: &str,
    thesis_id: Option<&str>,
    created_at: Option<NaiveDateTime>,
) -> Result<Decision, ServiceError> {
    let instrument_id = required("instrument_id", instrument_id)?;
    let rationale = required("rationale", rationale)?;
    let valuation_context = required("valuation_context", valuation_context)?;
    let time_horizon = required("time_horizon", time_horizon)?;
    let invalidation_criteria = required("invalidation_criteria", invalidation_criteria)?;
    ensure_tables(db)?;
    let snapshot = match thesis_id {
        Some(tid) => Some(linked_thesis(db, tid, &instrument_id)?),
        None => None,
    };
    let now = created_at.unwrap_or_else(|| Utc::now().naive_utc());
    let decision = Decision {
        id: new_decision_id(),
        instrument_id,
        rationale,
        valuation_context,
        time_horizon,
        review_date,
        invalidation_criteria,
        thesis_id: thesis_id.map(str::to_string),
        thesis_claim_snapshot: snapshot,
        created_at: now,
        status: "open".to_string(),
    };
    db.conn()
        .execute(
            "INSERT INTO decision (id, instrument_id, rationale, valuation_context, time_horizon, \
         review_date, invalidation_criteria, thesis_id, thesis_claim_snapshot, created_at, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
            rusqlite::params![
                decision.id,
                decision.instrument_id,
                decision.rationale,
                decision.valuation_context,
                decision.time_horizon,
                decision.review_date.format("%Y-%m-%d").to_string(),
                decision.invalidation_criteria,
                decision.thesis_id,
                decision.thesis_claim_snapshot,
                crate::theses::encode_ts(decision.created_at),
                decision.status,
            ],
        )
        .map_err(db_err)?;
    Ok(decision)
}

/// Return one decision or an error when it does not exist (`get_decision`).
pub fn get_decision(db: &Db, id: &str) -> Result<Decision, ServiceError> {
    ensure_tables(db)?;
    db.conn()
        .query_row(
            &format!("SELECT {DECISION_COLUMNS} FROM decision WHERE id = ?1"),
            [id],
            row_to_decision,
        )
        .map_err(|_| ServiceError::invalid(format!("unknown decision: {id}")))
}

/// List newest decisions first, optionally narrowed to one instrument
/// (`list_decisions`).
pub fn list_decisions(
    db: &Db,
    instrument_id: Option<&str>,
    include_retired: bool,
) -> Result<Vec<Decision>, ServiceError> {
    ensure_tables(db)?;
    let mut sql = format!("SELECT {DECISION_COLUMNS} FROM decision WHERE 1=1");
    if instrument_id.is_some() {
        sql.push_str(" AND instrument_id = ?1");
    }
    if !include_retired {
        sql.push_str(" AND status != 'retired'");
    }
    sql.push_str(" ORDER BY created_at DESC, id ASC");
    let mut stmt = db.conn().prepare(&sql).map_err(db_err)?;
    let params = instrument_id.map(str::to_string);
    let rows = stmt
        .query_map(rusqlite::params_from_iter(params.iter()), row_to_decision)
        .map_err(db_err)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Edit a decision's framing, preserving its identity and open timestamp
/// (`update_decision`).
///
/// The journal is append-only in the sense that reviews are stacked, not that
/// a typo in the original framing can never be fixed; the created timestamp
/// and id are kept so the review history stays anchored to the same record.
#[allow(clippy::too_many_arguments)]
pub fn update_decision(
    db: &Db,
    decision_id: &str,
    instrument_id: &str,
    rationale: &str,
    valuation_context: &str,
    time_horizon: &str,
    review_date: NaiveDate,
    invalidation_criteria: &str,
    thesis_id: Option<&str>,
) -> Result<Decision, ServiceError> {
    let instrument_id = required("instrument_id", instrument_id)?;
    let rationale = required("rationale", rationale)?;
    let valuation_context = required("valuation_context", valuation_context)?;
    let time_horizon = required("time_horizon", time_horizon)?;
    let invalidation_criteria = required("invalidation_criteria", invalidation_criteria)?;
    ensure_tables(db)?;
    let snapshot = match thesis_id {
        Some(tid) => Some(linked_thesis(db, tid, &instrument_id)?),
        None => None,
    };
    let updated = db
        .conn()
        .execute(
            "UPDATE decision SET instrument_id = ?2, rationale = ?3, valuation_context = ?4, \
         time_horizon = ?5, review_date = ?6, invalidation_criteria = ?7, thesis_id = ?8, \
         thesis_claim_snapshot = ?9 WHERE id = ?1",
            rusqlite::params![
                decision_id,
                instrument_id,
                rationale,
                valuation_context,
                time_horizon,
                review_date.format("%Y-%m-%d").to_string(),
                invalidation_criteria,
                thesis_id,
                snapshot,
            ],
        )
        .map_err(db_err)?;
    if updated == 0 {
        return Err(ServiceError::invalid(format!(
            "unknown decision: {decision_id}"
        )));
    }
    get_decision(db, decision_id)
}

/// Remove a decision and its review history; unknown ids error
/// (`delete_decision`).
pub fn delete_decision(db: &Db, decision_id: &str) -> Result<(), ServiceError> {
    ensure_tables(db)?;
    let known: bool = db
        .conn()
        .query_row(
            "SELECT 1 FROM decision WHERE id = ?1",
            [decision_id],
            |_| Ok(()),
        )
        .is_ok();
    if !known {
        return Err(ServiceError::invalid(format!(
            "unknown decision: {decision_id}"
        )));
    }
    let tx = db.conn().unchecked_transaction().map_err(db_err)?;
    tx.execute(
        "DELETE FROM decision_review WHERE decision_id = ?1",
        [decision_id],
    )
    .map_err(db_err)?;
    tx.execute("DELETE FROM decision WHERE id = ?1", [decision_id])
        .map_err(db_err)?;
    tx.commit().map_err(db_err)?;
    Ok(())
}

/// Append a review note; the original decision fields are never modified
/// (`append_review`).
pub fn append_review(
    db: &Db,
    decision_id: &str,
    note: &str,
    status: Option<&str>,
    created_at: Option<NaiveDateTime>,
) -> Result<DecisionReview, ServiceError> {
    let note = required("note", note)?;
    let valid_status = validate_status(status)?;
    ensure_tables(db)?;
    let known: bool = db
        .conn()
        .query_row(
            "SELECT 1 FROM decision WHERE id = ?1",
            [decision_id],
            |_| Ok(()),
        )
        .is_ok();
    if !known {
        return Err(ServiceError::invalid(format!(
            "unknown decision: {decision_id}"
        )));
    }
    let when = crate::theses::encode_ts(created_at.unwrap_or_else(|| Utc::now().naive_utc()));
    let tx = db.conn().unchecked_transaction().map_err(db_err)?;
    tx.execute(
        "INSERT INTO decision_review (decision_id, note, created_at, status) \
         VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![decision_id, note, when, valid_status],
    )
    .map_err(db_err)?;
    if let Some(status) = &valid_status {
        tx.execute(
            "UPDATE decision SET status = ?2 WHERE id = ?1",
            rusqlite::params![decision_id, status],
        )
        .map_err(db_err)?;
    }
    tx.commit().map_err(db_err)?;
    let id = db.conn().last_insert_rowid();
    Ok(DecisionReview {
        id,
        decision_id: decision_id.to_string(),
        note,
        created_at: delta_core::db::decode_ts(&when).unwrap_or_default(),
        status: valid_status,
    })
}

/// All reviews in chronological order; unknown decisions error
/// (`review_history`).
pub fn review_history(db: &Db, decision_id: &str) -> Result<Vec<DecisionReview>, ServiceError> {
    ensure_tables(db)?;
    let known: bool = db
        .conn()
        .query_row(
            "SELECT 1 FROM decision WHERE id = ?1",
            [decision_id],
            |_| Ok(()),
        )
        .is_ok();
    if !known {
        return Err(ServiceError::invalid(format!(
            "unknown decision: {decision_id}"
        )));
    }
    let mut stmt = db
        .conn()
        .prepare(
            "SELECT id, decision_id, note, created_at, status FROM decision_review \
         WHERE decision_id = ?1 ORDER BY created_at, id",
        )
        .map_err(db_err)?;
    let rows = stmt
        .query_map([decision_id], row_to_review)
        .map_err(db_err)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Open/reviewed decisions due on or before `as_of`, earliest first
/// (`due_reviews`).
pub fn due_reviews(db: &Db, as_of: Option<NaiveDate>) -> Result<Vec<Decision>, ServiceError> {
    ensure_tables(db)?;
    let cutoff = as_of.unwrap_or_else(|| Utc::now().date_naive());
    let mut stmt = db
        .conn()
        .prepare(&format!(
            "SELECT {DECISION_COLUMNS} FROM decision \
         WHERE review_date <= ?1 AND status != 'retired' \
         ORDER BY review_date, created_at, id"
        ))
        .map_err(db_err)?;
    let rows = stmt
        .query_map([cutoff.format("%Y-%m-%d").to_string()], row_to_decision)
        .map_err(db_err)?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Move current thesis links after a thesis rename, preserving snapshots
/// (`relink_thesis`).
///
/// Call this from the thesis rename transaction boundary. Historical claim
/// snapshots deliberately remain unchanged.
pub fn relink_thesis(
    db: &Db,
    old_id: &str,
    new_id: &str,
    _new_claim: &str,
) -> Result<usize, ServiceError> {
    ensure_tables(db)?;
    let count = db
        .conn()
        .execute(
            "UPDATE decision SET thesis_id = ?2 WHERE thesis_id = ?1",
            rusqlite::params![old_id, new_id],
        )
        .map_err(db_err)?;
    Ok(count)
}
