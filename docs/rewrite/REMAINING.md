# What's left on the Rust rewrite

_Reviewed 4 October 2026 against `rewrite/rust` @ `ff1033a`. Supersedes the
"feature-complete" claim previously in `CUTOVER.md`._

## Goal

Delta is **completely written in Rust**. Cutover happens only when every
Python workflow works in the Rust app. No feature is ported "after cutover".

## Where it actually stands

Healthy: `cargo fmt --check`, clippy `-D warnings` and 186 tests across 25 suites pass.
The 28 golden scenarios (10 states × 3 sizes) are Tier A green.

| Area | State |
|---|---|
| `delta-core`, `delta-llm`, `delta-plugins` | Ported. Open findings below |
| `delta-services` | Read side ported (analytics, ingest/extract/gather, sentiment, brief, thesis health/fleet, config ops). **Missing**: reports, chat, theses writes, decisions, review, thesis summary, setup checks, data-provider status/configure, Yahoo calendar |
| Home, Watchlist | Live data, but painted by hand at fixed coordinates. Watchlist lacks add/remove target, symbol search, scrub and benchmark |
| Research, Theses, Ask, Decisions, Settings | **Static pictures of the empty state.** `draw_research(screen)` takes no data. They pass the goldens only because every golden for them is the empty landing screen |
| App shell | Only 1–6/c, glossary, range, `,`/`.`, `U` and `q`. No palette, help, Go, model or provider pickers, forms, text input, markdown or light theme. Bindings differ from Python (see findings) |
| Oracle | 10 landing states only. No populated, dialog-open, scrubbed, report or chat states |
| CI | `rewrite/rust` CI runs the Python suite only. **No Rust gates** |
| Process | No task cards, PR reviewers or findings triage. No spike go/no-go recorded |

## Decisions (4 Oct 2026)

| # | Decision |
|---|---|
| D1 | Done = full functional parity, completely in Rust |
| D2 | Rebuild every screen as a data-driven `Component` (ratatui layout, no fixed coordinates). The hand-placed painters in `screens.rs` are retired screen by screen; the existing goldens stay as the regression gate |
| D3 | Oracle covers populated and key states per screen: populated, empty, each dialog/form open, error. Each screen's stream exports its own scenarios first, on the shared seed |
| D4 | Enforce the plan's Rule 3: Rust CI gates first, a task card per stream, a branch and PR per stream, an independent reviewer agent pass before merge |
| D5 | Add `chrono-tz`; US market = `America/New_York`, ASX = `Australia/Sydney` |
| D6 | `config.toml` writes go through `toml_edit` and preserve comments (an improvement over Python) |
| D7 | LLM calls retry 429/5xx/timeouts, up to 5 attempts, exponential backoff plus jitter, honouring `Retry-After` |
| D8 | Extract validates sentiment in [-1, 1] and skips the whole batch on failure, as pydantic does |
| D9 | Python feature freeze: bug fixes only (each with a regenerated golden, merged into `rewrite/rust`) |
| D10 | Screens are ported in parallel, as vertical slices: a stream owns its screen module and its screen-only services. Services several screens share are ported first (R3.1), so parallel streams never block each other |
| D11 | The headless CLI (`delta gather`, `delta report`, `delta review-due`, audit G8) is in scope now. It's new; Python never had it |
| D12 | Distribution: cargo-dist GitHub release binaries only. No Homebrew tap |
| D13 | Approved crates: `chrono-tz`, `toml_edit`, `tui-textarea`, `pulldown-cmark`, `nucleo`, `clap` (`unicode-width` is already in). Anything else: stop and ask |

## Findings triage

"Default" rows are accepted unless you say otherwise at the next gate.

| Finding | Outcome | Stream |
|---|---|---|
| core #1 sync event bus | Default: accept | — |
| core #2 row-by-row insert in one tx | Default: accept | — |
| core #3 config comments | D6 | R3.1c |
| core: `rusqlite` instead of `sqlx` | Default: accept as a plan deviation | — |
| llm: no retry policy | D7 | R3.1c |
| llm #1 retry prompt has its own cache key | Default: keep parity | — |
| plugins #3 partial HTML entity table | Default: accept; revisit if a feed breaks | — |
| plugins #4 SEC strict zip | Default: keep parity (loud failure) | — |
| plugins #5 `yfinance_calendar` not ported | Port | R3.1b |
| plugins #6 fixed-offset timezones (ASX too: no AEDT) | D5 | R3.1c |
| services #3 theses write path | Port | R3.1b |
| services #4 data-provider status / configure / setup checks | Port | R3.1b |
| services #5 sentiment range | D8 | R3.1c |
| widgets #3 chart scrub + benchmark | Port | R3.2 home-watchlist |
| widgets #4 table scrolling | Port | R3.1a |
| widgets #6 chips, dots, pills, pane rows, key grid | Port | R3.1a |
| screens #8 hardcoded painters | D2 | all R3.2 |
| screens #9 bindings differ from Python | Fix to match Python | R3.1a (app), R3.2 (per screen) |
| screens #10 light theme missing | Port | R3.1a |
| screens #11 quotes stream is opt-in (Python streams by default) | Match Python | R3.2 home-watchlist, research |

## Phases and streams

Branches cut from `rewrite/rust`; task cards in `docs/rewrite/tasks/`.

| Phase | Stream | Card | Runs |
|---|---|---|---|
| **R3.0** | Gates, file split, shared seed | `r3-gates.md` | serial, first |
| **R3.1** | TUI foundations | `r3-tui-foundations.md` | parallel |
| | Shared services | `r3-shared-services.md` | parallel |
| | Findings fixes | `r3-fixes.md` | parallel |
| **R3.2** | Home + Watchlist | `r3-home-watchlist.md` | parallel |
| | Research + reports | `r3-research.md` | parallel |
| | Ask + chat | `r3-ask.md` | parallel |
| | Theses | `r3-theses.md` | parallel |
| | Decisions | `r3-decisions.md` | parallel |
| | Settings + pickers | `r3-settings.md` | parallel |
| | Headless CLI | `r3-cli.md` | parallel; `report` waits on research's `reports` service |
| **R4** | Parity, review, cutover | `r4-cutover.md` | serial, last |

R3.2 runs 7 porting agents plus 7 reviewer agents. That's above the default workflow
size, so raise it in `/config` or run it in two waves.

## Your gates

1. End of R3.1: triage new findings.
2. End of R3.2: triage findings, approve any deviations.
3. R4: approve the cutover PR.
