# Cutover checklist (R4 -> main)

**Not ready.** The Rust app and shared services are integrated on
`spec/26-rust-v1` in draft PR #37. The expanded populated golden matrix,
release packaging, and maintainer gates remain. The cutover is one PR into
`main`; the human steps are marked **[gate]**.

- [ ] Every R3 task card in `docs/rewrite/tasks/` is done and merged.
- [ ] Full 3-size golden matrix green over every scenario, including populated
      and dialog states (Tier A cell-for-cell, Tier B triaged, approved
      deviations only). The integrated landing and initial populated states
      are green; extended R4 scenarios are in progress.
- [ ] Service parity: every `delta/services.py` operation plus reports, chat,
      theses, decisions and review match Python on the shared seed.
- [ ] End-of-phase adversarial reviewer pass, findings triaged.
- [ ] Benchmarks re-recorded on the populated app (`docs/rewrite/BENCHMARKS.md`):
      launch, RSS, chart scrub, 50 live tickers. Local smoke measurements are
      recorded; a repeatable first-frame run remains.
- [ ] Packaging: validate cargo-dist archives and publish a GitHub Release
      from the tag workflow. Release binaries only; no Homebrew tap (D12).
- [ ] Manual pass: `cargo run --release` on a copy of the real `data/delta.db`,
      every panel. A seed database pass succeeded; repository instructions
      currently forbid reading `data/*.db`.
- [ ] **[gate]** Tag `python-final` on `main` before the merge lands.
- [ ] **[gate]** Approve the PR merging `rewrite/rust` into `main`.
- [ ] In the cutover PR: remove `delta/`, `tests/`, `uv.lock`, `pyproject.toml`
      (keep `fixtures/` and `docs/`); CI runs the Rust gates only.

## Keys / environment (current live app)

These differ from Python and are being fixed to match it (findings
rust-screens #9).

| Input | Action |
|---|---|
| 1-6 | Home, Watchlist, Research, Theses, Ask, Decisions |
| c | Settings |
| g / esc | toggle / close the glossary (watchlist); Python uses `i` |
| h, l or arrows | cycle the inspector range; Python uses `r` |
| , / . | cycle the watched instrument |
| U | run ingest over the universe |
| q, Ctrl-c | quit (prints frame stats) |

`DELTA_QUOTES=1` enables the streaming Yahoo quote worker (Python streams by
default; findings rust-screens #11); `DELTA_FRAME_LOG=path` writes the
frame-time report on quit.
