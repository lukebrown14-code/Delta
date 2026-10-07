//! Port of `tests/test_decisions.py` (the append-only decision journal).

use chrono::{NaiveDate, NaiveDateTime};

use delta_core::db::Db;
use delta_services::decisions::{
    append_review, create_decision, delete_decision, due_reviews, get_decision, list_decisions,
    relink_thesis, review_history, update_decision, Decision,
};
use delta_services::error::ServiceError;
use delta_services::theses::{create_thesis, update_thesis};

const INST: &str = "US:AAPL";
const OTHER: &str = "US:MSFT";
const NOW: &str = "2026-03-22 12:00:00";

fn ts(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn date(text: &str) -> NaiveDate {
    NaiveDate::parse_from_str(text, "%Y-%m-%d").unwrap()
}

fn db() -> Db {
    Db::open_memory().unwrap()
}

#[track_caller]
fn assert_invalid<T>(result: Result<T, ServiceError>, needle: &str) {
    match result {
        Ok(_) => panic!("expected error containing {needle:?}"),
        Err(ServiceError::Invalid { message }) => assert!(
            message.contains(needle),
            "message {message:?} does not contain {needle:?}"
        ),
        Err(other) => panic!("unexpected error kind: {other}"),
    }
}

/// `_create`: the fixed original framing, `created_at` pinned to NOW.
fn create(db: &Db) -> Decision {
    create_decision(
        db,
        INST,
        "Services revenue is growing.",
        "Trading at 20x forward earnings.",
        "5 years",
        date("2026-04-01"),
        "Services revenue growth turns negative.",
        None,
        Some(ts(NOW)),
    )
    .unwrap()
}

#[test]
fn create_list_and_get_round_trip() {
    let db = db();
    let created = create(&db);

    assert_eq!(created.status, "open");
    assert_eq!(created.created_at, ts(NOW));
    assert_eq!(get_decision(&db, &created.id).unwrap().id, created.id);
    assert_eq!(list_decisions(&db, None, true).unwrap().len(), 1);
    // The stored framing matches field for field.
    let stored = get_decision(&db, &created.id).unwrap();
    assert_eq!(stored.rationale, created.rationale);
    assert_eq!(stored.valuation_context, created.valuation_context);
    assert_eq!(stored.time_horizon, created.time_horizon);
    assert_eq!(stored.review_date, created.review_date);
    assert_eq!(stored.invalidation_criteria, created.invalidation_criteria);
    assert_invalid(get_decision(&db, "nope"), "unknown decision");
    // The generated id is uuid4().hex-shaped: 32 lowercase hex chars.
    assert_eq!(created.id.len(), 32);
    assert!(created
        .id
        .chars()
        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
}

#[test]
fn original_framing_fields_are_required() {
    let cases: Vec<(&str, String, String)> = vec![
        ("instrument_id", "instrument_id".into(), " ".into()),
        ("rationale", "rationale".into(), " ".into()),
        ("valuation_context", "valuation_context".into(), " ".into()),
        ("time_horizon", "time_horizon".into(), " ".into()),
        (
            "invalidation_criteria",
            "invalidation_criteria".into(),
            " ".into(),
        ),
    ];
    for (label, field, value) in cases {
        let db = db();
        let result = match field.as_str() {
            "instrument_id" => create_decision(
                &db,
                &value,
                "r",
                "v",
                "t",
                date("2026-04-01"),
                "i",
                None,
                Some(ts(NOW)),
            ),
            "rationale" => create_decision(
                &db,
                INST,
                &value,
                "v",
                "t",
                date("2026-04-01"),
                "i",
                None,
                Some(ts(NOW)),
            ),
            "valuation_context" => create_decision(
                &db,
                INST,
                "r",
                &value,
                "t",
                date("2026-04-01"),
                "i",
                None,
                Some(ts(NOW)),
            ),
            "time_horizon" => create_decision(
                &db,
                INST,
                "r",
                "v",
                &value,
                date("2026-04-01"),
                "i",
                None,
                Some(ts(NOW)),
            ),
            _ => create_decision(
                &db,
                INST,
                "r",
                "v",
                "t",
                date("2026-04-01"),
                &value,
                None,
                Some(ts(NOW)),
            ),
        };
        assert_invalid(result, label);
    }
}

#[test]
fn reviews_append_without_changing_original_framing() {
    let db = db();
    let created = create(&db);
    let later = append_review(
        &db,
        &created.id,
        "Revenue remains on plan.",
        Some("reviewed"),
        Some(ts("2026-03-23 12:00:00")),
    )
    .unwrap();
    let earlier = append_review(
        &db,
        &created.id,
        "Initial check.",
        None,
        Some(ts("2026-03-22 13:00:00")),
    )
    .unwrap();

    let stored = get_decision(&db, &created.id).unwrap();
    assert_eq!(stored.rationale, created.rationale);
    assert_eq!(stored.valuation_context, created.valuation_context);
    assert_eq!(stored.invalidation_criteria, created.invalidation_criteria);
    assert_eq!(stored.status, "reviewed");
    assert_eq!(
        review_history(&db, &created.id)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        [earlier.id, later.id]
    );
    assert_eq!(later.status.as_deref(), Some("reviewed"));
}

#[test]
fn due_reviews_order_and_retirement() {
    let db = db();
    let late = create_decision(
        &db,
        INST,
        "r",
        "v",
        "t",
        date("2026-03-20"),
        "i",
        None,
        Some(ts(NOW)),
    )
    .unwrap();
    let early = create_decision(
        &db,
        INST,
        "r",
        "v",
        "t",
        date("2026-03-19"),
        "i",
        None,
        Some(ts("2026-03-22 12:00:01")),
    )
    .unwrap();
    let future = create_decision(
        &db,
        INST,
        "r",
        "v",
        "t",
        date("2026-03-23"),
        "i",
        None,
        Some(ts("2026-03-22 12:00:02")),
    )
    .unwrap();
    append_review(
        &db,
        &late.id,
        "No longer applicable.",
        Some("retired"),
        None,
    )
    .unwrap();

    let due = due_reviews(&db, Some(date("2026-03-22"))).unwrap();
    assert_eq!(
        due.iter().map(|d| d.id.as_str()).collect::<Vec<_>>(),
        [early.id.as_str()]
    );
    assert!(!due.iter().any(|d| d.id == future.id));
    assert!(!due.iter().any(|d| d.id == late.id));
}

#[test]
fn thesis_link_is_compatible_and_snapshotted() {
    let db = db();
    let thesis = create_thesis(
        &db,
        "Apple services keep growing.",
        "",
        &[],
        &[],
        &[INST.to_string()],
        "",
        None,
    )
    .unwrap();
    let created = create_decision(
        &db,
        INST,
        "r",
        "v",
        "t",
        date("2026-04-01"),
        "i",
        Some(&thesis.id),
        Some(ts(NOW)),
    )
    .unwrap();

    assert_eq!(created.thesis_id.as_deref(), Some(thesis.id.as_str()));
    assert_eq!(
        created.thesis_claim_snapshot.as_deref(),
        Some(thesis.claim.as_str())
    );

    let incompatible = create_thesis(
        &db,
        "Microsoft grows.",
        "",
        &[],
        &[],
        &[OTHER.to_string()],
        "",
        None,
    )
    .unwrap();
    assert_invalid(
        create_decision(
            &db,
            INST,
            "r",
            "v",
            "t",
            date("2026-04-01"),
            "i",
            Some(&incompatible.id),
            Some(ts(NOW)),
        ),
        "does not target",
    );
}

#[test]
fn relink_thesis_preserves_historical_snapshot() {
    let db = db();
    let thesis = create_thesis(
        &db,
        "Apple services keep growing.",
        "",
        &[],
        &[],
        &[INST.to_string()],
        "",
        None,
    )
    .unwrap();
    let created = create_decision(
        &db,
        INST,
        "r",
        "v",
        "t",
        date("2026-04-01"),
        "i",
        Some(&thesis.id),
        Some(ts(NOW)),
    )
    .unwrap();
    let new_id = "renamed-thesis";

    assert_eq!(
        relink_thesis(&db, &thesis.id, new_id, "New claim").unwrap(),
        1
    );
    let stored = get_decision(&db, &created.id).unwrap();
    assert_eq!(stored.thesis_id.as_deref(), Some(new_id));
    assert_eq!(
        stored.thesis_claim_snapshot.as_deref(),
        Some(thesis.claim.as_str())
    );
}

#[test]
fn thesis_rename_relinks_decision_but_preserves_snapshot() {
    let db = db();
    let thesis = create_thesis(
        &db,
        "Apple services keep growing.",
        "",
        &[],
        &[],
        &[INST.to_string()],
        "",
        None,
    )
    .unwrap();
    let created = create_decision(
        &db,
        INST,
        "r",
        "v",
        "t",
        date("2026-04-01"),
        "i",
        Some(&thesis.id),
        Some(ts(NOW)),
    )
    .unwrap();

    let renamed = update_thesis(
        &db,
        &thesis.id,
        "Apple services keep compounding.",
        &[INST.to_string()],
        "5 years",
        "active",
        None,
        None,
        None,
    )
    .unwrap();

    let stored = get_decision(&db, &created.id).unwrap();
    assert_eq!(stored.thesis_id.as_deref(), Some(renamed.id.as_str()));
    assert_eq!(
        stored.thesis_claim_snapshot.as_deref(),
        Some("Apple services keep growing.")
    );
}

#[test]
fn public_apis_create_tables_idempotently() {
    let db = db();
    assert!(list_decisions(&db, None, true).unwrap().is_empty());
    let created = create(&db);
    assert!(review_history(&db, &created.id).unwrap().is_empty());
    let due = due_reviews(&db, Some(date("2026-04-01"))).unwrap();
    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, created.id);
}

#[test]
fn update_decision_edits_framing_and_keeps_identity() {
    let db = db();
    let created = create(&db);
    let updated = update_decision(
        &db,
        &created.id,
        OTHER,
        "Revised rationale.",
        "25x forward earnings.",
        "3 years",
        date("2026-06-01"),
        "Growth stalls.",
        None,
    )
    .unwrap();

    assert_eq!(updated.id, created.id);
    assert_eq!(updated.created_at, created.created_at);
    assert_eq!(updated.instrument_id, OTHER);
    assert_eq!(updated.rationale, "Revised rationale.");
    assert_eq!(
        get_decision(&db, &created.id).unwrap().rationale,
        "Revised rationale."
    );
    assert_invalid(
        update_decision(
            &db,
            "nope",
            INST,
            "x",
            "y",
            "z",
            date("2026-04-01"),
            "w",
            None,
        ),
        "unknown decision",
    );
}

#[test]
fn delete_decision_removes_decision_and_reviews() {
    let db = db();
    let created = create(&db);
    append_review(&db, &created.id, "A note.", None, None).unwrap();
    delete_decision(&db, &created.id).unwrap();

    assert!(list_decisions(&db, None, true).unwrap().is_empty());
    assert_invalid(get_decision(&db, &created.id), "unknown decision");
    assert_invalid(delete_decision(&db, "nope"), "unknown decision");
}

#[test]
fn append_review_validates_status_and_note() {
    let db = db();
    let created = create(&db);
    assert_invalid(
        append_review(&db, &created.id, "note", Some("nonsense"), None),
        "status must be one of",
    );
    assert_invalid(
        append_review(&db, &created.id, " ", None, None),
        "note is required",
    );
    assert_invalid(
        append_review(&db, "nope", "note", None, None),
        "unknown decision",
    );
}
