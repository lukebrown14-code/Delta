# Cutover checklist (R4 -> main)

The Rust application and service ports are implemented on `rewrite/rust`.
Cutover is one PR from `rewrite/rust` into `main` (per
`docs/RUST_REWRITE_PLAN.md`); the remaining validation and human steps are
marked below.

- [x] Static painter 3-size golden matrix green with full cell attributes;
      glossary Tier B with exact text.
- [x] Rust config/DB loading and services ported; Python DB compatibility is
      exercised by `crates/delta-core/tests/parity.rs`.
- [x] Populated Settings matches the Python oracle cell-by-cell at 80x24,
      120x40 and 200x50.
- [x] README and CI now use the Rust binary and Rust validation gates.
- [ ] Compare populated Research, Theses, Ask and Decisions, plus live dialogs
      and focus states, against the Python oracle at the 3 supported sizes.
- [ ] Re-run and record reproducible launch, RSS and in-app frame benchmarks.
- [ ] Validate cargo-dist release archives. `cargo-dist` is not installed in
      the current environment.
- [ ] **[gate]** Approve the PR merging `rewrite/rust` into `main`.
- [ ] **[gate]** Tag `python-final` on `main` before the merge lands
      (the tag marks the last all-Python commit).
- [ ] Remove the Python package from `main` in the cutover PR: `delta/`,
      `tests/`, `uv.lock`, `pyproject.toml` (keep `fixtures/golden_screens/`
      and `docs/`), and update CI to the Rust gates
      (fmt, clippy `-D warnings`, nextest, golden matrix).
- [ ] Homebrew tap: create/configure the tap repository and add its publish
      step to the release workflow.

## Keys / environment (the live app)

| Input | Action |
|---|---|
| 1-6 | Home, Watchlist, Research, Theses, Ask, Decisions |
| c | Settings |
| g / esc | toggle / close the glossary (watchlist) |
| h, l or arrows | cycle the inspector range |
| , / . | cycle the watched instrument |
| U | run ingest over the universe |
| q, Ctrl-c | quit (prints frame stats) |

`DELTA_QUOTES=1` enables the streaming Yahoo quote worker;
`DELTA_FRAME_LOG=path` writes the frame-time report on quit.
