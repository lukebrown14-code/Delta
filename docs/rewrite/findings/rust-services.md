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

## R3.1b — shared services (ported)

Stream `r3-shared-services`. Closes the "Not ported" rows **#3** (theses write
path), **#4** (data-provider status / configure / setup checks) and the
plugins-file row **#5** (`yfinance_calendar`).

| Python | Rust | Notes |
|---|---|---|
| `delta/evidence.py` (all) | `src/evidence.rs` | `evidence`, `evidence_by_ids` (moved here from `thesis_health.rs`), `cite`, `source_quality`, shared `falsifier_hit` (kind+title+body). `EvidenceItem` gained the `raw` payload and `quality` fields the Python model carries |
| `delta/theses.py` (CRUD, evidence links, `accepted_items`, `_gather`) | `src/theses.rs` | Tables created idempotently; `update_thesis` migrates links on rename and calls `decisions::relink_thesis`. Error messages carry the Python text (`status must be one of ...`, `ambiguous thesis id prefix`, ...) |
| `delta/decisions.py` (all) | `src/decisions.rs` | Append-only reviews, `due_reviews`, `relink_thesis`; SQLModel-compatible DDL so fresh Rust DBs read back in Python |
| `delta/review.py` (`evidence_audit`, `review_queue`) | `src/review.rs` | Same stale/thin warning strings, the four priority groups, dedupe and `(ts desc, instrument, evidence, thesis)` ordering |
| `services.setup_checks` / `data_provider_status` / `configure_data_provider` | `src/setup.rs` + `DataProviderSpec` | Secrets go to `.env` only; non-secret fields + scope markets + `enabled = true` land in `config.toml` |
| `delta/plugins/data/yfinance_calendar.py` | `delta-plugins/src/calendar.rs` | No Rust yfinance: the calendar is read from quoteSummary `calendarEvents` (what yfinance's `.calendar` wraps) through the shared client's cookie/crumb handshake; dates keep yfinance's display keys so the event rules port one for one |

**Parity:** `tests/dump_shared_expected.py` runs the Python implementations
over a copy of `fixtures/golden_seed.db` (frozen clock 2026-09-21) and dumps
`tests/fixtures/shared_services_expected.json`; `tests/shared_parity.rs`
asserts the full evidence pool (173 items, field for field), every filtered
id list, theses + links + accepted pairs, decisions + histories + due,
`evidence_audit`, the full `review_queue`, and the five `setup_checks`
against it. Cross-read: the round-trip test writes theses/decisions through
the Rust APIs and proves the stored encodings equal the seed's raw rows; the
dumper's `--verify-written <db>` reads that same Rust-written file through
the Python SQLModel models (verified green).

**Python test cases ported:** `test_evidence.py` (all 13), `test_theses.py`
(persistence cases; LLM/screen cases belong to R3.2), `test_decisions.py`
(all 10), `test_review.py` (all 5), `test_provider_setup.py` +
`test_data_sources.py` (service parts), `test_yfinance_calendar.py`
(behaviour cases against wiremock + pure parsers).

## New findings (R3.1b)

| # | Category | Where | Finding |
|---|---|---|---|
| 8 | deviation | `decisions.rs::new_decision_id` | Python ids are `uuid4().hex`; Rust has no approved uuid crate, so the id is the first 32 hex chars of sha256 over wall-clock nanoseconds + pid + counter. Same 32-hex format and uniqueness contract, not v4-layout. Nothing parses the id. If a uuid crate is ever approved, swap the helper |
| 9 | gap | `review.rs` / `setup.rs` | Python reads `plugin.enabled` / `plugin.market` / `plugin.provider_spec` off live plugin objects; the Rust registry is stateless, so review/setup take a `PluginInfo` map and the spec table as inputs. When the runtime rig lands (R3.2 settings/research wiring), feed it from the registry |
| 10 | gap | `theses.rs` | `propose_evidence` (the LLM discovery call, `ThesisDraft` payload and `thesis_v1.j2` prompt wiring) is deliberately not ported here — the card scoped CRUD/links/`accepted_items`/`_gather`; the call belongs to the R3.2 theses stream |
| 11 | simplify | `setup.rs::configure_data_provider` | Python's trailing `delta.reload_data_sources()` hook has no Rust counterpart (no live registry yet); callers rebuild plugins. Note for the settings stream |
| 12 | simplify | `delta-plugins/plugin.rs` | `DataProviderSpec` statics sit in `plugin.rs` behind `provider_specs()` instead of per-plugin trait impls, so the parallel streams never touched files they don't own (`sec.rs` etc.). A later stream can move the spec into `impl DataPlugin for SecEdgar` without a behaviour change |
| 13 | parity | `evidence.rs` | Case-insensitive matching uses `to_lowercase()` where Python uses `str.casefold()`. Identical for ASCII (all stored data today); only a non-ASCII German-sharp-s / dotless-i edge could diverge. Flagged, not changed |
## Fixed

- **#5 (sentiment range) — fixed in 74444e1** (R3.1c, decision D8):
  `EventDraft.sentiment` deserializes through a [-1, 1] range check
  (pydantic's `Field(ge=-1, le=1)`), so one out-of-range sentiment fails the
  whole `EventBatch` and the extract loop skips it — nothing stored, as in
  Python. The failed response also fails the structured cache validator, so
  a poisoned answer is recalled live. Tests in
  `crates/delta-services/tests/extract_sentiment.rs` cover the skip and the
  inclusive boundaries.

## R3.1b review findings (7 Oct, reviewer agent — triage pending)

| # | Category | Location | Evidence | Proposed fix |
|---|---|---|---|---|
| R1 | parity | crates/delta-services/src/setup.rs:42-55 | `provider_key` trims API-key values before the emptiness check; Python `_provider_key` (services.py:766-770) does not — whitespace-only key counts as configured in Python, missing in Rust | Match Python: don't trim before the emptiness check |
| R2 | parity | crates/delta-services/src/setup.rs:180-185 | `data_provider_status` `configured` treats 0/""/[]/{} as truthy (only Null/false falsy); Python `bool(table.get(...))` is falsy for those | Mirror Python truthiness |
| R3 | parity | crates/delta-services/src/setup.rs:234-266 | `configure_data_provider` error precedence: Python raises "unknown markets" before "{label} is required"; Rust reverses. Multiple empty required fields report alphabetical (Rust) vs insertion (Python) order | Match Python order |
| R4 | docs | crates/delta-services/src/theses.rs:201 | Comment claims Python's `startswith` is literal; Python `.startswith()` on a LIKE-built query treats %/_ as wildcards. Unreachable with sha256-hex ids | Fix the comment |
| R5 | note | crates/delta-plugins/src/calendar.rs | Calendar fetch error log drops the instrument id Python logs | Add the id to the log line |
| R6 | note | tests/shared_parity.rs | Parity harness never compares the dumped `raw` field ("field for field" overstated); verified equivalent by code read | Add `raw` to the comparison |
| R7 | note | crates/delta-services/src/decisions.rs | No now-path `created_at` round-trip regression test (theses has one); the µs fix itself is verified correct | Add a test mirroring theses |
