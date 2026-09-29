//! Core JSON helpers for list-valued columns. Port of `delta/core/json.py`.

use serde_json::Value;

/// Serialise like Python `json.dumps(list)`: comma-space separators,
/// UTF-8, no trailing newline.
pub fn to_json(value: &[String]) -> String {
    // Python json.dumps default separators are ", " / ": "; serde_json
    // emits no spaces, so render list items explicitly.
    let items: Vec<String> = value
        .iter()
        .map(|s| serde_json::to_string(s).unwrap())
        .collect();
    format!["[{}]", items.join(", ")]
}

/// Parse a JSON list column; `None`, invalid JSON and non-lists all give `[]`,
/// and elements are stringified like Python `str(x)`.
pub fn from_json(value: Option<&str>) -> Vec<String> {
    let Some(raw) = value.filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    match serde_json::from_str::<Value>(raw) {
        Ok(Value::Array(items)) => items
            .into_iter()
            .map(|v| match v {
                Value::String(s) => s,
                Value::Null => "None".to_string(),
                Value::Bool(b) => b.to_string(),
                other => other.to_string(),
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let items = vec!["US:AAPL".to_string(), "ASX:BHP".to_string()];
        assert_eq!(to_json(&items), "[\"US:AAPL\", \"ASX:BHP\"]");
        assert_eq!(from_json(Some(&to_json(&items))), items);
    }

    #[test]
    fn lenient_parsing() {
        assert!(from_json(None).is_empty());
        assert!(from_json(Some("")).is_empty());
        assert!(from_json(Some("not json")).is_empty());
        assert_eq!(from_json(Some("{\"a\": 1}")), Vec::<String>::new());
        assert_eq!(
            from_json(Some("[1, \"x\"]")),
            vec!["1".to_string(), "x".to_string()]
        );
    }
}
