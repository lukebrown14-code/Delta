//! Canonical Python Pydantic response schemas for provider enforced outputs.
use serde_json::Value;

pub fn event_batch() -> Value {
    serde_json::from_str(
        r###"{
  "type": "json_schema",
  "json_schema": {
    "name": "EventBatch",
    "schema": {
      "$defs": {
        "EventDraft": {
          "properties": {
            "kind": {
              "enum": [
                "earnings",
                "guidance",
                "dividend",
                "insider_trade",
                "m&a",
                "regulatory",
                "macro",
                "other"
              ],
              "title": "Kind",
              "type": "string"
            },
            "summary": {
              "title": "Summary",
              "type": "string"
            },
            "sentiment": {
              "maximum": 1,
              "minimum": -1,
              "title": "Sentiment",
              "type": "number"
            },
            "evidence_ids": {
              "items": {
                "type": "string"
              },
              "title": "Evidence Ids",
              "type": "array"
            }
          },
          "required": [
            "kind",
            "summary",
            "sentiment"
          ],
          "title": "EventDraft",
          "type": "object"
        }
      },
      "properties": {
        "events": {
          "items": {
            "$ref": "#/$defs/EventDraft"
          },
          "title": "Events",
          "type": "array"
        }
      },
      "title": "EventBatch",
      "type": "object"
    }
  }
}"###,
    )
    .expect("canonical schema is valid JSON")
}

pub fn report_draft() -> Value {
    serde_json::from_str(
        r###"{
  "type": "json_schema",
  "json_schema": {
    "name": "ReportDraft",
    "schema": {
      "$defs": {
        "Claim": {
          "description": "The smallest cited unit: one statement plus the evidence supporting it.",
          "properties": {
            "text": {
              "title": "Text",
              "type": "string"
            },
            "evidence_ids": {
              "items": {
                "type": "string"
              },
              "title": "Evidence Ids",
              "type": "array"
            }
          },
          "required": [
            "text"
          ],
          "title": "Claim",
          "type": "object"
        }
      },
      "description": "The structured-call output shape for ``report_v1``.",
      "properties": {
        "summary": {
          "title": "Summary",
          "type": "string"
        },
        "bull_summary": {
          "default": "",
          "title": "Bull Summary",
          "type": "string"
        },
        "bull": {
          "items": {
            "$ref": "#/$defs/Claim"
          },
          "title": "Bull",
          "type": "array"
        },
        "bear_summary": {
          "default": "",
          "title": "Bear Summary",
          "type": "string"
        },
        "bear": {
          "items": {
            "$ref": "#/$defs/Claim"
          },
          "title": "Bear",
          "type": "array"
        },
        "risks_summary": {
          "default": "",
          "title": "Risks Summary",
          "type": "string"
        },
        "risks": {
          "items": {
            "$ref": "#/$defs/Claim"
          },
          "title": "Risks",
          "type": "array"
        },
        "catalysts_summary": {
          "default": "",
          "title": "Catalysts Summary",
          "type": "string"
        },
        "catalysts": {
          "items": {
            "$ref": "#/$defs/Claim"
          },
          "title": "Catalysts",
          "type": "array"
        },
        "unknowns": {
          "items": {
            "type": "string"
          },
          "title": "Unknowns",
          "type": "array"
        },
        "sentiment": {
          "maximum": 1,
          "minimum": -1,
          "title": "Sentiment",
          "type": "number"
        },
        "sentiment_reasons_summary": {
          "default": "",
          "title": "Sentiment Reasons Summary",
          "type": "string"
        },
        "sentiment_reasons": {
          "items": {
            "$ref": "#/$defs/Claim"
          },
          "title": "Sentiment Reasons",
          "type": "array"
        }
      },
      "required": [
        "summary",
        "sentiment"
      ],
      "title": "ReportDraft",
      "type": "object"
    }
  }
}"###,
    )
    .expect("canonical schema is valid JSON")
}

pub fn thesis_draft() -> Value {
    serde_json::from_str(
        r###"{
  "type": "json_schema",
  "json_schema": {
    "name": "ThesisDraft",
    "schema": {
      "$defs": {
        "CandidateDraft": {
          "description": "One (evidence_id, side, note) triple proposed by the thesis model.",
          "properties": {
            "evidence_id": {
              "title": "Evidence Id",
              "type": "string"
            },
            "side": {
              "enum": [
                "support",
                "against",
                "neutral"
              ],
              "title": "Side",
              "type": "string"
            },
            "note": {
              "title": "Note",
              "type": "string"
            }
          },
          "required": [
            "evidence_id",
            "side",
            "note"
          ],
          "title": "CandidateDraft",
          "type": "object"
        }
      },
      "description": "The structured-call payload: candidate triples for a batch of evidence.",
      "properties": {
        "candidates": {
          "items": {
            "$ref": "#/$defs/CandidateDraft"
          },
          "title": "Candidates",
          "type": "array"
        }
      },
      "title": "ThesisDraft",
      "type": "object"
    }
  }
}"###,
    )
    .expect("canonical schema is valid JSON")
}

pub fn summary_draft() -> Value {
    serde_json::from_str(
        r###"{
  "type": "json_schema",
  "json_schema": {
    "name": "SummaryDraft",
    "schema": {
      "properties": {
        "summary": {
          "title": "Summary",
          "type": "string"
        },
        "strongest_support": {
          "default": "",
          "title": "Strongest Support",
          "type": "string"
        },
        "strongest_counter": {
          "default": "",
          "title": "Strongest Counter",
          "type": "string"
        },
        "unknowns": {
          "items": {
            "type": "string"
          },
          "title": "Unknowns",
          "type": "array"
        },
        "citations": {
          "items": {
            "type": "string"
          },
          "title": "Citations",
          "type": "array"
        }
      },
      "required": [
        "summary"
      ],
      "title": "SummaryDraft",
      "type": "object"
    }
  }
}"###,
    )
    .expect("canonical schema is valid JSON")
}
