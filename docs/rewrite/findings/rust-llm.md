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

## Open questions for triage

| # | Category | Where | Finding | Proposal |
|---|---|---|---|---|
| 1 | bug-risk | delta/llm/structured.py retry | The retry call re-sends the *retry* prompt whose text differs from the original, so it gets a different cache key — a retry result is cached under its own key and a repeated failure re-calls live every time. Rust port preserves this behaviour (parity first) | Confirm intended; a shared key would change cache semantics |
| 2 | simplify | delta/llm/client.py `complete` | 9 positional args invite transposition; Rust port uses a `CompleteParams` struct | No action in Python needed |
| 3 | perf | delta/llm/providers.py `_estimate_prompt_cost` | chars/4 heuristic counts Rust `char` lengths vs Python `len()` — differs only for astral-plane chars (emoji); negligible for cost estimation | None |
| 4 | parity | delta/llm/providers.py 402 retry | Retry only fires when the refitted cap is strictly lower than the previous cap; Rust matches | None |
