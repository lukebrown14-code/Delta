//! Cited research reports (port of `delta/reports.py`).

use std::collections::BTreeSet;
use std::fmt;
use std::path::{Path, PathBuf};

use chrono::{DateTime, SecondsFormat, Utc};
use delta_core::config::{read_env_value_named, AppConfig, ENV_PATH};
use delta_core::db::Db;
use delta_llm::client::LlmClient;
use delta_llm::router::model_for;
use serde::de::{MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::json;

use crate::error::ServiceError;
use crate::evidence::{cite, evidence};

pub const REPORT_TEMPLATE: &str = "report_v2.j2";
pub const PROMPT_VERSION: &str = "report_v2";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claim {
    pub text: String,
    #[serde(default)]
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportDraft {
    pub summary: String,
    #[serde(default)]
    pub bull_summary: String,
    #[serde(default)]
    pub bull: Vec<Claim>,
    #[serde(default)]
    pub bear_summary: String,
    #[serde(default)]
    pub bear: Vec<Claim>,
    #[serde(default)]
    pub risks_summary: String,
    #[serde(default)]
    pub risks: Vec<Claim>,
    #[serde(default)]
    pub catalysts_summary: String,
    #[serde(default)]
    pub catalysts: Vec<Claim>,
    #[serde(default)]
    pub unknowns: Vec<String>,
    #[serde(deserialize_with = "crate::pipeline::sentiment_in_range")]
    pub sentiment: f64,
    #[serde(default)]
    pub sentiment_reasons_summary: String,
    #[serde(default)]
    pub sentiment_reasons: Vec<Claim>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    #[serde(flatten)]
    pub draft: ReportDraft,
    pub target_id: String,
    #[serde(serialize_with = "serialize_as_of")]
    pub as_of: String,
    pub prompt_version: String,
    pub citations: Citations,
}

/// Keep the evidence pool's newest-first order in persisted Python sidecars.
/// A sorted map would reorder citations and break byte parity with Python.
#[derive(Debug, Clone, Default)]
pub struct Citations(Vec<(String, String)>);

impl Citations {
    fn get(&self, id: &str) -> Option<&String> {
        self.0
            .iter()
            .find(|(key, _)| key == id)
            .map(|(_, value)| value)
    }

    fn ids(&self) -> impl Iterator<Item = &String> {
        self.0.iter().map(|(id, _)| id)
    }
}

impl Serialize for Citations {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut map = serializer.serialize_map(Some(self.0.len()))?;
        for (id, value) in &self.0 {
            map.serialize_entry(id, value)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Citations {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct CitationVisitor;

        impl<'de> Visitor<'de> for CitationVisitor {
            type Value = Citations;

            fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str("an evidence id to citation map")
            }

            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Self::Value, M::Error> {
                let mut entries = Vec::with_capacity(map.size_hint().unwrap_or(0));
                while let Some(entry) = map.next_entry()? {
                    entries.push(entry);
                }
                Ok(Citations(entries))
            }
        }

        deserializer.deserialize_map(CitationVisitor)
    }
}

fn serialize_as_of<S: Serializer>(value: &str, serializer: S) -> Result<S::Ok, S::Error> {
    let normalized = DateTime::parse_from_rfc3339(value)
        .map(|ts| {
            ts.with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::AutoSi, true)
        })
        .unwrap_or_else(|_| value.to_string());
    serializer.serialize_str(&normalized)
}

fn supported(claims: &mut Vec<Claim>, gathered: &BTreeSet<String>) {
    claims.retain(|claim| {
        !claim.evidence_ids.is_empty() && claim.evidence_ids.iter().all(|id| gathered.contains(id))
    });
}

fn validate_draft(
    draft: &mut ReportDraft,
    gathered: &BTreeSet<String>,
) -> Result<(), ServiceError> {
    if !(-1.0..=1.0).contains(&draft.sentiment) {
        return Err(ServiceError::invalid(
            "report sentiment must be between -1 and 1",
        ));
    }
    for claims in [
        &mut draft.bull,
        &mut draft.bear,
        &mut draft.risks,
        &mut draft.catalysts,
        &mut draft.sentiment_reasons,
    ] {
        supported(claims, gathered);
    }
    if [
        &draft.bull,
        &draft.bear,
        &draft.risks,
        &draft.catalysts,
        &draft.sentiment_reasons,
    ]
    .iter()
    .all(|claims| claims.is_empty())
    {
        return Err(ServiceError::invalid(
            "report has no claims supported by the gathered evidence",
        ));
    }
    Ok(())
}

/// Ask the configured report model to synthesize only gathered evidence.
pub async fn build_report(
    db: &mut Db,
    client: &LlmClient,
    cfg: &AppConfig,
    target_id: &str,
) -> Result<Report, ServiceError> {
    let items = evidence(db, Some(target_id), None, None, 200, None)?;
    if items.is_empty() {
        return Err(ServiceError::invalid(format!(
            "no evidence gathered for target {target_id:?}; gather evidence first"
        )));
    }
    let model =
        model_for(cfg, "report", None).map_err(|err| ServiceError::invalid(err.to_string()))?;
    let target = cfg.targets.get(target_id);
    let target_name = target
        .and_then(|spec| spec.get("label"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or(target_id);
    let target_kind = target
        .and_then(|spec| spec.get("kind"))
        .and_then(serde_json::Value::as_str)
        .unwrap_or("company");
    let vars = json!({
        "target": {"id": target_id, "name": target_name, "kind": target_kind},
        "items": items.iter().map(|item| json!({
            "id": item.id,
            "cite": cite(item),
            "kind": item.kind,
            "ts": format!("{}+00:00", item.ts.format(if item.ts.and_utc().timestamp_subsec_micros() == 0 { "%Y-%m-%dT%H:%M:%S" } else { "%Y-%m-%dT%H:%M:%S%.6f" })),
            "body": item.body.as_deref().unwrap_or(""),
        })).collect::<Vec<_>>(),
    });
    let schema = crate::schemas::report_draft();
    let (mut draft, _) = delta_llm::structured::structured::<ReportDraft>(
        client,
        db,
        "report",
        &model,
        REPORT_TEMPLATE,
        &vars,
        Some(&schema),
    )
    .await
    .map_err(|err| ServiceError::invalid(err.to_string()))?;
    let citations = Citations(
        items
            .iter()
            .map(|item| (item.id.clone(), cite(item)))
            .collect(),
    );
    let gathered: BTreeSet<String> = citations.ids().cloned().collect();
    validate_draft(&mut draft, &gathered)?;
    Ok(Report {
        draft,
        target_id: target_id.to_string(),
        as_of: Utc::now().to_rfc3339(),
        prompt_version: PROMPT_VERSION.to_string(),
        citations,
    })
}

/// Build and persist a report using the provider and output path in config.
pub async fn generate_report_configured(
    db: &mut Db,
    cfg: &AppConfig,
    target_id: &str,
) -> Result<(Report, PathBuf), ServiceError> {
    let client = delta_llm::client::build_client(
        &cfg.llm_provider,
        &|name| read_env_value_named(name, Path::new(ENV_PATH)),
        std::time::Duration::from_secs(60),
        Some(cfg.llm_max_output_tokens),
        &cfg.llm_base_url,
        &cfg.llm_api_key_env,
    )
    .map_err(|err| ServiceError::invalid(err.to_string()))?;
    let report = build_report(db, &client, cfg, target_id).await?;
    let path = write_report(&report, Path::new(&cfg.reports_dir))?;
    Ok((report, path))
}

fn section_sources(claims: &[Claim]) -> Vec<&str> {
    let mut seen = BTreeSet::new();
    claims
        .iter()
        .flat_map(|claim| &claim.evidence_ids)
        .filter(|id| seen.insert(id.as_str()))
        .map(String::as_str)
        .collect()
}

/// Render the same report structure as Python; interactive links target TUI actions.
pub fn render_markdown(report: &Report, interactive: bool) -> String {
    let as_of = DateTime::parse_from_rfc3339(&report.as_of)
        .map(|ts| ts.format("%Y-%m-%d %H:%M").to_string())
        .unwrap_or_else(|_| report.as_of.clone());
    let mut lines = vec![
        format!("# Report — {}", report.target_id),
        String::new(),
        format!(
            "*as of {as_of} UTC · prompt {} · sentiment {:.2}*",
            report.prompt_version, report.draft.sentiment
        ),
        String::new(),
        "## Summary".to_string(),
        String::new(),
        report.draft.summary.clone(),
        String::new(),
    ];
    for (field, title, summary, claims) in [
        (
            "bull",
            "Bull case",
            &report.draft.bull_summary,
            &report.draft.bull,
        ),
        (
            "bear",
            "Bear case",
            &report.draft.bear_summary,
            &report.draft.bear,
        ),
        (
            "risks",
            "Risks",
            &report.draft.risks_summary,
            &report.draft.risks,
        ),
        (
            "catalysts",
            "Catalysts",
            &report.draft.catalysts_summary,
            &report.draft.catalysts,
        ),
        (
            "sentiment_reasons",
            "Sentiment reasons",
            &report.draft.sentiment_reasons_summary,
            &report.draft.sentiment_reasons,
        ),
    ] {
        lines.extend([format!("## {title}"), String::new()]);
        if claims.is_empty() {
            lines.push("- none supported by the gathered evidence".to_string());
        } else {
            if !summary.is_empty() {
                lines.extend([summary.clone(), String::new()]);
            }
            lines.extend(["### Key points".to_string(), String::new()]);
            for (index, claim) in claims.iter().enumerate() {
                let suffix = if interactive {
                    format!(" [+thesis](thesis:{field}:{index})")
                } else {
                    String::new()
                };
                lines.push(format!("- {}{suffix}", claim.text));
            }
            lines.extend([String::new(), "### Sources".to_string(), String::new()]);
            for (index, evidence_id) in section_sources(claims).iter().enumerate() {
                let citation = report
                    .citations
                    .get(*evidence_id)
                    .map(String::as_str)
                    .unwrap_or(evidence_id);
                if interactive {
                    lines.push(format!(
                        "{}. [Inspect source](evidence:{evidence_id}) — {citation}",
                        index + 1
                    ));
                } else {
                    lines.push(format!("{}. {citation}", index + 1));
                }
            }
        }
        lines.push(String::new());
    }
    lines.extend(["## Unknowns".to_string(), String::new()]);
    if report.draft.unknowns.is_empty() {
        lines.push("- none".to_string());
    } else {
        lines.extend(report.draft.unknowns.iter().map(|item| format!("- {item}")));
    }
    lines.join("\n") + "\n"
}

/// Persist a current report and archive an earlier report from the same day.
pub fn write_report(report: &Report, base_dir: &Path) -> Result<PathBuf, ServiceError> {
    let as_of = DateTime::parse_from_rfc3339(&report.as_of)
        .map_err(|err| ServiceError::invalid(err.to_string()))?;
    let target_dir = base_dir.join(&report.target_id);
    std::fs::create_dir_all(&target_dir).map_err(|err| ServiceError::invalid(err.to_string()))?;
    let path = target_dir.join(format!("{}.md", as_of.format("%Y-%m-%d")));
    let sidecar = path.with_extension("json");
    if path.exists() {
        let history = target_dir.join("history");
        std::fs::create_dir_all(&history).map_err(|err| ServiceError::invalid(err.to_string()))?;
        let prior_time = read_report(&sidecar)
            .and_then(|prior| DateTime::parse_from_rfc3339(&prior.as_of).ok())
            .unwrap_or(as_of);
        let stem = prior_time.format("%Y-%m-%dT%H-%M-%S").to_string();
        let mut archived = history.join(format!("{stem}.md"));
        let mut suffix = 1;
        while archived.exists() {
            archived = history.join(format!("{stem}-{suffix}.md"));
            suffix += 1;
        }
        std::fs::rename(&path, &archived).map_err(|err| ServiceError::invalid(err.to_string()))?;
        if sidecar.exists() {
            std::fs::rename(&sidecar, archived.with_extension("json"))
                .map_err(|err| ServiceError::invalid(err.to_string()))?;
        }
    }
    std::fs::write(&path, render_markdown(report, false))
        .map_err(|err| ServiceError::invalid(err.to_string()))?;
    let json = serde_json::to_string_pretty(report)
        .map_err(|err| ServiceError::invalid(err.to_string()))?;
    std::fs::write(&sidecar, json).map_err(|err| ServiceError::invalid(err.to_string()))?;
    Ok(path)
}

pub fn read_report(sidecar: &Path) -> Option<Report> {
    let raw = std::fs::read_to_string(sidecar).ok()?;
    let mut report: Report = serde_json::from_str(&raw).ok()?;
    let ts = DateTime::parse_from_rfc3339(&report.as_of)
        .map(|ts| ts.with_timezone(&Utc))
        .ok()
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(&report.as_of, "%Y-%m-%dT%H:%M:%S%.f")
                .ok()
                .map(|ts| ts.and_utc())
        })?;
    report.as_of = ts.to_rfc3339();
    Some(report)
}

pub fn report_history(base_dir: &Path, target_id: &str) -> Vec<Report> {
    let dir = base_dir.join(target_id);
    let mut reports = Vec::new();
    for folder in [dir.clone(), dir.join("history")] {
        if let Ok(entries) = std::fs::read_dir(folder) {
            for path in entries.filter_map(|entry| entry.ok().map(|entry| entry.path())) {
                if path.extension().is_some_and(|ext| ext == "json") {
                    if let Some(report) = read_report(&path) {
                        reports.push(report);
                    }
                }
            }
        }
    }
    reports.sort_by(|a, b| {
        DateTime::parse_from_rfc3339(&b.as_of)
            .ok()
            .cmp(&DateTime::parse_from_rfc3339(&a.as_of).ok())
    });
    reports
}
