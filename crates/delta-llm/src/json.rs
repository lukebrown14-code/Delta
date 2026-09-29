//! One JSON fence parser for every LLM output path. Port of `delta/llm/json.py`.

/// `serde_json::from_str` after stripping a markdown code fence; errors on failure.
///
/// Returns the parsed value, or an error when the text is not valid JSON after
/// fence removal. Callers that must degrade gracefully catch the error.
pub fn extract_json(text: &str) -> Result<serde_json::Value, serde_json::Error> {
    let mut stripped = text.trim();
    if stripped.starts_with("```") {
        // Python: split("```", 2)[1] — the body between the first two fences.
        let mut parts = stripped.splitn(3, "```");
        let _ = parts.next();
        let body = parts.next().unwrap_or("");
        let body = body.strip_prefix("json").unwrap_or(body);
        stripped = body.trim();
    }
    serde_json::from_str(stripped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn plain_json() {
        assert_eq!(extract_json("{\"a\": 1}").unwrap(), json!({"a": 1}));
    }

    #[test]
    fn fenced_json() {
        assert_eq!(
            extract_json("```json\n{\"a\": 1}\n```").unwrap(),
            json!({"a": 1})
        );
        assert_eq!(extract_json("```\n[1, 2]\n```").unwrap(), json!([1, 2]));
    }

    #[test]
    fn invalid_raises() {
        assert!(extract_json("not json").is_err());
        assert!(extract_json("```json\nnope\n```").is_err());
    }
}
