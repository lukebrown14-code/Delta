# Subagents — split of agent/PLAN.md (Python→Rust gap review)

Base branch: `rewrite/rust` (commit `9bce196`). Worktrees at `../Delta-<slug>`.

| slug | branch | worktree | scope | todo |
|---|---|---|---|---|
| services-foundation | `agent/services-foundation` | `../Delta-services-foundation` | `crates/delta-services/**`, `crates/delta-llm/**` (+tests) | sentiment, brief, thesis_health, catalog, jev |
| tui-live-home | `agent/tui-live-home` | `../Delta-tui-live-home` | `crates/delta-tui/**` (+tests) | feed analytics into Home desk state/painters |

## Later (not started — no agents)
- **report-chat-flows** — waits on services-foundation (touches `delta-services` too): `reports.build_report`, `chat.chat()` + web-search tool, citation checks; then wire into TUI workers.
- **theses-decisions-flows** — waits on services-foundation: thesis CRUD/summary, decisions journal + review.
- **tui-interactivity** — waits on tui-live-home (touches `delta-tui` too): selection/scroll/filter/input primitives; then targets/research/theses/ask/decisions/settings screens, help screen.
- **calendar-plugin** — waits on user answer (port `yfinance_calendar.py` vs log as finding).

## Merge order (least risky first)
1. `services-foundation` (pure additions, no TUI)
2. `tui-live-home` (TUI-only, golden-gated)
After each: run `cargo fmt --all && cargo clippy --all-targets -- -D warnings && cargo test --workspace` on `rewrite/rust`, then `git worktree remove ../Delta-<slug>` + `git branch -d agent/<slug>`.
