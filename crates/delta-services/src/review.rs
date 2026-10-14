//! Evidence-quality audits and a deterministic, non-advisory review queue.
//! Port of `delta/review.py`.
//!
//! The module is deliberately read-only. It turns stored facts and the user's
//! active thesis falsifiers into things worth reviewing; it never scores a
//! security or suggests an action.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{Duration, NaiveDateTime};

use delta_core::db::Db;
use delta_core::models::Instrument;

use crate::error::ServiceError;
use crate::evidence::{evidence, falsifier_hit, EvidenceItem};

pub const PRICE_STALE_AFTER: Duration = Duration::days(3);
pub const NEWS_STALE_AFTER: Duration = Duration::days(14);
pub const PRIMARY_STALE_AFTER: Duration = Duration::days(30);
pub const EVIDENCE_WINDOW: Duration = Duration::days(30);
pub const MIN_NON_PRICE_ITEMS: usize = 3;
pub const MIN_SOURCES: usize = 2;

/// What the live registry knows about one data plugin — the fields
/// `review.py` reads off plugin objects (`enabled`, `market`). The Rust
/// registry keeps plugins stateless, so callers hand this in (see findings).
#[derive(Debug, Clone, Copy)]
pub struct PluginInfo {
    pub enabled: bool,
    /// `None` = works for any market.
    pub market: Option<&'static str>,
}

/// Summarised recency and diversity for one instrument (`EvidenceAudit`).
#[derive(Debug, Clone)]
pub struct EvidenceAudit {
    pub instrument_id: String,
    pub price_at: Option<NaiveDateTime>,
    pub news_at: Option<NaiveDateTime>,
    pub primary_at: Option<NaiveDateTime>,
    /// `not_configured` | `absent` | `fresh` | `stale`.
    pub primary_coverage: &'static str,
    pub non_price_items: usize,
    pub source_count: usize,
    pub warnings: Vec<String>,
}

/// One navigable reason to inspect evidence, ranked by [`review_queue`]
/// (`ReviewItem`).
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewItem {
    /// `primary_disclosure` | `falsifier` | `stale` | `thin_evidence`.
    pub kind: &'static str,
    pub instrument_id: String,
    pub title: String,
    pub detail: String,
    pub evidence_id: Option<String>,
    pub thesis_id: Option<String>,
    pub ts: Option<NaiveDateTime>,
}

/// Enabled primary sources that actually apply to an instrument
/// (`primary_sources_for`).
///
/// A source which is merely installed is not enough: disabled plugins and a
/// US-only source for an ASX instrument must not make coverage look missing.
pub fn primary_sources_for(
    plugins: &BTreeMap<String, PluginInfo>,
    universe: &[Instrument],
    instrument_id: &str,
) -> BTreeSet<String> {
    match universe.iter().find(|item| item.id == instrument_id) {
        None => BTreeSet::new(),
        Some(inst) => primary_sources(plugins, inst),
    }
}

/// Core of [`primary_sources_for`] without the universe lookup
/// (`_primary_sources`).
pub fn primary_sources(
    plugins: &BTreeMap<String, PluginInfo>,
    inst: &Instrument,
) -> BTreeSet<String> {
    let fallback_markets: &[(&str, &str)] = &[("sec_edgar", "us"), ("asx_announcements", "asx")];
    let mut configured = BTreeSet::new();
    for (name, plugin) in plugins {
        if !crate::evidence::PRIMARY_FILING_SOURCES.contains(&name.as_str()) || !plugin.enabled {
            continue;
        }
        let fallback = fallback_markets
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, market)| *market);
        let market = plugin.market.or(fallback);
        if market.is_none_or(|m| m.to_lowercase() == inst.market.to_lowercase()) {
            configured.insert(name.clone());
        }
    }
    configured
}

/// Summarise recency and diversity without inferring an exchange schedule
/// (`evidence_audit`).
///
/// `items` lets a caller that has already fetched the pool ([`review_queue`])
/// hand it in, avoiding a second full read per instrument.
pub fn evidence_audit(
    db: &Db,
    instrument_id: &str,
    now: NaiveDateTime,
    primary_sources: &BTreeSet<String>,
    items: Option<&[EvidenceItem]>,
) -> Result<EvidenceAudit, ServiceError> {
    let owned;
    let items: &[EvidenceItem] = match items {
        Some(items) => items,
        None => {
            owned = evidence(db, Some(instrument_id), None, None, 10_000, None)?;
            &owned
        }
    };
    let latest = |filter: &dyn Fn(&EvidenceItem) -> bool| -> Option<NaiveDateTime> {
        items
            .iter()
            .filter(|item| filter(item))
            .map(|item| item.ts)
            .max()
    };
    let price_at = latest(&|item| item.kind == "bar" && item.ts <= now);
    let news_at = latest(&|item| item.kind == "news" && item.ts <= now);
    let primary_at = latest(&|item| {
        item.kind == "filing" && item.ts <= now && primary_sources.contains(&item.source)
    });
    let recent: Vec<&EvidenceItem> = items
        .iter()
        .filter(|item| item.kind != "bar" && now - EVIDENCE_WINDOW <= item.ts && item.ts <= now)
        .collect();
    let coverage = if primary_sources.is_empty() {
        "not_configured"
    } else if primary_at.is_none() {
        "absent"
    } else if now - primary_at.expect("primary_at checked") > PRIMARY_STALE_AFTER {
        "stale"
    } else {
        "fresh"
    };
    let mut warnings: Vec<String> = Vec::new();
    append_stale(&mut warnings, "price", price_at, now, PRICE_STALE_AFTER);
    append_stale(&mut warnings, "news", news_at, now, NEWS_STALE_AFTER);
    match coverage {
        "not_configured" => warnings.push("no primary disclosure source configured".to_string()),
        "absent" => warnings.push("no primary disclosure found".to_string()),
        "stale" => warnings.push(format!(
            "latest primary disclosure is {} days old",
            days_since(now, primary_at)
        )),
        _ => {}
    }
    if recent.len() < MIN_NON_PRICE_ITEMS {
        warnings.push("thin recent evidence".to_string());
    }
    let sources: BTreeSet<&str> = recent.iter().map(|item| item.source.as_str()).collect();
    let source_count = sources.len();
    if source_count < MIN_SOURCES {
        warnings.push("low source diversity".to_string());
    }
    Ok(EvidenceAudit {
        instrument_id: instrument_id.to_string(),
        price_at,
        news_at,
        primary_at,
        primary_coverage: coverage,
        non_price_items: recent.len(),
        source_count,
        warnings,
    })
}

/// A stable priority queue of factual review prompts (`review_queue`).
///
/// `since` controls the "new disclosure" window. A caller passes the
/// session's captured last-seen value, rather than this service mutating it.
///
/// One pass over each instrument's evidence and a single memoised universe /
/// source map.
#[allow(clippy::too_many_arguments)]
pub fn review_queue(
    db: &Db,
    plugins: &BTreeMap<String, PluginInfo>,
    universe: &[Instrument],
    instrument_ids: &[String],
    since: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> Result<Vec<ReviewItem>, ServiceError> {
    let ids: Vec<String> = if instrument_ids.is_empty() {
        universe.iter().map(|inst| inst.id.clone()).collect()
    } else {
        instrument_ids.to_vec()
    };
    let since = since.unwrap_or(now - Duration::days(7));
    let sources: BTreeMap<String, BTreeSet<String>> = universe
        .iter()
        .map(|inst| (inst.id.clone(), primary_sources(plugins, inst)))
        .collect();
    let mut primary_items: Vec<ReviewItem> = Vec::new();
    let mut falsifiers: Vec<ReviewItem> = Vec::new();
    let mut stale: Vec<ReviewItem> = Vec::new();
    let mut thin: Vec<ReviewItem> = Vec::new();
    let active = active_theses(db)?;
    for instrument_id in ids {
        let items = evidence(db, Some(&instrument_id), None, None, 10_000, None)?;
        let empty = BTreeSet::new();
        let instrument_sources = sources.get(&instrument_id).unwrap_or(&empty);
        for item in &items {
            if item.kind == "filing"
                && instrument_sources.contains(&item.source)
                && since <= item.ts
                && item.ts <= now
            {
                primary_items.push(review_item(
                    "primary_disclosure",
                    &instrument_id,
                    item,
                    None,
                ));
            }
            if since <= item.ts && item.ts <= now {
                for (thesis_id, targets, terms) in &active {
                    if (targets.is_empty() || targets.iter().any(|t| t == &instrument_id))
                        && falsifier_hit(item, terms)
                    {
                        falsifiers.push(review_item(
                            "falsifier",
                            &instrument_id,
                            item,
                            Some(thesis_id),
                        ));
                    }
                }
            }
        }
        let audit = evidence_audit(db, &instrument_id, now, instrument_sources, Some(&items))?;
        let stale_warnings: Vec<String> = audit
            .warnings
            .iter()
            .filter(|w| starts_with_any(w, &["no primary", "latest", "no price", "no news"]))
            .cloned()
            .collect();
        if !stale_warnings.is_empty() {
            stale.push(ReviewItem {
                kind: "stale",
                instrument_id: instrument_id.clone(),
                title: "Coverage needs review".to_string(),
                detail: stale_warnings.join("; "),
                evidence_id: None,
                thesis_id: None,
                ts: None,
            });
        }
        let thin_warnings: Vec<String> = audit
            .warnings
            .iter()
            .filter(|w| matches!(w.as_str(), "thin recent evidence" | "low source diversity"))
            .cloned()
            .collect();
        if !thin_warnings.is_empty() {
            thin.push(ReviewItem {
                kind: "thin_evidence",
                instrument_id: instrument_id.clone(),
                title: "Evidence coverage is thin".to_string(),
                detail: thin_warnings.join("; "),
                evidence_id: None,
                thesis_id: None,
                ts: None,
            });
        }
    }
    Ok(dedupe_and_sort(&[primary_items, falsifiers, stale, thin]))
}

/// Active theses with casefolded falsifier terms (`_active_theses`).
type ActiveThesis = (String, Vec<String>, Vec<String>);

fn active_theses(db: &Db) -> Result<Vec<ActiveThesis>, ServiceError> {
    Ok(crate::theses::list_theses(db)?
        .into_iter()
        .filter(|thesis| thesis.status == "active" && !thesis.falsifiers.is_empty())
        .map(|thesis| {
            (
                thesis.id,
                thesis.targets,
                thesis
                    .falsifiers
                    .into_iter()
                    .filter(|term| !term.trim().is_empty())
                    .map(|term| term.to_lowercase())
                    .collect(),
            )
        })
        .collect())
}

fn review_item(
    kind: &'static str,
    instrument_id: &str,
    item: &EvidenceItem,
    thesis_id: Option<&str>,
) -> ReviewItem {
    let detail = if kind == "primary_disclosure" {
        item.source.clone()
    } else {
        "matches an active thesis falsifier".to_string()
    };
    ReviewItem {
        kind,
        instrument_id: instrument_id.to_string(),
        title: item.title.clone(),
        detail,
        evidence_id: Some(item.id.clone()),
        thesis_id: thesis_id.map(str::to_string),
        ts: Some(item.ts),
    }
}

fn starts_with_any(text: &str, prefixes: &[&str]) -> bool {
    prefixes.iter().any(|prefix| text.starts_with(prefix))
}

/// Deduplicate exact navigation targets while preserving priority groups
/// (`_dedupe_and_sort`).
fn dedupe_and_sort(groups: &[Vec<ReviewItem>]) -> Vec<ReviewItem> {
    let mut result: Vec<ReviewItem> = Vec::new();
    let mut seen: BTreeSet<(&'static str, String, Option<String>, Option<String>)> =
        BTreeSet::new();
    for group in groups {
        let mut sorted = group.clone();
        // (ts is None asc, ts desc, instrument, evidence id, thesis id).
        sorted.sort_by(|a, b| {
            a.ts.is_none()
                .cmp(&b.ts.is_none())
                .then_with(|| b.ts.cmp(&a.ts))
                .then_with(|| a.instrument_id.cmp(&b.instrument_id))
                .then_with(|| {
                    a.evidence_id
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.evidence_id.as_deref().unwrap_or(""))
                })
                .then_with(|| {
                    a.thesis_id
                        .as_deref()
                        .unwrap_or("")
                        .cmp(b.thesis_id.as_deref().unwrap_or(""))
                })
        });
        for item in sorted {
            let key = (
                item.kind,
                item.instrument_id.clone(),
                item.evidence_id.clone(),
                item.thesis_id.clone(),
            );
            if seen.insert(key) {
                result.push(item);
            }
        }
    }
    result
}

fn append_stale(
    warnings: &mut Vec<String>,
    label: &str,
    value: Option<NaiveDateTime>,
    now: NaiveDateTime,
    threshold: Duration,
) {
    match value {
        None => warnings.push(format!("no {label} data")),
        Some(value) => {
            let age = now - value;
            if age > threshold {
                warnings.push(format!(
                    "latest {label} is {} days old",
                    age.num_days().max(0)
                ));
            }
        }
    }
}

fn days_since(now: NaiveDateTime, value: Option<NaiveDateTime>) -> i64 {
    value.map_or(0, |value| (now - value).num_days().max(0))
}
