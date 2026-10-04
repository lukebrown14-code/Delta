//! Read-only evidence coverage audits and factual review prompts.

use crate::{evidence, list_theses, EvidenceItem, ServiceError};
use chrono::{Duration, NaiveDateTime};
use delta_core::{config::AppConfig, db::Db, models::Instrument};
use std::collections::BTreeSet;

#[derive(Debug, Clone)]
pub struct EvidenceAudit {
    pub instrument_id: String,
    pub price_at: Option<NaiveDateTime>,
    pub news_at: Option<NaiveDateTime>,
    pub primary_at: Option<NaiveDateTime>,
    pub primary_coverage: &'static str,
    pub non_price_items: usize,
    pub source_count: usize,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub kind: &'static str,
    pub instrument_id: String,
    pub title: String,
    pub detail: String,
    pub evidence_id: Option<String>,
    pub thesis_id: Option<String>,
    pub ts: Option<NaiveDateTime>,
}

pub fn primary_sources_for(cfg: &AppConfig, instrument: &Instrument) -> BTreeSet<String> {
    delta_plugins::configured_plugins(cfg)
        .into_iter()
        .filter(|plugin| crate::brief::PRIMARY_FILING_SOURCES.contains(&plugin.name()))
        .filter(|plugin| !plugin.universe(std::slice::from_ref(instrument)).is_empty())
        .map(|plugin| plugin.name().to_string())
        .collect()
}

fn stale_warning(
    warnings: &mut Vec<String>,
    label: &str,
    at: Option<NaiveDateTime>,
    now: NaiveDateTime,
    days: i64,
) {
    match at {
        None => warnings.push(format!("no {label} data")),
        Some(at) if now - at > Duration::days(days) => warnings.push(format!(
            "latest {label} is {} days old",
            (now - at).num_days().max(0)
        )),
        _ => {}
    }
}

pub fn audit_items(
    instrument_id: &str,
    items: &[EvidenceItem],
    primary_sources: &BTreeSet<String>,
    now: NaiveDateTime,
) -> EvidenceAudit {
    let latest = |kind: &str| {
        items
            .iter()
            .filter(|item| item.kind == kind && item.ts <= now)
            .map(|item| item.ts)
            .max()
    };
    let price_at = latest("bar");
    let news_at = latest("news");
    let primary_at = items
        .iter()
        .filter(|item| {
            item.kind == "filing" && item.ts <= now && primary_sources.contains(&item.source)
        })
        .map(|item| item.ts)
        .max();
    let recent = items
        .iter()
        .filter(|item| item.kind != "bar" && item.ts <= now && item.ts >= now - Duration::days(30))
        .collect::<Vec<_>>();
    let primary_coverage = if primary_sources.is_empty() {
        "not_configured"
    } else {
        match primary_at {
            None => "absent",
            Some(at) if now - at > Duration::days(30) => "stale",
            _ => "fresh",
        }
    };
    let mut warnings = Vec::new();
    stale_warning(&mut warnings, "price", price_at, now, 3);
    stale_warning(&mut warnings, "news", news_at, now, 14);
    match primary_coverage {
        "not_configured" => warnings.push("no primary disclosure source configured".into()),
        "absent" => warnings.push("no primary disclosure found".into()),
        "stale" => warnings.push(format!(
            "latest primary disclosure is {} days old",
            (now - primary_at.unwrap()).num_days().max(0)
        )),
        _ => {}
    }
    if recent.len() < 3 {
        warnings.push("thin recent evidence".into());
    }
    let source_count = recent
        .iter()
        .map(|item| &item.source)
        .collect::<BTreeSet<_>>()
        .len();
    if source_count < 2 {
        warnings.push("low source diversity".into());
    }
    EvidenceAudit {
        instrument_id: instrument_id.into(),
        price_at,
        news_at,
        primary_at,
        primary_coverage,
        non_price_items: recent.len(),
        source_count,
        warnings,
    }
}

pub fn evidence_audit(
    db: &Db,
    instrument_id: &str,
    primary_sources: &BTreeSet<String>,
    now: NaiveDateTime,
) -> Result<EvidenceAudit, ServiceError> {
    Ok(audit_items(
        instrument_id,
        &evidence(db, Some(instrument_id), 10_000)?,
        primary_sources,
        now,
    ))
}

pub fn review_queue(
    db: &Db,
    cfg: &AppConfig,
    universe: &[Instrument],
    instrument_ids: &[String],
    since: Option<NaiveDateTime>,
    now: NaiveDateTime,
) -> Result<Vec<ReviewItem>, ServiceError> {
    let since = since.unwrap_or(now - Duration::days(7));
    let ids = if instrument_ids.is_empty() {
        universe
            .iter()
            .map(|inst| inst.id.clone())
            .collect::<Vec<_>>()
    } else {
        instrument_ids.to_vec()
    };
    let theses = list_theses(db)?
        .into_iter()
        .filter(|thesis| thesis.status == "active" && !thesis.falsifiers.is_empty())
        .collect::<Vec<_>>();
    let mut groups: [Vec<ReviewItem>; 4] = Default::default();
    for id in ids {
        let sources = universe
            .iter()
            .find(|inst| inst.id == id)
            .map(|inst| primary_sources_for(cfg, inst))
            .unwrap_or_default();
        let items = evidence(db, Some(&id), 10_000)?;
        for item in items
            .iter()
            .filter(|item| item.ts >= since && item.ts <= now)
        {
            if item.kind == "filing" && sources.contains(&item.source) {
                groups[0].push(prompt("primary_disclosure", &id, item, None));
            }
            let haystack = format!(
                "{} {} {}",
                item.kind,
                item.title,
                item.body.as_deref().unwrap_or("")
            )
            .to_lowercase();
            for thesis in &theses {
                if (thesis.targets.is_empty() || thesis.targets.contains(&id))
                    && thesis
                        .falsifiers
                        .iter()
                        .filter(|term| !term.trim().is_empty())
                        .any(|term| haystack.contains(&term.to_lowercase()))
                {
                    groups[1].push(prompt("falsifier", &id, item, Some(thesis.id.clone())));
                }
            }
        }
        let audit = audit_items(&id, &items, &sources, now);
        for (group, kind, title, thin) in [
            (2, "stale", "Coverage needs review", false),
            (3, "thin_evidence", "Evidence coverage is thin", true),
        ] {
            let warnings = audit
                .warnings
                .iter()
                .filter(|warning| {
                    let is_thin = matches!(
                        warning.as_str(),
                        "thin recent evidence" | "low source diversity"
                    );
                    is_thin == thin
                })
                .cloned()
                .collect::<Vec<_>>();
            if !warnings.is_empty() {
                groups[group].push(ReviewItem {
                    kind,
                    instrument_id: id.clone(),
                    title: title.into(),
                    detail: warnings.join("; "),
                    evidence_id: None,
                    thesis_id: None,
                    ts: None,
                });
            }
        }
    }
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for mut group in groups {
        group.sort_by(|a, b| {
            b.ts.cmp(&a.ts)
                .then_with(|| a.instrument_id.cmp(&b.instrument_id))
                .then_with(|| a.evidence_id.cmp(&b.evidence_id))
                .then_with(|| a.thesis_id.cmp(&b.thesis_id))
        });
        for item in group {
            if seen.insert((
                item.kind,
                item.instrument_id.clone(),
                item.evidence_id.clone(),
                item.thesis_id.clone(),
            )) {
                result.push(item);
            }
        }
    }
    Ok(result)
}

fn prompt(
    kind: &'static str,
    id: &str,
    item: &EvidenceItem,
    thesis_id: Option<String>,
) -> ReviewItem {
    ReviewItem {
        kind,
        instrument_id: id.into(),
        title: item.title.clone(),
        detail: if kind == "primary_disclosure" {
            item.source.clone()
        } else {
            "matches an active thesis falsifier".into()
        },
        evidence_id: Some(item.id.clone()),
        thesis_id,
        ts: Some(item.ts),
    }
}
