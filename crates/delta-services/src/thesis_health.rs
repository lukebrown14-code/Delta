//! Deterministic thesis health: an evidence-based read, never a truth claim.
//! Port of `delta/thesis_health.py` plus `services.thesis_fleet`.
//!
//! The state is computed by a pure function over ACCEPTED evidence — the LLM
//! may draft prose about it later, but it never decides the state.
//!
//! `EvidenceItem` and `evidence_by_ids` live in [`crate::evidence`] (their
//! Python home is `delta/evidence.py`); they are re-exported here for the
//! health callers.

use std::collections::BTreeSet;

use chrono::{DateTime, Duration, NaiveDateTime, Utc};

use delta_core::db::Db;

pub use crate::evidence::{evidence_by_ids, EvidenceItem};
// The thesis tables are `theses.py`'s; re-exported for the health callers.
pub use crate::theses::ensure_tables;

/// Items at most this old count fully; older ones count half.
pub const RECENT_DAYS: i64 = 7;

/// `ThesisStatus` values.
pub const STATUSES: [&str; 3] = ["active", "concluded", "paused"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EvidenceSide {
    Support,
    Against,
    Neutral,
}

impl EvidenceSide {
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceSide::Support => "support",
            EvidenceSide::Against => "against",
            EvidenceSide::Neutral => "neutral",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "support" => Some(EvidenceSide::Support),
            "against" => Some(EvidenceSide::Against),
            "neutral" => Some(EvidenceSide::Neutral),
            _ => None,
        }
    }
}

/// A long-horizon claim plus the framing that makes it checkable (`Thesis`).
#[derive(Debug, Clone, PartialEq)]
pub struct Thesis {
    pub id: String,
    pub claim: String,
    pub scope: String,
    pub assumptions: Vec<String>,
    pub falsifiers: Vec<String>,
    pub targets: Vec<String>,
    pub time_horizon: String,
    pub created_at: NaiveDateTime,
    pub status: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    Emerging,
    Building,
    Mixed,
    Weakening,
    Challenged,
    Idle,
}

impl HealthState {
    pub fn as_str(self) -> &'static str {
        match self {
            HealthState::Emerging => "emerging",
            HealthState::Building => "building",
            HealthState::Mixed => "mixed",
            HealthState::Weakening => "weakening",
            HealthState::Challenged => "challenged",
            HealthState::Idle => "idle",
        }
    }
}

/// Textual markup colour for a state's badge (`_STYLES` / `state_style`).
pub fn state_style(state: HealthState) -> &'static str {
    match state {
        HealthState::Emerging => "cyan",
        HealthState::Building => "green",
        HealthState::Mixed => "yellow",
        HealthState::Weakening => "orange1",
        HealthState::Challenged => "red",
        HealthState::Idle => "dim",
    }
}

#[derive(Debug, Clone)]
pub struct HealthResult {
    pub state: HealthState,
    pub tilt: f64,
    pub support: usize,
    pub against: usize,
    pub neutral: usize,
    pub computed_at: NaiveDateTime,
    pub drivers: Vec<String>,
}

/// The health badge as the desk shows it (`badge_text`): state and tilt only.
pub fn badge_text(result: &HealthResult) -> String {
    format!("{} · tilt {:+.2}", result.state.as_str(), result.tilt)
}

/// Score a thesis over its accepted `(evidence item, side)` pairs
/// (`compute_health`).
///
/// Rules, in precedence order:
///
/// 1. Coverage — fewer than `min_coverage` items forces `emerging`.
/// 2. Freshness — nothing newer than `stale_days` forces `idle` (a single
///    fresh item prevents idle).
/// 3. Falsifiers — an item whose kind or title contains a falsifier string
///    (case-insensitive) forces `challenged`.
/// 4. Balance — support vs against, weighted 1.0 within the last
///    `RECENT_DAYS` and 0.5 older, mapped to tilt in `[-1, 1]`: `>= 0.25`
///    building, `<= -0.25` weakening, otherwise mixed.
///
/// `drivers` are the evidence ids that moved the result, most important
/// first (falsifier hits, then contribution and recency), capped at five.
pub fn compute_health(
    thesis: &Thesis,
    linked: &[(EvidenceItem, EvidenceSide)],
    now: NaiveDateTime,
    stale_days: i64,
    min_coverage: usize,
) -> HealthResult {
    let mut support = 0;
    let mut against = 0;
    let mut neutral = 0;
    let mut weighted_support = 0.0;
    let mut weighted_against = 0.0;
    let mut freshest: Option<NaiveDateTime> = None;
    let mut falsified = false;
    let mut ranked: Vec<(f64, NaiveDateTime, String)> = Vec::new();

    let recent_cutoff = now - Duration::days(RECENT_DAYS);
    let stale_cutoff = now - Duration::days(stale_days);
    let falsifiers: Vec<String> = thesis
        .falsifiers
        .iter()
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect();

    for (item, side) in linked {
        if freshest.is_none_or(|f| item.ts > f) {
            freshest = Some(item.ts);
        }
        match side {
            EvidenceSide::Support => support += 1,
            EvidenceSide::Against => against += 1,
            EvidenceSide::Neutral => neutral += 1,
        }

        let weight = if item.ts >= recent_cutoff { 1.0 } else { 0.5 };
        let hit = falsifier_hit(item, &falsifiers);
        falsified = falsified || hit;
        match side {
            EvidenceSide::Support => weighted_support += weight,
            EvidenceSide::Against => weighted_against += weight,
            EvidenceSide::Neutral => {}
        }

        let importance = (if hit { 2.0 } else { 0.0 })
            + (if *side != EvidenceSide::Neutral {
                weight
            } else {
                0.0
            });
        ranked.push((importance, item.ts, item.id.clone()));
    }

    let total = weighted_support + weighted_against;
    let tilt = if total == 0.0 {
        0.0
    } else {
        (weighted_support - weighted_against) / total
    };

    let state = if linked.len() < min_coverage {
        HealthState::Emerging
    } else if freshest.is_none_or(|f| f <= stale_cutoff) {
        HealthState::Idle
    } else if falsified {
        HealthState::Challenged
    } else if tilt >= 0.25 {
        HealthState::Building
    } else if tilt <= -0.25 {
        HealthState::Weakening
    } else {
        HealthState::Mixed
    };

    ranked.sort_by(|a, b| b.1.cmp(&a.1));
    ranked.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let drivers = ranked.into_iter().take(5).map(|(_, _, id)| id).collect();
    HealthResult {
        state,
        tilt,
        support,
        against,
        neutral,
        computed_at: now,
        drivers,
    }
}

fn falsifier_hit(item: &EvidenceItem, falsifiers: &[String]) -> bool {
    // Thesis health searches kind + title (the review queue's shared matcher
    // also includes body; see `delta.evidence.falsifier_hit`).
    if falsifiers.is_empty() {
        return false;
    }
    let haystack = format!("{} {}", item.kind, item.title).to_lowercase();
    falsifiers.iter().any(|t| haystack.contains(t))
}

/// One thesis and its computed health (or None with no accepted evidence).
#[derive(Debug, Clone)]
pub struct ThesisHealth {
    pub thesis: Thesis,
    pub result: Option<HealthResult>,
}

impl ThesisHealth {
    pub fn state(&self) -> HealthState {
        self.result
            .as_ref()
            .map_or(HealthState::Emerging, |r| r.state)
    }
}

/// Most at risk first: the order the Home theses box lists claims in
/// (`RISK_ORDER`).
fn risk_order(state: HealthState) -> usize {
    match state {
        HealthState::Challenged => 0,
        HealthState::Weakening => 1,
        HealthState::Mixed => 2,
        HealthState::Idle => 3,
        HealthState::Emerging => 4,
        HealthState::Building => 5,
    }
}

fn list_theses(db: &Db) -> Result<Vec<Thesis>, delta_core::db::DbError> {
    let mut stmt = db
        .conn()
        .prepare("SELECT id, claim, scope, assumptions, falsifiers, targets, time_horizon, created_at, status FROM thesis")?;
    let rows = stmt.query_map([], |row| {
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
    })?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}

/// Accepted evidence links for one thesis (`theses.evidence_for`).
fn accepted_links(
    db: &Db,
    thesis_id: &str,
) -> Result<Vec<(String, EvidenceSide)>, delta_core::db::DbError> {
    let mut stmt = db.conn().prepare(
        "SELECT evidence_id, side FROM thesis_evidence WHERE thesis_id = ?1 AND accepted = 1",
    )?;
    let rows = stmt.query_map([thesis_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })?;
    Ok(rows
        .filter_map(|r| r.ok())
        .filter_map(|(id, side)| EvidenceSide::parse(&side).map(|s| (id, s)))
        .collect())
}

/// Health of every thesis, most at risk first (`services.thesis_fleet`).
///
/// Ties break on tilt (most negative first), then on the claim.
pub fn thesis_fleet(
    db: &Db,
    now: Option<NaiveDateTime>,
) -> Result<Vec<ThesisHealth>, delta_core::db::DbError> {
    let now = now.unwrap_or_else(|| Utc::now().naive_utc());
    ensure_tables(db)?;
    let mut fleet: Vec<ThesisHealth> = Vec::new();
    for thesis in list_theses(db)? {
        let links = accepted_links(db, &thesis.id)?;
        let ids: Vec<String> = links.iter().map(|(id, _)| id.clone()).collect();
        let items = evidence_by_ids(db, &ids)?;
        let item_ids: BTreeSet<&String> = items.iter().map(|i| &i.id).collect();
        let accepted: Vec<(EvidenceItem, EvidenceSide)> = links
            .iter()
            .filter(|(id, _)| item_ids.contains(id))
            .filter_map(|(id, side)| {
                items
                    .iter()
                    .find(|i| &i.id == id)
                    .map(|item| (item.clone(), *side))
            })
            .collect();
        let result = if accepted.is_empty() {
            None
        } else {
            Some(compute_health(&thesis, &accepted, now, 14, 3))
        };
        fleet.push(ThesisHealth { thesis, result });
    }
    fleet.sort_by(|a, b| {
        risk_order(a.state())
            .cmp(&risk_order(b.state()))
            .then_with(|| {
                let at = a.result.as_ref().map_or(0.0, |r| r.tilt);
                let bt = b.result.as_ref().map_or(0.0, |r| r.tilt);
                at.partial_cmp(&bt).unwrap_or(std::cmp::Ordering::Equal)
            })
            .then_with(|| a.thesis.claim.cmp(&b.thesis.claim))
    });
    Ok(fleet)
}

/// Chrono naive datetimes are already UTC; helper for callers holding
/// timezone-aware values (mirrors `to_utc`).
pub fn to_utc(ts: DateTime<Utc>) -> NaiveDateTime {
    ts.with_timezone(&Utc).naive_utc()
}
