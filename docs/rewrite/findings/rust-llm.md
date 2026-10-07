# Findings — rust-llm (R1c)

Stream: `delta-llm`. Reviewed against `delta/llm/*.py`.

## Ported

| Python | Rust | Notes |
|---|---|---|
| `eval.py` | `src/eval.rs` | provenance contract, ported first per plan |
| `json.py` | `src/json.rs` | fence-stripping parity (splitn 2 fences, optional `json` prefix) |
| `router.py` | `src/router.rs` | single model > plugin override > routing; fails loudly |
| `cache.py` | `delta-core/src/db.rs` (`llm_lookup`, `llm_store_call`) | same table, same miss rules (row without `response` is a miss) |
| `client.py` | `src/client.rs` | cache key `stable_id(model, prompt_version, prompt)`; hits logged as `cached=true` rows; `cache_validator` poison gate |
| `providers.py` | `src/providers.rs` | reqwest instead of the `openai` SDK; OpenRouter pricing/credits/auto-fit (402 retry, `MAX_TOKENS_FLOOR`, 0.95 margin), Anthropic `x-api-key` headers, `verify_key` |
| `structured.py` | `src/structured.rs` | minijinja `StrictUndefined` over verbatim copies of `prompts/*.j2`; retry-once on invalid JSON; cache gated on schema validation |

## Not yet ported (roll into R2 or later)

- `catalog.py` (model catalog / `data/model_catalog.json` picker data) — UI-facing, deferred to R3 settings work.
- `jev.py` (typed decision client) — depends on services flows; deferred with R2.
- `client.py` retry policy: Python relies on the `openai` SDK's built-in `max_retries=5`; the Rust port has no retry loop yet. Proposal: add bounded retry with backoff on 429/5xx in a follow-up.
- Streaming: the plan mentions it, but the post-audit Python client has no streaming path — nothing to port until Python grows one.

## Fixed

- **client.py retry policy — fixed in 85992f6** (R3.1c, decision D7):
  `post_chat` now retries 429/5xx/timeout failures up to 5 attempts with
  exponential backoff (500ms doubling, 8s cap) plus bounded jitter,
  honouring `Retry-After` verbatim; 402 passes straight through so the
  OpenRouter refit path is unchanged. The sleep is a `RetrySleep` seam, and
  `crates/delta-llm/tests/retry.rs` covers 429-then-200, 5xx ×5, timeouts,
  non-retryable 4xx and the 402 refit flow over wiremock without real
  sleeping.

- **R3.0 item 6 (Rust `FakeLlm`) — fixed in 71ffdcf** (deferred to R3.1c,
  which owns `delta-llm`): `delta_llm::fake::FakeLlm` is the trait-object
  test double loading `fixtures/llm/<task>/<name>.json`
  (`{"cost_usd": float, "text": payload}`) with the same keying as Python's
  `load_llm_fixtures` (`<task>/response.json` addressable as `<task>`), so
  both ecosystems read the same files. Injectable as the `Provider` of a
  real `LlmClient` — `serve("report/apple")` selects the canned response and
  the fixture's `cost_usd` flows through the untouched cost logging onto the
  `llmcall` row. The R3.2 chat/report streams inject it through
  `LlmClient::new(Arc::new(FakeLlm::load(root)?))`. Tests in
  `crates/delta-llm/tests/fake.rs`.

## Open questions for triage

| # | Category | Where | Finding | Proposal |
|---|---|---|---|---|
| 1 | bug-risk | delta/llm/structured.py retry | The retry call re-sends the *retry* prompt whose text differs from the original, so it gets a different cache key — a retry result is cached under its own key and a repeated failure re-calls live every time. Rust port preserves this behaviour (parity first) | Confirm intended; a shared key would change cache semantics |
| 2 | simplify | delta/llm/client.py `complete` | 9 positional args invite transposition; Rust port uses a `CompleteParams` struct | No action in Python needed |
| 3 | perf | delta/llm/providers.py `_estimate_prompt_cost` | chars/4 heuristic counts Rust `char` lengths vs Python `len()` — differs only for astral-plane chars (emoji); negligible for cost estimation | None |
| 4 | parity | delta/llm/providers.py 402 retry | Retry only fires when the refitted cap is strictly lower than the previous cap; Rust matches | None |

## R3.1c review findings (7 Oct, reviewer agent — triage pending)

| # | Category | Location | Evidence | Proposed fix |
|---|---|---|---|---|
| 5 | bug | crates/delta-llm/src/providers.rs:540 `retry_delay` | `Duration::from_secs_f64` on a server-supplied `Retry-After`; a hostile/broken header like `1e300` passes the finite/>=0 filter and panics the task | Clamp/saturate, e.g. `try_from_secs_f64().unwrap_or(RETRY_MAX_DELAY)` |
| 6 | parity | crates/delta-llm/src/providers.rs `EventDraft.sentiment` | `#[serde(default)]` makes an omitted sentiment store 0.0 in Rust; Python's `Field(ge=-1, le=1)` is required and rejects the whole batch (delta/extract.py:34) | Make the field required for full pydantic parity |
| 7 | docs | crates/delta-llm/src/fake.rs | FakeLlm serialises `text` compact (`serde_json::to_string`) vs Python `json.dumps` space-y separators — semantically identical JSON; the "exactly as Python serialises" doc claim is overstated | Soften the doc comment |
| 8 | note | crates/delta-llm/src/providers.rs | D7 also retries connect errors (`is_connect`) — a harmless superset of the card's 429/5xx/timeouts; no duplicate reaches the server | None (document) |
