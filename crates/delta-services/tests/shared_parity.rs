//! R3.1b parity: the Rust shared services read the same values the Python
//! implementations produce on `fixtures/golden_seed.db`.
//!
//! The expected values are dumped by `tests/dump_shared_expected.py` (run it
//! after reseeding) into `tests/fixtures/shared_services_expected.json`; this
//! test asserts the Rust ports against them. The round-trip test additionally
//! writes theses/decisions through the Rust APIs and proves the stored bytes
//! are Python-readable encodings (JSON round-trip); the reverse direction
//! (Python reads a Rust-written file through the Python models) is the
//! dumper's `--verify-written <db>` mode.

use std::path::PathBuf;

use chrono::{NaiveDate, NaiveDateTime};
use delta_core::db::Db;
use serde_json::Value;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn expected() -> Value {
    let path = repo_root().join("tests/fixtures/shared_services_expected.json");
    serde_json::from_str(
        &std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display())),
    )
    .expect("parse shared_services_expected.json")
}

fn seed_db() -> Db {
    let path = repo_root().join("fixtures/golden_seed.db");
    assert!(path.exists(), "seed DB missing: {}", path.display());
    Db::open(&path).expect("open golden seed (write-free)")
}

fn ts(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").unwrap()
}

fn json_str(value: &Value) -> String {
    value.as_str().expect("string").to_string()
}

fn json_opt_str(value: &Value) -> Option<String> {
    value.as_str().map(str::to_string)
}

fn item_id(value: &Value) -> String {
    json_str(&value["id"])
}

/// Assert the full evidence pool matches the Python dump, item for item.
#[test]
fn evidence_pool_matches_python() {
    let db = seed_db();
    let exp = expected();
    let items = delta_services::evidence::evidence(&db, None, None, None, 10_000, None).unwrap();
    let dumped = exp["evidence_all"].as_array().unwrap();
    assert_eq!(items.len(), dumped.len(), "pool size");
    for (item, want) in items.iter().zip(dumped) {
        assert_eq!(item.id, item_id(want), "order");
        let target_ids: Vec<String> = want["target_ids"]
            .as_array()
            .unwrap()
            .iter()
            .map(json_str)
            .collect();
        assert_eq!(item.target_ids, target_ids, "{}", item.id);
        assert_eq!(item.ts, ts(&json_str(&want["ts"])), "{}", item.id);
        assert_eq!(item.kind, json_str(&want["kind"]), "{}", item.id);
        assert_eq!(item.title, json_str(&want["title"]), "{}", item.id);
        assert_eq!(
            item.body.as_deref(),
            json_opt_str(&want["body"]).as_deref(),
            "{}",
            item.id
        );
        assert_eq!(item.source, json_str(&want["source"]), "{}", item.id);
        assert_eq!(
            item.url.as_deref(),
            json_opt_str(&want["url"]).as_deref(),
            "{}",
            item.id
        );
        assert_eq!(item.sentiment, want["sentiment"].as_f64(), "{}", item.id);
        assert_eq!(item.quality, json_str(&want["quality"]), "{}", item.id);
        assert_eq!(
            delta_services::evidence::cite(item),
            json_str(&want["cite"]),
            "{}",
            item.id
        );
    }
}

/// Every filtered read's id list matches the Python dump.
#[test]
fn evidence_filters_match_python() {
    let db = seed_db();
    let exp = expected();
    let run = |items: Vec<delta_services::EvidenceItem>| -> Vec<String> {
        items.iter().map(|i| i.id.clone()).collect()
    };
    let exp_ids =
        |key: &str| -> Vec<String> { exp[key].as_array().unwrap().iter().map(json_str).collect() };
    assert_eq!(
        run(
            delta_services::evidence::evidence(&db, Some("US:AAPL"), None, None, 10_000, None)
                .unwrap()
        ),
        exp_ids("evidence_aapl")
    );
    assert_eq!(
        run(
            delta_services::evidence::evidence(&db, Some("US:MSFT"), None, None, 10_000, None)
                .unwrap()
        ),
        exp_ids("evidence_msft")
    );
    assert_eq!(
        run(
            delta_services::evidence::evidence(&db, None, Some("2026-09-14"), None, 10_000, None)
                .unwrap()
        ),
        exp_ids("evidence_since")
    );
    for kind in ["bar", "news", "filing", "event", "fundamental", "web"] {
        assert_eq!(
            run(
                delta_services::evidence::evidence(&db, None, None, Some(kind), 10_000, None)
                    .unwrap()
            ),
            exp_ids(&format!("evidence_kind_{kind}")),
            "kind {kind}"
        );
    }
    assert_eq!(
        run(
            delta_services::evidence::evidence(&db, None, None, None, 10_000, Some("services"))
                .unwrap()
        ),
        exp_ids("evidence_search_services")
    );
    assert_eq!(
        run(
            delta_services::evidence::evidence(&db, None, None, None, 10_000, Some("CLOUD"))
                .unwrap()
        ),
        exp_ids("evidence_search_upper")
    );
    assert_eq!(
        run(delta_services::evidence::evidence(&db, None, None, None, 3, None).unwrap()),
        exp_ids("evidence_limit_3")
    );
    assert_eq!(
        run(delta_services::evidence::evidence(&db, None, None, None, 0, None).unwrap()),
        exp_ids("evidence_limit_0")
    );
    assert_eq!(
        run(delta_services::evidence::evidence(
            &db,
            Some("US:AAPL"),
            Some("2026-09-15"),
            Some("news"),
            10_000,
            None
        )
        .unwrap()),
        exp_ids("evidence_combined")
    );
    assert_eq!(
        run(delta_services::evidence::evidence_by_ids(
            &db,
            &[
                "news:news-aapl-10q".to_string(),
                "event:event-aapl-earnings".to_string(),
                "fundamental:1".to_string(),
                "bar:1".to_string(),
                "news:nope".to_string(),
                "no-prefix".to_string(),
                "filing:news-aapl-chip".to_string(),
            ]
        )
        .unwrap()),
        exp_ids("evidence_by_ids")
    );
}

#[test]
fn theses_match_python() {
    let db = seed_db();
    let exp = expected();
    let theses = delta_services::theses::list_theses(&db).unwrap();
    let dumped = exp["theses"].as_array().unwrap();
    assert_eq!(theses.len(), dumped.len());
    for (thesis, want) in theses.iter().zip(dumped) {
        assert_eq!(thesis.id, json_str(&want["id"]));
        assert_eq!(thesis.claim, json_str(&want["claim"]));
        assert_eq!(thesis.scope, json_str(&want["scope"]));
        let strings = |key: &str| -> Vec<String> {
            want[key].as_array().unwrap().iter().map(json_str).collect()
        };
        assert_eq!(thesis.assumptions, strings("assumptions"));
        assert_eq!(thesis.falsifiers, strings("falsifiers"));
        assert_eq!(thesis.targets, strings("targets"));
        assert_eq!(thesis.time_horizon, json_str(&want["time_horizon"]));
        assert_eq!(thesis.created_at, ts(&json_str(&want["created_at"])));
        assert_eq!(thesis.status, json_str(&want["status"]));

        let links = delta_services::theses::evidence_for(&db, &thesis.id, false).unwrap();
        let dumped_links = &exp["thesis_evidence"][&thesis.id];
        assert_eq!(links.len(), dumped_links.as_array().unwrap().len());
        for (link, want_link) in links.iter().zip(dumped_links.as_array().unwrap()) {
            assert_eq!(link.thesis_id, json_str(&want_link["thesis_id"]));
            assert_eq!(link.evidence_id, json_str(&want_link["evidence_id"]));
            assert_eq!(link.side, json_str(&want_link["side"]));
            assert_eq!(link.note, json_str(&want_link["note"]));
            assert_eq!(link.accepted, want_link["accepted"].as_bool().unwrap());
        }

        // Accepted evidence resolves to the same items with the same sides.
        let accepted = delta_services::theses::accepted_items(&db, &thesis.id, 10_000).unwrap();
        let dumped_accepted = &exp["accepted_items"][&thesis.id];
        assert_eq!(accepted.len(), dumped_accepted.as_array().unwrap().len());
        for ((item, link), want_pair) in accepted.iter().zip(dumped_accepted.as_array().unwrap()) {
            assert_eq!(item.id, item_id(&want_pair["item"]));
            assert_eq!(link.side, json_str(&want_pair["side"]));
            assert_eq!(link.note, json_str(&want_pair["link"]));
        }
    }
}

#[test]
fn decisions_match_python() {
    let db = seed_db();
    let exp = expected();
    let decisions = delta_services::decisions::list_decisions(&db, None, true).unwrap();
    let dumped = exp["decisions"].as_array().unwrap();
    assert_eq!(decisions.len(), dumped.len());
    for (decision, want) in decisions.iter().zip(dumped) {
        assert_eq!(decision.id, json_str(&want["id"]));
        assert_eq!(decision.instrument_id, json_str(&want["instrument_id"]));
        assert_eq!(decision.rationale, json_str(&want["rationale"]));
        assert_eq!(
            decision.valuation_context,
            json_str(&want["valuation_context"])
        );
        assert_eq!(decision.time_horizon, json_str(&want["time_horizon"]));
        assert_eq!(
            decision.review_date,
            NaiveDate::parse_from_str(&json_str(&want["review_date"]), "%Y-%m-%d").unwrap()
        );
        assert_eq!(
            decision.invalidation_criteria,
            json_str(&want["invalidation_criteria"])
        );
        assert_eq!(
            decision.thesis_id.as_deref(),
            json_opt_str(&want["thesis_id"]).as_deref()
        );
        assert_eq!(
            decision.thesis_claim_snapshot.as_deref(),
            json_opt_str(&want["thesis_claim_snapshot"]).as_deref()
        );
        assert_eq!(decision.created_at, ts(&json_str(&want["created_at"])));
        assert_eq!(decision.status, json_str(&want["status"]));

        let history = delta_services::decisions::review_history(&db, &decision.id).unwrap();
        let dumped_history = &exp["decision_reviews"][&decision.id];
        assert_eq!(history.len(), dumped_history.as_array().unwrap().len());
        for (entry, want_entry) in history.iter().zip(dumped_history.as_array().unwrap()) {
            assert_eq!(entry.id, want_entry["id"].as_i64().unwrap());
            assert_eq!(entry.note, json_str(&want_entry["note"]));
            assert_eq!(entry.created_at, ts(&json_str(&want_entry["created_at"])));
            assert_eq!(
                entry.status.as_deref(),
                json_opt_str(&want_entry["status"]).as_deref()
            );
        }
    }
    let due = delta_services::decisions::due_reviews(
        &db,
        Some(chrono::NaiveDate::from_ymd_opt(2026, 9, 21).unwrap()),
    )
    .unwrap();
    let due_ids: Vec<String> = due.iter().map(|d| d.id.clone()).collect();
    let want_due: Vec<String> = exp["due_reviews"]
        .as_array()
        .unwrap()
        .iter()
        .map(json_str)
        .collect();
    assert_eq!(due_ids, want_due);
}

#[test]
fn audit_and_queue_match_python() {
    let db = seed_db();
    let exp = expected();
    let now = ts(&json_str(&exp["now"]));

    let sources = std::collections::BTreeSet::from(["sec_edgar".to_string()]);
    let audit =
        delta_services::review::evidence_audit(&db, "US:AAPL", now, &sources, None).unwrap();
    let want = &exp["audit_aapl"];
    assert_eq!(
        audit
            .price_at
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
        json_opt_str(&want["price_at"])
    );
    assert_eq!(
        audit
            .news_at
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
        json_opt_str(&want["news_at"])
    );
    assert_eq!(
        audit
            .primary_at
            .map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
        json_opt_str(&want["primary_at"])
    );
    assert_eq!(audit.primary_coverage, json_str(&want["primary_coverage"]));
    assert_eq!(
        audit.non_price_items,
        want["non_price_items"].as_u64().unwrap() as usize
    );
    assert_eq!(
        audit.source_count,
        want["source_count"].as_u64().unwrap() as usize
    );
    let warnings: Vec<String> = want["warnings"]
        .as_array()
        .unwrap()
        .iter()
        .map(json_str)
        .collect();
    assert_eq!(audit.warnings, warnings);

    // The queue's rig: both seed instruments, sec_edgar enabled (its market
    // falls back to "us").
    use delta_core::models::{AssetClass, Instrument};
    let universe = vec![
        Instrument {
            id: "US:AAPL".to_string(),
            market: "us".to_string(),
            symbol: "AAPL".to_string(),
            name: None,
            currency: "USD".to_string(),
            sector: None,
            asset_class: AssetClass::Equity,
            watchlists: Vec::new(),
            tags: Default::default(),
            industry: None,
            meta: Default::default(),
        },
        Instrument {
            id: "US:MSFT".to_string(),
            market: "us".to_string(),
            symbol: "MSFT".to_string(),
            name: None,
            currency: "USD".to_string(),
            sector: None,
            asset_class: AssetClass::Equity,
            watchlists: Vec::new(),
            tags: Default::default(),
            industry: None,
            meta: Default::default(),
        },
    ];
    let plugins = std::collections::BTreeMap::from([(
        "sec_edgar".to_string(),
        delta_services::review::PluginInfo {
            enabled: true,
            market: None,
        },
    )]);
    let queue = delta_services::review::review_queue(
        &db,
        &plugins,
        &universe,
        &[],
        Some(now - chrono::Duration::days(7)),
        now,
    )
    .unwrap();
    let dumped_queue = exp["review_queue"].as_array().unwrap();
    assert_eq!(queue.len(), dumped_queue.len(), "queue size");
    for (item, want_item) in queue.iter().zip(dumped_queue) {
        assert_eq!(item.kind, json_str(&want_item["kind"]), "{:?}", item);
        assert_eq!(item.instrument_id, json_str(&want_item["instrument_id"]));
        assert_eq!(item.title, json_str(&want_item["title"]));
        assert_eq!(item.detail, json_str(&want_item["detail"]));
        assert_eq!(
            item.evidence_id.as_deref(),
            json_opt_str(&want_item["evidence_id"]).as_deref()
        );
        assert_eq!(
            item.thesis_id.as_deref(),
            json_opt_str(&want_item["thesis_id"]).as_deref()
        );
        assert_eq!(
            item.ts.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
            json_opt_str(&want_item["ts"])
        );
    }
}

#[test]
fn setup_checks_match_python() {
    let db = seed_db();
    let exp = expected();
    // Bare temp cwd: no config.toml, no .env, provider keys cleared (the
    // dumper guarantees the same environment).
    let dir = tempfile::tempdir().unwrap();
    let env = dir.path().join(".env");
    for key in [
        "OPENROUTER_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "CUSTOM_API_KEY",
    ] {
        std::env::remove_var(key);
    }
    let settings = delta_core::config::Settings::default();
    let cfg = delta_core::config::AppConfig {
        llm_provider: "openrouter".to_string(),
        plugins: std::collections::BTreeMap::from([(
            "sec_edgar".to_string(),
            serde_json::json!({}),
        )]),
        ..delta_core::config::AppConfig::default()
    };
    // The dumper runs with the seed's data via the rig engine; replicate the
    // db-dependent checks by pointing at the same seed.
    let checks = delta_services::setup::setup_checks(
        &settings,
        &cfg,
        &db,
        &dir.path().join("config.toml"),
        &env,
    )
    .unwrap();
    let dumped = exp["setup_checks"].as_array().unwrap();
    assert_eq!(checks.len(), dumped.len());
    for (check, want) in checks.iter().zip(dumped) {
        assert_eq!(check.name, json_str(&want["name"]));
        assert_eq!(check.ok, want["ok"].as_bool().unwrap(), "{}", check.name);
        assert_eq!(check.fix, json_str(&want["fix"]), "{}", check.name);
    }
}

/// Cross-read, Rust->JSON half: theses and decisions written through the Rust
/// APIs store exactly the encodings Python reads (same column formats as the
/// seed's raw rows). Run
/// `uv run python tests/dump_shared_expected.py --verify-written target/r3-roundtrip/written.db`
/// afterwards for the Python-models half against the same file.
#[test]
fn rust_written_rows_round_trip_through_python_encodings() {
    use delta_services::decisions::{append_review, create_decision};
    use delta_services::theses::{add_evidence, create_thesis, thesis_id};

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("written.db");
    let db = Db::open(&path).unwrap();
    let created_at =
        NaiveDateTime::parse_from_str("2026-09-20 12:00:00", "%Y-%m-%d %H:%M:%S").unwrap();

    let thesis = create_thesis(
        &db,
        "Rust writes read back in Python",
        "parity",
        &["encoding matches".to_string()],
        &["mismatch".to_string()],
        &["US:AAPL".to_string()],
        "3m",
        Some(created_at),
    )
    .unwrap();
    add_evidence(
        &db,
        &thesis.id,
        "news:roundtrip",
        "support",
        "linked by Rust",
        Some(true),
    )
    .unwrap();
    let decision = create_decision(
        &db,
        "US:AAPL",
        "Written through the Rust API",
        "10x forward",
        "1y",
        chrono::NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
        "Parity breaks",
        Some(&thesis.id),
        Some(created_at),
    )
    .unwrap();
    append_review(
        &db,
        &decision.id,
        "First look.",
        None,
        Some(created_at + chrono::Duration::hours(1)),
    )
    .unwrap();
    append_review(
        &db,
        &decision.id,
        "Still on plan.",
        Some("reviewed"),
        Some(created_at + chrono::Duration::hours(20)),
    )
    .unwrap();
    drop(db);

    // Dump the raw rows the way the Python dumper does.
    let conn = rusqlite::Connection::open(&path).unwrap();
    let mut raw = serde_json::Map::new();
    for table in ["thesis", "thesis_evidence", "decision", "decision_review"] {
        let mut stmt = conn
            .prepare(&format!("SELECT * FROM {table} ORDER BY rowid"))
            .unwrap();
        let columns: Vec<String> = stmt
            .column_names()
            .into_iter()
            .map(str::to_string)
            .collect();
        let rows: Vec<Vec<Value>> = stmt
            .query_map([], |row| {
                Ok((0..columns.len())
                    .map(|i| {
                        let v: rusqlite::types::Value = row.get(i).unwrap();
                        match v {
                            rusqlite::types::Value::Null => Value::Null,
                            rusqlite::types::Value::Integer(n) => Value::from(n),
                            rusqlite::types::Value::Real(f) => Value::from(f),
                            rusqlite::types::Value::Text(s) => Value::from(s),
                            rusqlite::types::Value::Blob(b) => {
                                Value::from(String::from_utf8_lossy(&b).to_string())
                            }
                        }
                    })
                    .collect())
            })
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();
        raw.insert(
            table.to_string(),
            serde_json::json!({ "columns": columns, "rows": rows }),
        );
    }
    let raw = Value::Object(raw);

    // Thesis row: id is the stable (claim, scope) hash; lists are JSON text;
    // created_at is SQLModel's datetime format.
    let thesis_row = &raw["thesis"]["rows"][0];
    assert_eq!(thesis_row[0], Value::from(thesis.id.clone()));
    assert_eq!(
        thesis_row[0],
        Value::from(thesis_id("Rust writes read back in Python", "parity"))
    );
    assert_eq!(
        thesis_row[1],
        Value::from("Rust writes read back in Python")
    );
    assert_eq!(thesis_row[2], Value::from("parity"));
    assert_eq!(thesis_row[3], Value::from("[\"encoding matches\"]"));
    assert_eq!(thesis_row[4], Value::from("[\"mismatch\"]"));
    assert_eq!(thesis_row[5], Value::from("[\"US:AAPL\"]"));
    assert_eq!(thesis_row[6], Value::from("3m"));
    assert_eq!(thesis_row[7], Value::from("2026-09-20 12:00:00.000000"));
    assert_eq!(thesis_row[8], Value::from("active"));

    let link_row = &raw["thesis_evidence"]["rows"][0];
    assert_eq!(link_row[0], Value::from(thesis.id.clone()));
    assert_eq!(link_row[1], Value::from("news:roundtrip"));
    assert_eq!(link_row[2], Value::from("support"));
    assert_eq!(link_row[3], Value::from("linked by Rust"));
    assert_eq!(link_row[4], Value::from(1));

    // Decision row: uuid-hex id, DATE review_date, DATETIME created_at,
    // snapshotted claim, status.
    let decision_row = &raw["decision"]["rows"][0];
    assert_eq!(decision_row[0], Value::from(decision.id.clone()));
    assert_eq!(decision.id.len(), 32);
    assert_eq!(decision_row[1], Value::from("US:AAPL"));
    assert_eq!(decision_row[5], Value::from("2026-10-01"));
    assert_eq!(decision_row[7], Value::from(thesis.id.clone()));
    assert_eq!(
        decision_row[8],
        Value::from("Rust writes read back in Python")
    );
    assert_eq!(decision_row[9], Value::from("2026-09-20 12:00:00.000000"));
    assert_eq!(decision_row[10], Value::from("reviewed"));

    let review_rows = raw["decision_review"]["rows"].as_array().unwrap();
    assert_eq!(review_rows.len(), 2);
    assert_eq!(review_rows[0][1], Value::from(decision.id.clone()));
    assert_eq!(review_rows[0][2], Value::from("First look."));
    assert_eq!(review_rows[0][3], Value::from("2026-09-20 13:00:00.000000"));
    assert_eq!(review_rows[0][4], Value::Null);
    assert_eq!(review_rows[1][2], Value::from("Still on plan."));
    assert_eq!(review_rows[1][3], Value::from("2026-09-21 08:00:00.000000"));
    assert_eq!(review_rows[1][4], Value::from("reviewed"));

    // Leave the file in the workspace target dir for the Python verifier.
    let keep = repo_root().join("target/r3-roundtrip/written.db");
    std::fs::create_dir_all(keep.parent().unwrap()).unwrap();
    std::fs::copy(&path, &keep).unwrap();
}
