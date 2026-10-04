//! User-owned investment decision journal, compatible with `delta/decisions.py`.

use chrono::{NaiveDate, NaiveDateTime, Utc};
use delta_core::db::{decode_ts, Db};
use rusqlite::{params, OptionalExtension, Row};

use crate::{error::ServiceError, theses::get_thesis};

pub const DECISION_STATUSES: [&str; 3] = ["open", "retired", "reviewed"];

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

#[derive(Debug, Clone)]
pub struct DecisionReview {
    pub id: i64,
    pub decision_id: String,
    pub note: String,
    pub created_at: NaiveDateTime,
    pub status: Option<String>,
}

#[derive(Debug, Clone)]
pub struct DecisionInput {
    pub instrument_id: String,
    pub rationale: String,
    pub valuation_context: String,
    pub time_horizon: String,
    pub review_date: NaiveDate,
    pub invalidation_criteria: String,
    pub thesis_id: Option<String>,
}

pub fn ensure_decision_tables(db: &Db) -> Result<(), ServiceError> {
    db.conn().execute_batch(
        "CREATE TABLE IF NOT EXISTS decision (
        id VARCHAR NOT NULL PRIMARY KEY, instrument_id VARCHAR NOT NULL,
        rationale VARCHAR NOT NULL, valuation_context VARCHAR NOT NULL,
        time_horizon VARCHAR NOT NULL, review_date DATE NOT NULL,
        invalidation_criteria VARCHAR NOT NULL, thesis_id VARCHAR,
        thesis_claim_snapshot VARCHAR, created_at DATETIME NOT NULL,
        status VARCHAR NOT NULL DEFAULT 'open');
        CREATE INDEX IF NOT EXISTS ix_decision_instrument_id ON decision(instrument_id);
        CREATE INDEX IF NOT EXISTS ix_decision_review_date ON decision(review_date);
        CREATE INDEX IF NOT EXISTS ix_decision_thesis_id ON decision(thesis_id);
        CREATE INDEX IF NOT EXISTS ix_decision_status ON decision(status);
        CREATE TABLE IF NOT EXISTS decision_review (
        id INTEGER PRIMARY KEY, decision_id VARCHAR NOT NULL,
        note VARCHAR NOT NULL, created_at DATETIME NOT NULL, status VARCHAR);
        CREATE INDEX IF NOT EXISTS ix_decision_review_decision_id ON decision_review(decision_id);
        CREATE INDEX IF NOT EXISTS ix_decision_review_created_at ON decision_review(created_at);",
    )?;
    Ok(())
}

fn required(name: &str, value: &str) -> Result<String, ServiceError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(ServiceError::invalid(format!("{name} is required")));
    }
    Ok(trimmed.to_string())
}

fn validate(input: &DecisionInput) -> Result<(), ServiceError> {
    required("instrument_id", &input.instrument_id)?;
    required("rationale", &input.rationale)?;
    required("valuation_context", &input.valuation_context)?;
    required("time_horizon", &input.time_horizon)?;
    required("invalidation_criteria", &input.invalidation_criteria)?;
    Ok(())
}

fn thesis_snapshot(db: &Db, input: &DecisionInput) -> Result<Option<String>, ServiceError> {
    if let Some(id) = &input.thesis_id {
        let thesis = get_thesis(db, id)?;
        if !thesis.targets.is_empty() && !thesis.targets.contains(&input.instrument_id) {
            return Err(ServiceError::invalid(format!(
                "thesis {id} does not target {}",
                input.instrument_id
            )));
        }
        Ok(Some(thesis.claim))
    } else {
        Ok(None)
    }
}

fn date(raw: String) -> rusqlite::Result<NaiveDate> {
    NaiveDate::parse_from_str(&raw, "%Y-%m-%d").map_err(|_| rusqlite::Error::InvalidQuery)
}

fn timestamp(raw: String) -> rusqlite::Result<NaiveDateTime> {
    decode_ts(&raw).ok_or(rusqlite::Error::InvalidQuery)
}

fn decision_row(row: &Row<'_>) -> rusqlite::Result<Decision> {
    Ok(Decision {
        id: row.get(0)?,
        instrument_id: row.get(1)?,
        rationale: row.get(2)?,
        valuation_context: row.get(3)?,
        time_horizon: row.get(4)?,
        review_date: date(row.get(5)?)?,
        invalidation_criteria: row.get(6)?,
        thesis_id: row.get(7)?,
        thesis_claim_snapshot: row.get(8)?,
        created_at: timestamp(row.get(9)?)?,
        status: row.get(10)?,
    })
}

const COLUMNS: &str = "id,instrument_id,rationale,valuation_context,time_horizon,review_date,invalidation_criteria,thesis_id,thesis_claim_snapshot,created_at,status";

pub fn create_decision(db: &Db, input: &DecisionInput) -> Result<Decision, ServiceError> {
    ensure_decision_tables(db)?;
    validate(input)?;
    let snapshot = thesis_snapshot(db, input)?;
    let id: String = db
        .conn()
        .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
    let now = Utc::now()
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string();
    db.conn().execute("INSERT INTO decision (id,instrument_id,rationale,valuation_context,time_horizon,review_date,invalidation_criteria,thesis_id,thesis_claim_snapshot,created_at,status) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,'open')",
        params![id,input.instrument_id.trim(),input.rationale.trim(),input.valuation_context.trim(),input.time_horizon.trim(),input.review_date.to_string(),input.invalidation_criteria.trim(),input.thesis_id,snapshot,now])?;
    get_decision(db, &id)
}

pub fn get_decision(db: &Db, id: &str) -> Result<Decision, ServiceError> {
    ensure_decision_tables(db)?;
    db.conn()
        .query_row(
            &format!("SELECT {COLUMNS} FROM decision WHERE id=?1"),
            [id],
            decision_row,
        )
        .optional()?
        .ok_or_else(|| ServiceError::invalid(format!("unknown decision: {id}")))
}

pub fn list_decisions(
    db: &Db,
    instrument_id: Option<&str>,
    include_retired: bool,
) -> Result<Vec<Decision>, ServiceError> {
    ensure_decision_tables(db)?;
    let mut stmt = db.conn().prepare(&format!("SELECT {COLUMNS} FROM decision WHERE (?1 IS NULL OR instrument_id=?1) AND (?2=1 OR status!='retired') ORDER BY created_at DESC,id"))?;
    let rows = stmt
        .query_map(params![instrument_id, include_retired], decision_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn update_decision(db: &Db, id: &str, input: &DecisionInput) -> Result<Decision, ServiceError> {
    get_decision(db, id)?;
    validate(input)?;
    let snapshot = thesis_snapshot(db, input)?;
    db.conn().execute("UPDATE decision SET instrument_id=?1,rationale=?2,valuation_context=?3,time_horizon=?4,review_date=?5,invalidation_criteria=?6,thesis_id=?7,thesis_claim_snapshot=?8 WHERE id=?9",
        params![input.instrument_id.trim(),input.rationale.trim(),input.valuation_context.trim(),input.time_horizon.trim(),input.review_date.to_string(),input.invalidation_criteria.trim(),input.thesis_id,snapshot,id])?;
    get_decision(db, id)
}

pub fn delete_decision(db: &mut Db, id: &str) -> Result<(), ServiceError> {
    get_decision(db, id)?;
    let tx = db.conn_mut().transaction()?;
    tx.execute("DELETE FROM decision_review WHERE decision_id=?1", [id])?;
    tx.execute("DELETE FROM decision WHERE id=?1", [id])?;
    tx.commit()?;
    Ok(())
}

fn review_row(row: &Row<'_>) -> rusqlite::Result<DecisionReview> {
    Ok(DecisionReview {
        id: row.get(0)?,
        decision_id: row.get(1)?,
        note: row.get(2)?,
        created_at: timestamp(row.get(3)?)?,
        status: row.get(4)?,
    })
}

pub fn append_review(
    db: &mut Db,
    decision_id: &str,
    note: &str,
    status: Option<&str>,
) -> Result<DecisionReview, ServiceError> {
    get_decision(db, decision_id)?;
    let note = required("note", note)?;
    if let Some(status) = status {
        if !DECISION_STATUSES.contains(&status) {
            return Err(ServiceError::invalid(format!(
                "invalid decision status: {status}"
            )));
        }
    }
    let now = Utc::now()
        .naive_utc()
        .format("%Y-%m-%d %H:%M:%S%.6f")
        .to_string();
    let tx = db.conn_mut().transaction()?;
    tx.execute(
        "INSERT INTO decision_review (decision_id,note,created_at,status) VALUES (?1,?2,?3,?4)",
        params![decision_id, note, now, status],
    )?;
    let id = tx.last_insert_rowid();
    if let Some(status) = status {
        tx.execute(
            "UPDATE decision SET status=?1 WHERE id=?2",
            params![status, decision_id],
        )?;
    }
    tx.commit()?;
    let mut stmt = db
        .conn()
        .prepare("SELECT id,decision_id,note,created_at,status FROM decision_review WHERE id=?1")?;
    Ok(stmt.query_row([id], review_row)?)
}

pub fn review_history(db: &Db, decision_id: &str) -> Result<Vec<DecisionReview>, ServiceError> {
    get_decision(db, decision_id)?;
    let mut stmt = db.conn().prepare("SELECT id,decision_id,note,created_at,status FROM decision_review WHERE decision_id=?1 ORDER BY created_at,id")?;
    let rows = stmt
        .query_map([decision_id], review_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn due_reviews(db: &Db, as_of: NaiveDate) -> Result<Vec<Decision>, ServiceError> {
    ensure_decision_tables(db)?;
    let mut stmt = db.conn().prepare(&format!("SELECT {COLUMNS} FROM decision WHERE review_date<=?1 AND status!='retired' ORDER BY review_date,created_at,id"))?;
    let rows = stmt
        .query_map([as_of.to_string()], decision_row)?
        .collect::<Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn relink_thesis(db: &Db, old_id: &str, new_id: &str) -> Result<usize, ServiceError> {
    ensure_decision_tables(db)?;
    Ok(db.conn().execute(
        "UPDATE decision SET thesis_id=?1 WHERE thesis_id=?2",
        params![new_id, old_id],
    )?)
}
