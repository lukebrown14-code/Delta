//! Offline citation-validity harness. Port of `delta/llm/eval.py`.
//!
//! The provenance contract lives here: citations must reference gathered
//! evidence IDs only. Ported first (per the rewrite plan) and pinned by tests.

/// The citations present in `valid_ids`, de-duplicated, order preserved.
pub fn valid_citations<'a>(
    citations: impl IntoIterator<Item = &'a str>,
    valid_ids: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut kept = Vec::new();
    for citation in citations {
        if valid_ids.contains(citation) && seen.insert(citation.to_string()) {
            kept.push(citation.to_string());
        }
    }
    kept
}

/// The citations not backed by `valid_ids`, de-duplicated, order preserved.
pub fn hallucinated_citations<'a>(
    citations: impl IntoIterator<Item = &'a str>,
    valid_ids: &std::collections::BTreeSet<String>,
) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    let mut dropped = Vec::new();
    for citation in citations {
        if !valid_ids.contains(citation) && seen.insert(citation.to_string()) {
            dropped.push(citation.to_string());
        }
    }
    dropped
}

/// Fraction of produced citations backed by `valid_ids`; 1.0 when none.
///
/// A run with no citations has nothing invalid to flag, so it rates a perfect
/// 1.0 rather than dividing by zero.
pub fn citation_validity_rate<'a>(
    citations: impl IntoIterator<Item = &'a str>,
    valid_ids: &std::collections::BTreeSet<String>,
) -> f64 {
    let produced: Vec<&str> = citations.into_iter().collect();
    if produced.is_empty() {
        return 1.0;
    }
    f64::from(valid_citations(produced.iter().copied(), valid_ids).len() as u32)
        / f64::from(produced.len() as u32)
}

/// One golden scenario: the name, the evidence pool, and the pass rule.
#[derive(Debug, Clone)]
pub struct GoldenCase {
    pub name: String,
    pub evidence_ids: std::collections::BTreeSet<String>,
    /// Minimum citation-validity rate the case must reach to count as passing.
    pub min_validity: f64,
}

/// What one golden run produced and how it scored.
#[derive(Debug, Clone)]
pub struct EvalResult {
    pub case: String,
    pub min_validity: f64,
    pub citations: Vec<String>,
    pub valid: Vec<String>,
    pub hallucinated: Vec<String>,
}

impl EvalResult {
    pub fn rate(&self) -> f64 {
        if self.citations.is_empty() {
            return 1.0;
        }
        f64::from(self.valid.len() as u32) / f64::from(self.citations.len() as u32)
    }

    pub fn passed(&self) -> bool {
        self.rate() >= self.min_validity
    }

    /// Fail the run when the citation-validity rate is below the threshold.
    pub fn assert_valid(&self) -> Result<(), String> {
        if self.passed() {
            Ok(())
        } else {
            Err(format!(
                "eval case {:?}: citation validity {:.2}% below threshold {:.2}%; hallucinated {:?}",
                self.case,
                self.rate() * 100.0,
                self.min_validity,
                self.hallucinated
            ))
        }
    }
}

/// Score one golden run, keeping the model's citations and the split.
pub fn evaluate(case: &GoldenCase, citations: &[&str]) -> EvalResult {
    EvalResult {
        case: case.name.clone(),
        min_validity: case.min_validity,
        citations: citations.iter().map(|s| s.to_string()).collect(),
        valid: valid_citations(citations.iter().copied(), &case.evidence_ids),
        hallucinated: hallucinated_citations(citations.iter().copied(), &case.evidence_ids),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    fn golden(ids: &[&str]) -> BTreeSet<String> {
        ids.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn valid_and_hallucinated_split_correctly() {
        let ids = golden(&["bar:1", "bar:2", "news:news-1"]);
        let c = ["bar:1", "bogus:9", "bar:2", "bar:2"];
        assert_eq!(valid_citations(c, &ids), vec!["bar:1", "bar:2"]);
        let c = ["bar:1", "bogus:9", "ghost:1", "bogus:9"];
        assert_eq!(hallucinated_citations(c, &ids), vec!["bogus:9", "ghost:1"]);
    }

    #[test]
    fn rate_ranges_and_empty_is_perfect() {
        let ids = golden(&["bar:1", "bar:2", "news:news-1"]);
        assert_eq!(citation_validity_rate(["bar:1", "bar:2"], &ids), 1.0);
        assert_eq!(citation_validity_rate(["bar:1", "bogus:9"], &ids), 0.5);
        assert_eq!(citation_validity_rate([], &ids), 1.0);
    }

    #[test]
    fn evaluate_flags_hallucinations() {
        let case = GoldenCase {
            name: "sample".to_string(),
            evidence_ids: golden(&["bar:1", "bar:2", "news:news-1"]),
            min_validity: 1.0,
        };
        let result = evaluate(&case, &["bar:1", "bogus:9"]);
        assert_eq!(result.rate(), 0.5);
        assert_eq!(result.hallucinated, vec!["bogus:9".to_string()]);
        assert!(!result.passed());
    }

    #[test]
    fn assert_valid_threshold() {
        let case = GoldenCase {
            name: "strict".to_string(),
            evidence_ids: golden(&["bar:1", "bar:2", "news:news-1"]),
            min_validity: 1.0,
        };
        assert!(evaluate(&case, &["bar:1", "bar:2"]).assert_valid().is_ok());
        let err = evaluate(&case, &["bar:1", "bogus:9"])
            .assert_valid()
            .unwrap_err();
        assert!(err.contains("50.00%"), "{err}");
    }
}
