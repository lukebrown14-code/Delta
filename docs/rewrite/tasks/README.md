# Task cards

One card per stream. An agent reads only its card, `agent/AGENTS.md`,
`docs/RUST_REWRITE_PLAN.md` and `docs/rewrite/REMAINING.md`.

## Rules for every stream

- Branch from the latest `rewrite/rust`; open one PR back into it.
- Touch only the paths the card says it **owns**. Anything else: stop and ask.
- Gates, in order, all green: `cargo fmt --all --check`,
  `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`
  (golden matrix included), and `uv run pytest -q` (Python stays green).
- Tests are offline: `wiremock` for HTTP, `FakeLlm` for models. No live calls.
- Port Python behaviour exactly. Log anything else in the card's findings file
  (category, Python file:line, evidence, proposal). Provenance, citation or
  prompt changes: log and wait, never change silently.
- A golden mismatch you can't avoid: log it in `docs/rewrite/DEVIATIONS.md` and stop.
  An agent never approves its own deviation.
- Crates: only those approved in `REMAINING.md` D13.
- Before merge, a separate reviewer agent (no shared context) reviews the PR
  against the Python source and the card's done list, and returns pass/fail.
- Update `agent/codemap/rust.md` for files and symbols you add or move.

## Screen streams (R3.2) also

- Build the screen as a data-driven `Component` on the R3.1a framework; no
  fixed coordinates. Retire the old painter for the screen.
- First, add the screen's scenarios to its module under `tests/golden_scenarios/`
  and export them; then port to zero Tier A mismatches at 80x24, 120x40 and 200x50.
- Bindings match Python exactly (`BINDINGS` in the Python screen).
