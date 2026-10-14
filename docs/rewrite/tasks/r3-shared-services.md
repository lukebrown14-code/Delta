# R3.1b — Shared services

Services used by more than one screen, ported first so the R3.2 streams don't
block each other.

- **Branch:** `rewrite/r3-shared-services`
- **Runs:** parallel with R3.1a and R3.1c, after R3.0
- **Findings:** `docs/rewrite/findings/rust-services.md`

## Owns

- `crates/delta-services/src/{evidence,theses,decisions,review,setup}.rs` and their tests
- `crates/delta-services/src/thesis_health.rs` (move `evidence_by_ids` into `evidence.rs`)
- `crates/delta-plugins/src/plugin.rs` (`DataProviderSpec` and the calendar registration only), new `crates/delta-plugins/src/calendar.rs`

## Python sources → Rust

| Python | Rust | Used by |
|---|---|---|
| `delta/evidence.py` (all: `evidence`, `cite`, `source_quality`, `falsifier_hit`) | `evidence.rs` | research, ask, theses |
| `delta/theses.py` (CRUD, evidence links, `accepted_items`, `_gather`) | `theses.rs` | theses, decisions, research |
| `delta/decisions.py` (CRUD, reviews, `due_reviews`, `relink_thesis`) | `decisions.rs` | decisions, home, cli |
| `delta/review.py` (`evidence_audit`, `review_queue`) | `review.rs` | home, research |
| `services.setup_checks`, `data_provider_status`, `configure_data_provider` | `setup.rs` + `DataProviderSpec` | home, settings |
| `delta/plugins/data/yfinance_calendar.py` | `calendar.rs` | ingest |

## Python tests to port

`test_evidence.py`, `test_theses.py`, `test_decisions.py`, `test_review.py`,
`test_provider_setup.py` (service parts), `test_yfinance_calendar.py`.

## Done

- [ ] Every function above ported, with the Python tests' cases reproduced in Rust.
- [ ] Parity: outputs on `fixtures/golden_seed.db` match Python (a Python script dumps the expected values as JSON; Rust asserts them).
- [ ] Theses and decisions tables written by Rust read back correctly in Python, and the reverse.
- [ ] Evidence ID format and citation rules unchanged (provenance tests).
