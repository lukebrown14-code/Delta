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

- [x] Integration tests run the binary against a temp copy of `golden_seed.db` + FakeLLM + wiremock.
- [x] `delta --version` launch benchmark unchanged.
- [x] Release workflow still builds the `delta` binary.

## Notes (implementation, Oct 2026)

- Per the spec #26 sequencing note, the subcommands were rebuilt on the base
  binary instead of a new `crates/delta/`: `delta-tui/src/main.rs` dispatches
  to `delta-tui/src/cli.rs` before the TUI launches, so no new crate or `clap`
  dependency was needed and the release/dist target is unchanged.
- `report` required the reports service early: `delta-services/src/reports.rs`
  + `schemas.rs` are faithful donor ports (adapted to base's `evidence`
  signature and `sentiment_in_range`), with the donor's service tests; the
  r3-research stream (#30) reviews and owns them from here.
- Gather exit codes: base `ingest` is fail-fast per Python parity (any hard
  source failure exits 1), and the offline-configurable sources skip failing
  feeds by design — the card's "exit 1 if every source failed" is therefore
  the stronger "exit 1 if the ingest stage failed".
