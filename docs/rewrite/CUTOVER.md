# Cutover checklist (R4 -> main)

The rewrite branch (`rewrite/rust`) is feature-complete through the golden
matrix and the live app. Cutover is one PR from `rewrite/rust` into `main`
(per `docs/RUST_REWRITE_PLAN.md`); the human steps are marked **[gate]**.

- [x] Full 3-size golden matrix green (Tier A cell-for-cell, zero open
      deviations; glossary Tier B with exact text).
- [x] Service parity: the offline desk feeds the same states the exporter
      captured; real config/DB loading paths ported (`Desk::open`).
- [x] Benchmarks recorded (`docs/rewrite/BENCHMARKS.md`): launch < 2 ms,
      RSS ~6 MB, full-grid repaint 1.67 ms (release, 120x40).
- [x] Packaging: cargo-dist config in `Cargo.toml` +
      `.github/workflows/release.yml` (tag-push builds).
- [ ] **[gate]** Approve the PR merging `rewrite/rust` into `main`.
- [ ] **[gate]** Tag `python-final` on `main` before the merge lands
      (the tag marks the last all-Python commit).
- [ ] Remove the Python package from `main` in the cutover PR: `delta/`,
      `tests/`, `uv.lock`, `pyproject.toml` (keep `fixtures/golden_screens/`
      and `docs/`), and update CI to the Rust gates
      (fmt, clippy `-D warnings`, nextest, golden matrix).
- [ ] Homebrew tap: create the `homebrew-tap` repo, then add the tap
      publish step to the release workflow (cargo-dist prints the formula).

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
