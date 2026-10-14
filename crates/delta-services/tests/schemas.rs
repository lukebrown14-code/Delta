//! The canonical Python Pydantic response schemas (delta/extract.py
//! `EventBatch`, delta/reports.py `ReportDraft`, delta/theses.py
//! `ThesisDraft`, delta/thesis_summary.py `SummaryDraft`) as
//! provider-enforced `response_format` payloads. Field expectations are
//! taken from the Python models, not from the Rust JSON.

use delta_services::schemas;
use serde_json::json;

#[test]
fn event_batch_matches_the_python_model() {
    let schema = schemas::event_batch();
    assert_eq!(schema["type"], "json_schema");
    assert_eq!(schema["json_schema"]["name"], "EventBatch");
    let model = &schema["json_schema"]["schema"];
    assert_eq!(model["type"], "object");
    // `events` defaults to empty, so pydantic omits `required` entirely.
    assert!(model.get("required").is_none());
    let draft = &model["$defs"]["EventDraft"];
    assert_eq!(draft["required"], json!(["kind", "summary", "sentiment"]));
    assert_eq!(draft["properties"]["sentiment"]["minimum"], -1);
    assert_eq!(draft["properties"]["sentiment"]["maximum"], 1);
    assert_eq!(
        draft["properties"]["kind"]["enum"],
        json!([
            "earnings",
            "guidance",
            "dividend",
            "insider_trade",
            "m&a",
            "regulatory",
            "macro",
            "other"
        ])
    );
    assert_eq!(
        model["properties"]["events"]["items"]["$ref"],
        "#/$defs/EventDraft"
    );
}

#[test]
fn report_draft_matches_the_python_model() {
    let schema = schemas::report_draft();
    assert_eq!(schema["type"], "json_schema");
    assert_eq!(schema["json_schema"]["name"], "ReportDraft");
    let model = &schema["json_schema"]["schema"];
    assert_eq!(model["required"], json!(["summary", "sentiment"]));
    assert_eq!(model["properties"]["sentiment"]["minimum"], -1);
    assert_eq!(model["properties"]["sentiment"]["maximum"], 1);
    let claim = &model["$defs"]["Claim"];
    assert_eq!(claim["required"], json!(["text"]));
    for section in ["bull", "bear", "risks", "catalysts", "sentiment_reasons"] {
        assert_eq!(
            model["properties"][section]["items"]["$ref"], "#/$defs/Claim",
            "section {section} must cite Claim"
        );
    }
    for summary in [
        "bull_summary",
        "bear_summary",
        "risks_summary",
        "catalysts_summary",
    ] {
        assert_eq!(model["properties"][summary]["default"], "");
    }
}

#[test]
fn thesis_draft_matches_the_python_model() {
    let schema = schemas::thesis_draft();
    assert_eq!(schema["type"], "json_schema");
    assert_eq!(schema["json_schema"]["name"], "ThesisDraft");
    let model = &schema["json_schema"]["schema"];
    let candidate = &model["$defs"]["CandidateDraft"];
    assert_eq!(
        candidate["required"],
        json!(["evidence_id", "side", "note"])
    );
    assert_eq!(
        candidate["properties"]["side"]["enum"],
        json!(["support", "against", "neutral"])
    );
    assert_eq!(
        model["properties"]["candidates"]["items"]["$ref"],
        "#/$defs/CandidateDraft"
    );
}

#[test]
fn summary_draft_matches_the_python_model() {
    let schema = schemas::summary_draft();
    assert_eq!(schema["type"], "json_schema");
    assert_eq!(schema["json_schema"]["name"], "SummaryDraft");
    let model = &schema["json_schema"]["schema"];
    assert_eq!(model["required"], json!(["summary"]));
    assert_eq!(model["properties"]["strongest_support"]["default"], "");
    assert_eq!(model["properties"]["strongest_counter"]["default"], "");
    assert_eq!(model["properties"]["unknowns"]["type"], "array");
    assert_eq!(model["properties"]["citations"]["type"], "array");
}
