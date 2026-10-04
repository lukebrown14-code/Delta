# R3.2 — Headless CLI (new; audit G8)

Python never had this, so there is no parity target. Behaviour is defined here.

- **Branch:** `rewrite/r3-cli`
- **Runs:** parallel, after R3.1. `report` lands after `r3-research` merges `reports.rs`
- **Findings:** `docs/rewrite/findings/rust-services.md`

## Owns

New `crates/delta/` (binary `delta`, clap); `crates/delta-tui` becomes a library
plus `run_tui()`. Moves `delta-tui/src/main.rs` startup into the new crate.
`.github/workflows/release.yml` / `Cargo.toml` dist targets if the binary name moves.

## Commands

| Command | Behaviour |
|---|---|
| `delta` | Launches the TUI (default; same as today) |
| `delta --version` | Prints the version without touching the terminal (keep the benchmark path) |
| `delta gather [--target X]` | Ingest + extract + sentiment for all targets or one; per-source summary to stdout; exit 1 if every source failed |
| `delta report <target>` | Gather, then build the cited report; prints the path; exit 1 on citation failure |
| `delta review-due [--as-of DATE]` | Lists decisions with reviews due (`decisions::due_reviews`); exit 0, or 2 if any are overdue so cron can alert |

All commands use the same config, `.env` and DB as the TUI, and log to stderr.

## Done

- [ ] Integration tests run the binary against a temp copy of `golden_seed.db` + FakeLLM + wiremock.
- [ ] `delta --version` launch benchmark unchanged.
- [ ] Release workflow still builds the `delta` binary.
