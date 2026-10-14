# Findings — rust-core (R1a)

Stream: `delta-core`. Reviewed against `delta/core/*.py`.

| # | Category | Where | Finding | Proposal |
|---|---|---|---|---|
| 1 | simplify | delta/core/events.py | Bus is async but every subscriber in the codebase is fire-and-forget; asyncio adds no ordering guarantee | Rust port uses sync inline handlers; revisit only when an async subscriber exists |
| 2 | simplify | delta/core/db.py:211 | `store_items` chunks by SQLITE_MAX_VARIABLE_NUMBER, but SQLAlchemy still round-trips per statement; chunking only saves round-trips | Rust port inserts row-by-row inside one transaction (same semantics); batch if ingest profiling demands it |
| 3 | simplify | delta/core/config.py `update_config` | Uses `tomli_w`, so comments in config.toml are already lost on every write — the Rust plan's "toml_edit keeps comments" goal is stricter than current Python behaviour | Rust port matches Python (full rewrite via `toml`). Switching to toml_edit would be a behaviour *improvement*, not parity — your call |

## Fixed

- **#3 (config comments) — fixed in e9da059** (R3.1c, decision D6):
  `update_config` edits in place with `toml_edit` and replays only the
  before -> after diff, so comments and key order of untouched entries
  survive (an improvement over Python, as the finding anticipated). Existing
  keys are updated through in-place value swaps — `Table::insert` re-formats
  the key and would drop its comment. Every caller path is exercised against
  a commented config.toml, and the written file is parsed with Python's
  stdlib `tomllib` in `crates/delta-services/tests/config_comments.rs`.

## Crate deviations from the plan

- `rusqlite` (bundled) instead of `sqlx`: delta-core's DB access is
  synchronous, and sqlx's compile-time checking buys little against a schema
  defined by the Python app. sqlx remains an option for `delta-services` if
  async query concurrency shows up in profiles.
- Findings from the plan's adversarial review pass (end of R1) go here too.
