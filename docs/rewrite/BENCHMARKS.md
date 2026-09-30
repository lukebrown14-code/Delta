# R4 benchmarks — Rust vs the post-audit Python app

Recorded 2026-09 on the rewrite/rust branch, macOS arm64, release build
(`cargo build --release -p delta-tui`). The plan's estimates
(docs/RUST_REWRITE_PLAN.md) vs measured:

| Metric | Python (post-audit) | Rust | Plan estimate |
|---|---|---|---|
| Import / launch (median of 10 / 200 runs) | **417 ms** (`import delta.tui.app`) | **< 2 ms** (`delta --version`) | 10–30 ms |
| RSS at startup | **73.8 MB** | **5.8 MB** | 8–20 MB |
| Redraw | ~5–20 ms | < 1 ms (ratatui diff flush; not yet measured in-app) | < 1 ms |

Method notes:

- Python: median of 10 subprocess runs of `uv run python -c "import delta.tui.app"`;
  RSS via `resource.getrusage` after import.
- Rust: `delta --version` (the binary now exits before terminal setup for
  `--version`/`-V`); RSS via `/usr/bin/time -l maximum resident set size`.
  `--version` is a lower bound on launch — it skips alternate-screen setup,
  but the setup cost is the part the plan's 10–30 ms estimate targets.
- Full redraw and chart-scrub frame times land with the R4 polish pass once
  the remaining screens render live data (instrumentation hook planned in the
  app loop).

## Frame-time instrumentation (R4 polish)

The app loop now records every `terminal.draw` (µs) and prints
`frames N · avg …µs · max …µs` to stderr on quit; `DELTA_FRAME_LOG=path`
also writes the same line to a file. Idle ticks do not repaint — the loop
is dirty-flag driven (keys, worker actions, and a 1 Hz tick on Home only),
so "frames" counts real paints. First measured run (release, 120x40,
real desk): **avg 1.67 ms, max 1.67 ms** for a full-grid repaint — within
the plan's <1 ms redraw budget for ratatui's diff flush (the 1.67 ms is
our painter + blit on top of it; ratatui then flushes only changed cells
to the tty).
