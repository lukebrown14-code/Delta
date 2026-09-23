# Delta rewrite plan: Rust + Ratatui

_Written 23 September 2026. Not started: begins after `docs/UI_UX_AUDIT.md` is complete._

## Context
Goal: make Delta beautiful and very performant. After investigating Go and Rust, **Rust + Ratatui** was chosen:
- Go + Bubble Tea is quicker to build and has prettier forms, but it redraws the whole view as a string each frame.
- Ratatui redraws only changed cells in under 1 ms and has native braille charts.

That matters for a charts-and-live-data trading TUI.

**Timing:** this starts **after the whole UI/UX audit (phases 0–3 of `docs/UI_UX_AUDIT.md`) has merged.** The post-audit Python app is the functional spec and the performance baseline. Its sections J and K, F8 and G9 are the UI spec for the port.

**Expected gains over the post-audit Python app** (estimates; R4 measures them):

| | Python post-audit | Rust + Ratatui |
|---|---|---|
| Launch to first frame | ~0.8–1.2 s (imports alone take 0.65 s today) | ~10–30 ms |
| Memory while running | ~120–200 MB (77 MB at import today) | ~8–20 MB |
| Full redraw | ~5–20 ms | <1 ms |
| Chart scrub (K9) or live quotes for 50+ tickers | Can lag | 60+ fps |
| Network-bound work (Yahoo, SEC, LLM) | — | Same speed |
| Install | uv plus a venv | Single binary |

**Known trade-offs we accept:**
- Losing Textual's CSS, focus and palette means we build our own.
- There is no official Anthropic or OpenAI Rust SDK.
- There is no yfinance, so we write our own Yahoo client.
- Estimated effort is about 3–5 months part-time.

## Getting started: new branch off main
Do this first, once the audit has merged into `main`:
```
git fetch origin
git worktree add ../Delta-rust -b rewrite/rust origin/main
```
- All rewrite work happens on `rewrite/rust` (in the `../Delta-rust` worktree) or on stream branches cut from it. **Never commit to `main` or the `audit/*` branches directly.**
- This plan lives on `main`, so `rewrite/rust` inherits it.
- Before starting each phase, merge the latest `main` into `rewrite/rust`, so any late Python fixes are carried into the spec.
- Cutover (R4) is one PR from `rewrite/rust` into `main`.

## Rule 1: the UI looks exactly the same
The rewrite changes the engine, not the look. Every screen at 80×24, 120×40 and 200×50 must match the post-audit Python app **cell for cell**: character, foreground, background and bold/italic/underline.

**The oracle (built in R0, before any screen work):**
- Add a golden-screen exporter to the Python suite: `tests/export_golden.py`, reusing the `snapshot_app` fixture from `tests/test_snapshots.py`.
- For every panel, modal and key state (for example the chart scrubbed, a dialog open, the narrow layout), it dumps the Textual screen buffer as JSON: rows × cells of `{ch, fg, bg, attrs}` with **resolved RGB**.
  - Textual blends `$text-muted`, `$panel` and similar tokens with alpha. The exporter captures the final colours, so Rust never has to guess them.
  - Output goes to `fixtures/golden_screens/<panel>/<state>@<w>x<h>.json`.
- The Rust side runs the same scenario on the same `fixtures/` DB and frozen clock, renders to ratatui's `TestBackend`, and diffs cell by cell.
  - Any mismatch fails the test.
  - The test also prints a side-by-side text diff so the agent can fix it.
- `Theme` is **generated** from the exporter's resolved token table, not typed by hand.

**Known hard spots.** Each gets a custom widget, not a stock crate:
- **Markdown (reports, chat):**
  - `tui-markdown` won't match Textual's layout.
  - Write a `DeltaMarkdown` renderer that copies Textual's heading, list, code-block and wrapping rules.
- **Text wrapping and Unicode width:**
  - Match Rich's word-wrap and cell-width rules. Use the `unicode-width` crate with the same emoji and CJK handling as Rich.
  - Covered by dedicated wrap-parity tests.
- **Scrollbars, DataTable cursor, zebra rows, Input cursor, OptionList highlight:** hand-drawn to match Textual's glyphs and colours.
- **Number, date and time formatting:**
  - Rounding: use the Python `format()` semantics, tested with a table of edge cases (0.005, negatives, -0.0).
  - Timezone: use the same UTC-naive SQLite convention.

**Deviations:**
- An exact match can be impossible in rare cases, for example terminal-specific glyphs.
- Log each one in `docs/rewrite/DEVIATIONS.md` with a screenshot pair.
- Each needs **your** approval before it's allowed. An agent can never approve its own deviation.

## Rule 2: review for bugs and improvements while porting
Yes, this is the best moment for it. Every line gets read and re-expressed, and Rust's compiler (exhaustive `match`, no `None` surprises, typed SQL) surfaces bugs Python hides. But finding and fixing are kept separate, so the port never silently changes behaviour.

**Finding:**
- Each porting agent logs findings to `docs/rewrite/findings/<stream>.md`. There is one file per stream, so parallel agents never conflict.
- Each finding records:
  - Category: `bug`, `perf`, `simplify` or `dead-code`.
  - The Python file and line.
  - The evidence (a failing test is preferred).
  - A proposed fix.

**Dedicated review agents:**
- At the end of R1, R2 and R3, a separate **reviewer agent** (one per crate or screen) adversarially compares the Rust code against the Python source.
- It looks for:
  - Error paths.
  - Empty and None cases.
  - Timezones.
  - Float rounding.
  - Sort stability.
  - Retries and cancellation.
  - Provenance and citation checks.
- It adds to the same findings files.

**Fixing:**
| Finding type | Where it gets fixed |
|---|---|
| A bug that changes visible output or stored data | Fixed **in Python on `main` first** (a small PR with a test). The goldens are regenerated, `main` is merged into `rewrite/rust`, then Rust follows. The oracle stays the single source of truth |
| A bug in behaviour you can't see (crash, leak, race, retry) | Fixed directly in Rust, with a test. Noted in the findings file |
| Performance or simplification | Rust only, as long as parity tests stay green |
| Anything touching provenance, citation or prompts | Never changed silently. Log it and wait for your call |

- The findings files are triaged by you at each phase gate. Unfixed items roll into the next phase or a post-cutover backlog.

## Rule 3: all work is done by AI agents
The plan is written so no step needs a human except the approval gates.

**Task cards:**
- Every workstream gets a card in `docs/rewrite/tasks/<stream>.md` with:
  - The branch name.
  - Owned paths.
  - The Python sources to port.
  - The golden scenarios that must pass.
  - The findings file.
  - The done checklist.
- An agent reads only its own card, `AGENTS.md` and this plan.

**Hard gates, checked by machine rather than judgement:**
- `cargo fmt --check`, then clippy with `-D warnings`, then `cargo nextest run`.
- The golden-screen diff: zero mismatches or approved deviations.
- Service parity against `fixtures/`.
- The Python suite stays green.

**Every PR:**
1. The porting agent opens it.
2. A separate reviewer agent reviews it (it doesn't share the author's context) and must return pass or fail with reasons.
3. Merges into `rewrite/rust` happen only on pass plus green CI.

**Orchestration:**
- Phases run as workflows: one agent per stream in a worktree (`isolation: "worktree"`).
- Each agent reports changed files, test output and new findings.
- There is a stop-and-ask trigger. An agent halts and reports, rather than improvising, if it:
  - needs a file it doesn't own,
  - hits a deviation it can't avoid, or
  - has a finding in the "wait for your call" row.

**Your gates** (the only human steps):
1. The spike go/no-go.
2. Findings triage and deviation approval at the end of each phase.
3. Cutover.

## Architecture

**Repo layout:**
- The work lives in the same repo on the long-lived branch `rewrite/rust` (see above).
- A Cargo workspace sits at the root next to the Python code until cutover.
- `data/delta.db`, `config.toml` and `.env` stay **format-compatible**, so users keep their history and the rewrite can be checked against real data.

```
crates/
  delta-core/      config (serde + toml_edit, keeps comments), models, IDs, sqlx SQLite + migrations, events
  delta-plugins/   traits Market / DataSource / Target; yahoo, sec, asx, rss, jev; static registry (replaces entry points)
  delta-llm/       reqwest client, cache, cost log, retries, streaming, prompts (minijinja, include_str!), citation validator
  delta-services/  ingest, data_health, reports, chat, theses, decisions, review, brief, calibration, changes
  delta-tui/       app loop, theme, components, widgets, screens
  delta/           binary: clap CLI (TUI by default; `gather`, `report`, `review-due` headless, G8)
fixtures/          shared golden data: seeded DB, FakeLLM responses, HTTP cassettes. Used by both implementations during the port
```

**TUI pattern** (the Ratatui "component" template):
- A `Component` trait with `handle_key → Option<Action>`, `update(Action)` and `draw(Frame, Rect)`.
- A single `tokio::mpsc<Action>` bus. Background tasks (ingest, quotes, LLM) send actions, and the render loop draws on events or a ~60 Hz tick while anything animates.
- The event loop never blocks. This is the rule the audit enforced with B1 and B4, now structural.

**Crates:**

| Need | Crate |
|---|---|
| TUI and terminal | `ratatui`, `crossterm` (event-stream) |
| Async | `tokio` |
| Errors | `color-eyre` |
| Charts | Built-in `Chart`, `Canvas` (braille), `Sparkline`; custom `PriceChart` widget for K1–K11 |
| Inputs | `tui-textarea` (forms, chat), own `SuggestionList` / autocomplete |
| Markdown | Custom `DeltaMarkdown` (a `pulldown-cmark` parser plus Textual-matching layout) |
| Polish | `tachyonfx` (transitions, loading shimmer), `throbber-widgets-tui`, `tui-big-text` |
| Fuzzy search | `nucleo` (ctrl+k, palette, `:` completion) |
| DB | `sqlx` (sqlite, compile-time checked queries, WAL pragmas) |
| HTTP, websocket | `reqwest` + rustls; `tokio-tungstenite` + `prost` (Yahoo quote stream) |
| RSS | `feed-rs` |
| Config and secrets | `serde`, `toml_edit`, `dotenvy` |
| Dates | `jiff` or `chrono` |
| Clipboard | OSC52 escape (E6), `arboard` fallback |
| Tests | `insta` + ratatui `TestBackend` (snapshots at 80×24, 120×40, 200×50), `wiremock` (HTTP), trait-object `FakeLlm` |

**Theme:**
- One `Theme` struct generated from Textual's resolved `delta-dark` RGB values (dark-only), plus the colour-blind variant if I1 shipped.
- Widgets take styles only from `Theme`. There are no inline colours; this is the token-only rule carried over.

**Contracts that must be preserved exactly:**
- Evidence ID format.
- Citation validation: claims and answers reference gathered evidence IDs only, and an invalid result fails.
- Prompt template text. Files are copied verbatim from `delta/llm/prompts/` with the same versions.
- Cost logging.

## Phases and parallel workstreams
The rules are the same as the audit:
- One branch per stream, `rewrite/r<N>-<slug>`, off `rewrite/rust`.
- Exclusive file ownership.
- Each stream opens its own PR.
- Checks: `cargo fmt --check`, then `cargo clippy --all-targets -- -D warnings`, then `cargo nextest run`, and the Python suite stays green.

| Phase | Stream | Owns | Done when |
|---|---|---|---|
| **R0** serial | Skeleton and oracle | Workspace, CI, `tests/export_golden.py` + `fixtures/golden_screens/`, generated `Theme`, the golden-diff harness, `delta-tui` app loop, `Pane`, status bar, `-narrow` breakpoint, task cards, `AGENTS.md` Rust section | The goldens are exported. An empty 7-panel app navigates 1–6/c, and the status bar row matches its golden exactly |
| **R1** parallel | R1a Core and DB | `delta-core` | Opens a Python-created `delta.db` and round-trips every table (parity test on `fixtures/`) |
| | R1b Plugins | `delta-plugins` | Yahoo chart, quote and quoteSummary (with cookie/crumb), websocket quotes, SEC with 429 retry, ASX, RSS. Tests run through wiremock cassettes |
| | R1c LLM | `delta-llm` | Cache, cost log, streaming, JSON repair, citation validator. The Python golden tests are ported and pass |
| | R1d Widgets | `delta-tui/widgets`, `components` | `PriceChart` (connected line, nice Y ticks, markers, benchmark, scrub), `BrailleGraph`, table, Dialog/modal stack, `EmptyState`, `SectionHeading`, autocomplete, command palette, which-key |
| **R2** serial | Services | `delta-services` | Every `services.py` operation is ported. Output matches Python on the fixture DB |
| **R3** parallel | One stream per screen | `screens/{home,watchlist,research,theses,ask,decisions,settings}.rs` | Zero golden-screen mismatches at all 3 sizes and in every key state. Same bindings as Python |
| **R4** serial | Parity and cutover | `bench/`, packaging | Parity checklist signed off, benchmarks recorded, `cargo-dist` binaries and a Homebrew tap. The Python app is tagged `python-final` and removed from `main` |

**De-risking spike (first week of R0):**
- Build only the Watchlist inspector (header, range tabs, chart, scrub) against a copy of `data/delta.db`.
- Compare it side by side with Python for looks, launch time, RSS and scrub smoothness.
- Go/no-go for the rest.

**Main risks and mitigations:**
- **Yahoo breakage without yfinance:** the client is isolated behind the `DataSource` trait, pinned by cassettes and has a contract test run manually.
- **Provenance drift:** the citation validator and ID tests are ported first (R1c) and run against shared fixtures.
- **Losing Textual conveniences** (focus, palette, modals): these are built once in R1d, before any screen work starts.

## Verification
- **Per stream:**
  - fmt, clippy and nextest pass.
  - insta snapshots are reviewed with `cargo insta review`.
  - Offline only: wiremock and `FakeLlm`, with no live calls.
- **Parity:**
  - `delta-services` output is diffed against Python on `fixtures/`.
  - Every screen and state is diffed cell by cell against `fixtures/golden_screens/`, with zero mismatches beyond approved `DEVIATIONS.md` entries.
- **Review:**
  - Reviewer-agent pass on every PR.
  - End-of-phase adversarial review.
  - Findings files triaged at each gate.
- **Performance (R4):**
  - `hyperfine 'delta --version'` and launch to first frame.
  - RSS measured with `/usr/bin/time -l`.
  - Frame time logged while holding `]` on the chart, and with 50 live tickers.
  - Everything is compared with the post-audit Python baseline recorded at the start of R0.
- **Manual:** run `cargo run --release` against a copy of the real `data/delta.db` and go through every panel.
