# Findings — rust-core (R1a)

Stream: `delta-core`. Reviewed against `delta/core/*.py`.

| # | Category | Where | Finding | Proposal |
|---|---|---|---|---|
| 1 | simplify | delta/core/events.py | Bus is async but every subscriber in the codebase is fire-and-forget; asyncio adds no ordering guarantee | Rust port uses sync inline handlers; revisit only when an async subscriber exists |
| 2 | simplify | delta/core/db.py:211 | `store_items` chunks by SQLITE_MAX_VARIABLE_NUMBER, but SQLAlchemy still round-trips per statement; chunking only saves round-trips | Rust port inserts row-by-row inside one transaction (same semantics); batch if ingest profiling demands it |
| 3 | simplify | delta/core/config.py `update_config` | Uses `tomli_w`, so comments in config.toml are already lost on every write — the Rust plan's "toml_edit keeps comments" goal is stricter than current Python behaviour | Rust port matches Python (full rewrite via `toml`). Switching to toml_edit would be a behaviour *improvement*, not parity — your call |

## Crate deviations from the plan

- `rusqlite` (bundled) instead of `sqlx`: delta-core's DB access is
  synchronous, and sqlx's compile-time checking buys little against a schema
  defined by the Python app. sqlx remains an option for `delta-services` if
  async query concurrency shows up in profiles.
- Findings from the plan's adversarial review pass (end of R1) go here too.
