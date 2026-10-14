# R3.1c — Findings fixes

The triaged findings from R1/R2 (decisions D5–D8 in `REMAINING.md`).

- **Branch:** `rewrite/r3-fixes`
- **Runs:** parallel with R3.1a and R3.1b, after R3.0
- **Findings:** update the source finding rows to "fixed"

## Owns

`crates/delta-plugins/src/markets.rs`, `crates/delta-core/src/config.rs`,
`crates/delta-llm/src/{client,providers}.rs`, `crates/delta-services/src/pipeline.rs`,
and the `Cargo.toml` of those crates.

## Work

| # | Fix | Test |
|---|---|---|
| D5 | `chrono-tz`: US = `America/New_York`, ASX = `Australia/Sydney` | Sessions and `next_open` on both sides of each DST change, both hemispheres |
| D6 | `toml_edit` for config writes; comments and key order preserved | Write a commented `config.toml`, edit via each `update_config` path, comments intact, Python still parses it |
| D7 | LLM retry: 429/5xx/timeouts, max 5 attempts, exponential backoff + jitter, `Retry-After` honoured; the 402 refit path unchanged | wiremock: 429 then 200; 5xx ×5 fails; `Retry-After` respected (with a time seam) |
| D8 | Extract: sentiment outside [-1, 1] skips the whole batch | Batch with sentiment 5 is skipped, nothing stored, matching Python |

## Done

- [ ] All four fixed with tests; finding rows marked fixed.
- [ ] No change to cache keys, cost logging or prompts.
