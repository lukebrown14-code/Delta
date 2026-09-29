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

## Parity verified

`tests/analytics.rs` pins `data_health`, `recent_closes`, `llm_costs`, `total_spend`,
`pulse` (counts, total, busiest/quietest, empty short window), `upcoming_events`,
and `latest_headline` to outputs produced by running `delta.services` on the
identically seeded DB (seed script preserved in the test's `seeded_db`).

## Not ported (rolled into later work — none silently dropped)

| # | Category | Where | Finding |
|---|---|---|---|
| 1 | gap | `classify_sentiment` / `jev.py` | The Jev typed-decision client is not ported; `gather` therefore runs ingest+extract and returns the sentiment stage unimplemented. Block it behind a TODO in `gather` until jev lands (R3 ask flow needs it) |
| 2 | gap | `brief_for` / `delta/brief.py` | Deferred to R3 with the screens that render briefs |
| 3 | gap | `thesis_fleet` / `theses.py` / `thesis_health.py` / `evidence.py` | The thesis tables and health model are their own port (large; R3 Theses screen dependency) |
| 4 | gap | `data_provider_status` / `configure_data_provider` / `setup_checks` | Need `DataProviderSpec` on the plugin trait; small follow-up once the settings screen is scheduled |
| 5 | parity | `extract.rs` validation | Python validates `sentiment` in [-1, 1] via pydantic (a bad batch is skipped whole). The Rust port clamps nothing and skips on serde failure only — a model returning sentiment 5 would store it. Proposal: add the range check to `EventDraft` with `#[serde(try_from)]`-style validation; flagged rather than silently changed |
| 6 | simplify | `pulse` floor | Python reads both tables with `>= floor`; the Rust port filters per row over the same span. Same output, one fewer temp |
| 7 | perf | `analytics.rs` readers | `pulse`/`upcoming_events` load full rows instead of column projections; fine at desk scale, revisit only if profiling says so |
