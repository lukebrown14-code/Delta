# Cutover checklist (R4 -> main)

**Not ready.** Five screens are still static and several services are unported;
see `docs/rewrite/REMAINING.md`. Cutover is one PR from `rewrite/rust` into
`main` (per `docs/RUST_REWRITE_PLAN.md`); the human steps are marked **[gate]**.

- [ ] Every R3 task card in `docs/rewrite/tasks/` is done and merged.
- [ ] Full 3-size golden matrix green over every scenario, including populated
      and dialog states (Tier A cell-for-cell, Tier B triaged, approved
      deviations only). The 28 landing-state goldens are green today.
- [ ] Service parity: every `delta/services.py` operation plus reports, chat,
      theses, decisions and review match Python on the shared seed.
- [ ] End-of-phase adversarial reviewer pass, findings triaged.
- [ ] Benchmarks re-recorded on the populated app (`docs/rewrite/BENCHMARKS.md`):
      launch, RSS, chart scrub, 50 live tickers. Launch (< 2 ms) and RSS
      (~6 MB) are recorded for the current partial app.
- [x] Packaging: cargo-dist config in `Cargo.toml` +
      `.github/workflows/release.yml` (tag-push builds). Release binaries only;
      no Homebrew tap (decision D12).
- [ ] Manual pass: `cargo run --release` on a copy of the real `data/delta.db`,
      every panel.
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
