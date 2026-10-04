# Findings — rust-services (R2)

Stream: `delta-services`. Reviewed against `delta/services.py`, `delta/extract.py`, `delta/targets.py`.

## Ported

| Python | Rust | Notes |
|---|---|---|
| `ingest` / `gather` | `src/pipeline.rs` | bar-since floor with 1-day overlap (B2), market/ticker filters, `ingest_workers` cap, per-plugin scope filtering, idempotent store with per-plugin log lines |
| `delta/extract.py` | `src/pipeline.rs::extract_events` | uncovered-news selection, per-instrument batching, citation-traceability drop rule, per-batch persistence, `event_id` contract, prompt `extract_v1` verbatim |
| `data_health`, `recent_closes`, `llm_costs`, `total_spend` | `src/analytics.rs` | grouped aggregation; only grouped columns read (no response payloads) |
| `pulse` / `upcoming_events` | `src/analytics.rs` | published-time semantics, future events excluded here and owned by `upcoming_events` (boundary comment preserved), whole-window tally, `(-count, id)` ranking |
| `latest_headline` | `src/analytics.rs` | newest `scan` rows, in-memory id filter |
| `latest_report` / `latest_report_age` | `src/analytics.rs` | sidecar `as_of`, filename-date fallback, non-date stem → None |
| targets group | `src/config_ops.rs` + `src/targets.rs` | `target_specs`/`add_target`/`remove_target` with the same validation messages; legacy kind-by-shape modelling |
| markets group | `src/config_ops.rs` | `market_profiles`/`save_market`/`remove_market` with dependency check; `set_plugin_enabled` |
| `classify_sentiment` / `jev.py` | `src/sentiment.rs`, `delta-llm/src/jev.rs` | cached Jev decision client, stance validation, persistence and aggregate coverage |
| `brief_for` / `brief.py` | `src/brief.rs` | deterministic brief from prices, evidence, events and stance data |
| thesis fleet, evidence and health | `src/theses.rs`, `src/thesis_health.rs`, `src/evidence.rs` | CRUD, candidate/link workflow, filtering and computed health |
| provider status/setup | `src/config_ops.rs` | provider status, configuration fields and setup validation |

## Parity verified

`tests/analytics.rs` pins `data_health`, `recent_closes`, `llm_costs`, `total_spend`,
`pulse` (counts, total, busiest/quietest, empty short window), `upcoming_events`,
and `latest_headline` to outputs produced by running `delta.services` on the
identically seeded DB (seed script preserved in the test's `seeded_db`).

## Findings for follow-up

| # | Category | Where | Finding |
|---|---|---|---|
| 1 | simplify | `pulse` floor | Python reads both tables with `>= floor`; the Rust port filters per row over the same span. Same output, one fewer temp |
| 2 | perf | `analytics.rs` readers | `pulse`/`upcoming_events` load full rows instead of column projections; fine at desk scale, revisit only if profiling says so |
