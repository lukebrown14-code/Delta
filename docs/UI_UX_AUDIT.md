# Delta UI/UX, performance and simplification audit

_Written 23 September 2026 against `feat/decision-review`. Replaces `TUI_REDESIGN_PLAN.md` and `TUI_STYLE_PLAN.md`._

## Context
This is a full audit of Delta, a keyboard-first Textual TUI that helps hobby traders and investors make better decisions. It is based on read-only exploration of `delta/tui` (9.1k lines), `services`/`core`, plugins and LLM.

The output is a menu of ideas, each with an ID. Pick the IDs you want implemented. The ideas are grouped from "clear wins" to "outside the box". Effort is marked **S**, **M** or **L**.

**How to use this document:** tick the IDs you want implemented (for example in the PR that picks up a workstream). Each workstream below runs as its own agent on its own branch off `main`.

⚠️ **Base-branch caveat:** `main` is 33 commits behind `feat/decision-review`, and the Decisions screen and domain only exist on that branch. File and line references in this audit are from `feat/decision-review`. Either merge `feat/decision-review` into `main` first, or WS6 (Decisions parts) and WS8 wait until it has merged.

---

## How to run this in parallel

**Rules for every agent**
- Work in its own git worktree with a **new branch off the latest `main`** (Agent `isolation: "worktree"`), one branch per workstream: `audit/ws<N>-<slug>`. Never commit to `main` or `feat/decision-review` directly. Each stream opens its own PR into `main`.
- Only edit files the workstream **owns**. If it needs a change in someone else's file, write a note in its PR description instead of editing the file.
- Follow `AGENTS.md`:
  - Tests are offline (respx, FakeLLM, `tmp_path` DBs).
  - Keep provenance and citation validation intact.
  - Run checks in this order: `uv run ruff check .`, then `uv run mypy --strict delta/core delta/llm`, then `uv run pytest`.
- Implement only the ticked IDs, then commit and report: what changed, test output, and anything left for another stream.

**Phases** (a phase starts once the previous phase has merged)

| Phase | Mode | Why |
|---|---|---|
| 0 Foundations | 1 agent, serial | Shared widgets, helpers and snapshot baselines that every other stream builds on |
| 1 Core streams | **WS1–WS6 in parallel** | No two streams own the same files |
| 2 Key grammar and navigation | 1 agent, serial | Touches `BINDINGS` on every screen, so it must run alone |
| 3 Features | **WS8–WS10 in parallel** | New modules, with small hooks into screens |

**Phase 0 status: done** (branch `audit/ws0-foundations`). What other streams can now use:
- `delta/tui/components.py`:
  - `QuoteFeedMixin` (`sync_quotes` / `quote_for` / `stop_quotes`)
  - `SuggestionList`
  - `EmptyState`
  - `SectionHeading`
  - `require_selection`
  - `goto`
  - `thesis_from_citations`
  - `quote_suffixes`
- `DeltaScreen.narrow` and `apply_breakpoint()` with `shell.NARROW_WIDTH`, replacing the per-screen constants.
- `services.total_spend()`; `llm_costs` now aggregates in SQL.
- 21 layout snapshots (7 panels at 80×24, 120×40 and 200×50), with frozen time and no network. Accept intended layout changes with `uv run pytest tests/test_snapshots.py --snapshot-update`, then review the SVG diffs.

**Workstreams, their files and their IDs**

| WS | Phase | Owns (exclusive) | IDs |
|---|---|---|---|
| **WS0 Foundations** ✅ | 0 | `tests/test_snapshots.py` + `tests/__snapshots__/` (new), `delta/tui/components.py` (new: `Autocomplete`, `EmptyState`, `SectionHeading`, `QuoteFeedMixin`, `require_selection`, `goto`), `services.total_spend`, the breakpoint class in `shell.py` | J16, C1, C2, C3, D7, J6, J9, B8 |
| **WS1 Chart and inspector** | 1 | `widgets.py` (`BrailleGraph`, `PriceChart` only), `axes.py`, `screens/targets.py`, `asset_metrics.py`, `tests/test_chart*.py`, `tests/test_asset_metrics.py` | K1–K13, J7, J8, B12, C10, J10 (targets part) |
| **WS2 Data and performance** | 1 | `core/db.py`, `plugins/**`, `evidence.py`, `review.py`, `brief.py`, `sentiment.py`, `extract.py`, and in `services.py` the ingest and `data_health` functions | A3, A4, A5, B1, B2, B3, B5, B6, B9, B10, B11, B13, B14, B15 |
| **WS3 LLM layer** | 1 | `delta/llm/**`, `chat.py` (non-TUI), `reports.py` | A7, C5, H3, H4 |
| **WS4 Home** | 1 | `screens/home.py` | A1, J1, J3, B4 (home), J10 (home part) |
| **WS5 Research and Chat screens** | 1 | `screens/research.py`, `screens/chat.py`, `screens/data.py`, `screens/reports.py` | A2, J4 (screen side), J12, B4 (research), C9 (research) |
| **WS6 Decisions, Theses and Config screens** | 1 | `screens/decisions.py`, `screens/theses.py`, `screens/config.py`, `source_setup.py`, `market_setup.py` | D6, J13, J14, C4, B4 (the rest), C9 (theses) |
| **WS7 Shell and keys** | 2 | `app.py`, `shell.py`, `help.py`, the `BINDINGS` of every screen | D1, D2, D3, D4, D5, J2, J4 (global `z`), J5, J11, B7, E1–E8, J15, C12, I1, I2, I4 |
| **WS8 Decision-quality features** | 3 | `decisions.py` (domain), new `delta/calibration.py`, a new prompt `premortem_v1` | F1–F9, F11 (F10 left out: it conflicts with the product's stated philosophy) |
| **WS9 Research features** | 3 | new `delta/changes.py` and `delta/insiders.py`, a report-diff view, `cli.py`, a demo fixture DB | G1–G10, I3 |
| **WS10 LLM UX** | 3 | streaming paths in `llm/` and the chat and report views | H1, H2 |

Leftover cross-cutting simplification (C6, C7, C8, C11, A6) goes to the **WS2** agent for `services.py`, `runtime.py`, `config.py` and dead code, since it is already in those files.

**Kick-off prompt per agent** (fill in the WS number):
> Create branch `audit/ws<N>-<slug>` from the latest `main` (`git fetch && git switch -c audit/ws<N>-<slug> origin/main`). You are implementing workstream WS&lt;N&gt; from `docs/UI_UX_AUDIT.md`. Read that file, `AGENTS.md` and `PROJECT_SPEC.md`. Implement only the ticked IDs listed for WS&lt;N&gt;, editing only the files WS&lt;N&gt; owns. Add or adjust offline tests for each ID. Run ruff, then mypy, then pytest, and report changed files, test results and any cross-stream follow-ups.

**Merging:** within a phase, merge the smallest diff first. Then rebase the other streams and re-run `uv run pytest`, and update the snapshot baselines (J16) at the end of each phase.

---

## A. Bugs found during the audit (fix regardless)
- **A1 (S)** Wrong on-screen hints:
  - `home.py:763`: "1 builds the watchlist". Watchlist is on 2.
  - `home.py:874`: "press 2, then U". U is on Research (3).
  - `home.py:892`: "a tracks a claim". The key is `n`.
  - `home.py:516`: `g` is labelled "palette". The palette is ctrl+p.
  - The review line says "2 evidence". Evidence is on 3.
- **A2 (S)** The chat `w` web toggle does nothing, because `OfflineSearchTool` is the only implementation (`chat.py:84`). Either implement it or hide the toggle.
- **A3 (S)** Jev sentiment (`services.classify_sentiment`, `services.py:220`) is never triggered by Gather or Research refresh, so the sentiment column stays empty.
- **A4 (S)** Brief evidence IDs don't match the evidence namespace (`brief.py:97,132,172`). This is a provenance risk.
- **A5 (S)** The two falsifier matchers disagree: `review._matches` checks title and body, `thesis_health._falsifier_hit` checks kind and title.
- **A6 (S)** Config writes hardcode `Path("config.toml")` and ignore `CONFIG_PATH`. This happens 6 times in `services.py`.
- **A7 (S)** Invalid LLM JSON is cached permanently, so a bad cached answer repeats the same retry every time.

## B. Performance
- **B1 (S)** Move yfinance `ticker.history` onto `asyncio.to_thread` and replace `iterrows()`. Today the UI freezes during every bar download (`plugins/data/yfinance.py:59-80`).
- **B2 (S)** Incremental bar ingest: fetch from each instrument's last stored bar instead of re-pulling 365 days (`services.py:167`).
- **B3 (S)** SQLite pragmas in `db.py:128`: WAL, `synchronous=NORMAL`, `busy_timeout`.
- **B4 (M)** Move screen refreshes (Home, Theses, Decisions, Research load, Config) off the event loop and into one `@work(thread=True)` pattern. Show a shared loading shimmer while they run.
- **B5 (M)** `review_queue` hotspot (`review.py:148-165`):
  - It loads `evidence(limit=10_000)` twice per instrument.
  - It rebuilds `universe()` inside the loop.
  - Fix with a single pass and a memoised universe.
- **B6 (S)** `data_health`: replace the per-instrument `ORDER BY` with one `GROUP BY MAX(ts)`. The status bar can then use `SELECT MAX(ts)`.
- **B7 (S)** Status bar: run one app-level timer instead of one per screen (7 today), and pause it while suspended.
- **B8 (S)** `llm_costs`: aggregate with SQL instead of loading cached response blobs. Add one `total_spend()` helper to replace the 5 copies.
- **B9 (M)** Add a `news_instrument` join table to replace `LIKE '%"id"%'` over JSON (`evidence.py:172`). This speeds up chat, reports, review and theses.
- **B10 (S)** Stop calling `_ensure_tables` (`metadata.create_all`) on every theses/decisions call. Run it once at init.
- **B11 (S)** Run independent ingest plugins concurrently with `asyncio.gather` and a semaphore.
- **B12 (S)** Short in-memory TTL cache for `fetch_asset_metrics`, so reopening the inspector doesn't make 3–5 Yahoo calls.
- **B13 (S)** Batch commits for sentiment, `add_evidence` and LLM logging.
- **B14 (S)** Add retries and 429 handling to SEC and Jev, and a concurrency cap to ASX and RSS. Today one SEC 404 aborts the whole fetch.
- **B15 (S)** Lazy plugin imports, and discover plugins once at startup instead of on every `reload_markets`.

## C. Simplification
- **C1 (M)** Extract a `QuoteFeedMixin`. The feed start/stop/suspend code is copied in Home, Targets and Research.
- **C2 (M)** One `Autocomplete` widget to replace the three Input + OptionList + `on_key` copies (targets, market_setup, model_picker). Reuse it for the Decision instrument field.
- **C3 (S)** Shared helpers:
  - `require_selection()` guard.
  - `goto(screen)`.
  - `NARROW_WIDTH` responsive mixin.
  - `thesis_from_citations()` (duplicated in chat and research).
- **C4 (S)** Make `SourceSetupModal` a proper `Dialog`, drop the duplicated `action_dismiss_dialog`, and remove the `DecisionForm` CSS overrides.
- **C5 (M)** Merge `LLMClient.complete` and `.chat`, and share cache/log code with `JevClient`. Keep one JSON fence parser.
- **C6 (S)** A single `update_config(mutator)` helper to replace 6 copies of load/mutate/write.
- **C7 (S)** One `reload(parts)` method in `runtime.py` to replace three near-identical reload methods.
- **C8 (S)** Delete dead code:
  - `core/events.py` bus.
  - `InstrumentTable`.
  - `session_factory`.
  - `models.LLMCall`.
  - `asset_metrics._values`.
  - The `apscheduler` dependency (unless G8 is chosen).
  - The unused `screens/reports.py`.
- **C9 (M)** Split files over 1k lines (research, targets, home, theses) into a screen file plus pane widget files.
- **C10 (S)** Move the static metric tables in `asset_metrics.py` (about 450 lines) into a data or TOML file.
- **C11 (S)** One canonical list of asset classes (it exists 4 times today) and one set of number formatters.
- **C12 (S)** Update the key map in `PROJECT_SPEC.md` §7 and the decision tables in §4.

## D. Keyboard-first UX (consistency)
- **D1 (M)** A key grammar across all screens:
  - `n` new, `e` edit, `d` delete (with confirm), `r` refresh, `/` filter, `enter` open, `esc` back.
  - Theses accept/reject move to `+`/`-`, or `y`/`x` in a dedicated review mode.
  - Removes the current clashes between `a`, `d`, `x`, `e`, `r` and `s`.
- **D2 (S)** Every destructive action asks for confirmation with the same `y`/`n` inline prompt. Targets remove and Config remove-market don't ask today.
- **D3 (M)** A back/forward history stack (`ctrl+o` / `ctrl+i`, or `backspace`) so `esc` means the same thing everywhere.
- **D4 (M)** A sticky "focused instrument" shared by all screens and shown in the status bar. Pressing 3/4/5/6 opens that ticker's research, theses, ask or decisions.
- **D5 (S)** Replace the Home timer-retry highlight hack (`home.py:1083`) with D4.
- **D6 (S)** Decisions gains edit, delete and filter. Enter in a form moves to the next field instead of submitting. Detail updates on highlight, like the other screens.
- **D7 (S)** Consistent empty, loading and error states: a shared `EmptyState` widget that always suggests the next key to press.

## E. Keyboard-first power features (unique)
- **E1 (M)** A `:` command line in vim ex-mode, with history and tab completion:
  - `:add BHP.AX`
  - `:report NVDA`
  - `:ask why did margins fall?`
  - `:decide buy MSFT`
  - `:theme light`
- **E2 (M)** A which-key popup. Press `space` (leader) to see the next possible keys, e.g. `space r g` generates a report. It helps people discover keys without the help screen.
- **E3 (S)** Global fuzzy ticker jump with `ctrl+k`. It searches across watchlist, theses and decisions, then sets the focused instrument (D4).
- **E4 (S)** Vim-style marks: `m` + letter bookmarks an instrument or screen, `'` + letter jumps back to it.
- **E5 (S)** Count prefixes and `gg`/`G`/`ctrl+d`/`ctrl+u` in every table.
- **E6 (S)** Yank with `y` to the clipboard via OSC52: ticker, claim with citation, or a markdown summary of a decision.
- **E7 (M)** A dot-repeat `.` to repeat the last action, e.g. accept the next candidate evidence.
- **E8 (S)** `ctrl+s` saves any screen as SVG using Textual's `save_screenshot`, for journaling or sharing.

## F. Decision quality (unique features that fit "make the best decisions")
- **F1 (M)** **Pre-mortem on new decisions.** Before a decision is saved, the LLM writes "it's 12 months later and this failed, here's why". It cites only gathered evidence, and you must acknowledge it.
- **F2 (S)** **Cooling-off lock.** A new buy/sell decision stays "pending" for a configurable number of hours before you can mark it acted-on. This nudges against impulse trades.
- **F3 (M)** **Calibration score.** Record your confidence (%) on each decision, then score it at review. Home shows a Brier score and a braille calibration chart, so you learn whether you're over-confident.
- **F4 (S)** **Confirmation-bias meter** on Theses: the ratio of supporting to contradicting accepted evidence, with a warning if you only ever accept bullish items.
- **F5 (M)** **Steelman key** on a thesis. It produces the strongest cited counter-case, framed as the opposite position.
- **F6 (M)** **Invalidation watchers.** Decision invalidation criteria and thesis falsifiers become live triggers, e.g. price crosses a level or a matching filing arrives. When one fires it flashes the status bar and optionally sends a macOS notification.
- **F7 (S)** **Outcome vs benchmark at review.** Auto-compute the return since the decision against the market index, so reviews are honest.
- **F8 (S)** **Decision markers on the price chart.** Buy/sell/review dates appear as glyphs on `PriceChart`.
- **F9 (S)** **Bias checklist** in `DecisionForm`: FOMO, anchoring, sunk-cost and recency as quick toggles. They are stored and shown at review.
- **F10 (M)** **Position-size modal**: account size, risk %, stop (ATR-based default), resulting quantity. No broker integration. ⚠️ This conflicts with the stated ethos in the help tutorial (`help.py`: "does not size positions"). Only do it if that philosophy changes.
- **F11 (S)** **Slow mode.** Hide intraday quotes and show only daily closes, to reduce noise-driven trading for long-term investors.

## G. Research and data (surface what already exists)
- **G1 (M)** **"Δ since last look" per instrument.** A changelog of new evidence, events and report-claim changes since you last opened the ticker. It fits the app name.
- **G2 (M)** **Report diff.** Compare the current report with the previous one in `history/` side by side: claims added or removed, and the change in the sentiment score.
- **G3 (M)** **Store a Yahoo `ticker.info` snapshot as evidence** (valuation, analyst targets), so reports and chat can cite it. It is display-only today.
- **G4 (M)** **Form 4 insider net flow** from SEC data that is already fetched, with a sparkline per ticker.
- **G5 (M)** **Earnings-prep brief.** Auto-generated N days before an upcoming earnings event, with what to watch and a cited list of the last guidance.
- **G6 (S)** **Sentiment trend sparkline** per instrument, once A3 is fixed.
- **G7 (M)** **Compare mode.** Two instruments side by side (metrics, charts, report headlines).
- **G8 (M)** **Headless CLI** for cron or launchd: `delta gather`, `delta report X`, `delta review-due`. Uses the existing services, and could run on the unused apscheduler.
- **G9 (M)** **Watchlist heatmap and correlation grid** in colour blocks: who moves together, as a quick diversification check.
- **G10 (S)** **RSS dedupe and clustering.** Collapse the same story from several feeds into one evidence item with several sources.

## H. LLM experience
- **H1 (M)** Stream tokens into Chat and the report view, so long answers don't look frozen.
- **H2 (S)** **Cost preview.** Show the estimated tokens and $ before a report, chat or summary runs, with the option to confirm above a threshold.
- **H3 (S)** Log cache hits (the spec requires it) and show "cached" in the UI.
- **H4 (M)** An eval harness: golden evidence sets with an asserted citation validity rate. The spec calls this a prerequisite.

## I. Look and feel
- **I1 (S)** A colour-blind-safe dark palette option: blue/orange for up/down.
- **I2 (S)** A density toggle (compact or comfortable) and an ASCII fallback for terminals without braille glyphs.
- **I3 (M)** A first-run guided tour: a demo database with fixture data, and a step-through overlay that teaches 1-6, `g`, `?` and `:`.
- **I4 (S)** Move per-screen `DEFAULT_CSS` into `delta.tcss` sections so the styling can be themed and reviewed in one place.

## J. Layout and visual design (second-pass review)
This review is based on the `compose_content` and CSS of every screen.

What already works:
- The btop-style `Pane` grammar.
- Hints in the pane border.
- Token-only colour.
- The braille charts.

The findings below are about how screen space is spent and about hierarchy.

**Home (`home.py:455-527`)**
- **J1 (M)** Replace the redundant "go" and "system" panes (bottom row, 8–12 rows) with a **"Needs you today" agenda**:
  - Reviews due.
  - Falsifier hits.
  - Earnings in the next 7 days.
  - Stale sources.
  - Each line has a jump key.
  
  Rationale: "go" repeats the footer nav, and "system" repeats the status bar (provider, model, spend). The bottom third of the dashboard currently shows chrome, not decisions.
- **J2 (S)** Fold the `DELTA overview clock` header row into the status bar, and drop `ScreenFooter`'s `padding-top: 1`. That returns 2 rows to content on every screen.
- **J3 (S)** First-run Home: when there are no targets, the whole grid becomes the `setup_checks` checklist with one key per step, instead of six empty panes.

**Global layout mechanics**
- **J4 (M)** **Pane zoom** (tmux-style `z`): maximise the focused pane and press again to restore. This matters because:
  - Research is 36 + 1fr + 40 columns, so at 120 columns the report gets about 42 columns to read markdown.
  - Theses has the same 36/1fr/40 split.
  
  One `DeltaScreen` feature solves both.
- **J5 (M)** Drop the Research "company" column once D4 (sticky focused instrument) and E3 (`ctrl+k` switcher) exist. That gives 36 columns back to the report. Company summary and gather chips move into the report header.
- **J6 (S)** One breakpoint system. Today:
  - `NARROW_WIDTH = 100` is set on 6 screens.
  - `PaneRow` uses 72.
  - Research uses its own `-compact`.
  
  Replace these with an app-level `-narrow`/`-short` class set once on resize, and screens style against it.
- **J7 (S)** The Targets chart is fixed at `height: 8` (`targets.py:589`). Let it flex (for example `1fr`, min 6) so tall terminals get a real chart.

**Density and hierarchy**
- **J8 (M)** Targets inspector: its 8 metrics are each a bordered `Pane` inside a pane (`targets.py:645-653`). That is box-in-box, 2 border rows per card, with little on screen. Replace it with a borderless two-column key/value grid under muted section headings. That roughly doubles the metrics visible per screen.
- **J9 (S)** A typographic scale used everywhere:
  - Pane title: lowercase, bold, accent.
  - Section heading: muted bold.
  - Field: label muted, value foreground.
  
  Decisions uses UPPERCASE headings (`decisions.py:191`) and Config uses `.cfg-heading`. Unify them into one `SectionHeading` widget.
- **J10 (S)** Don't rely on colour alone for changes. Add `▲`/`▼` glyphs next to green/red percentages (Home `.w-chg`, Targets chart change, the report sentiment delta). This pairs with I1.
- **J11 (S)** Nav badges in the status bar: `6 Decisions·2` (reviews due), `4 Theses!` (falsifier hit), `3 Research•` (new evidence). Attention becomes visible from any screen.

**Chat (`chat.py:142-173`)**
- **J12 (M)** Replace the "options" pane with a **citations sidebar**. It shows the evidence cited by the highlighted answer (kind colour, date, source), and pressing enter jumps to it in Research. Model and provider chips repeat `m`/`p` and the status bar, and the web chip is a no-op (A2).

**Decisions (`decisions.py:35-60, 144-197`)**
- **J13 (M)** Rework `DecisionForm`:
  - Multi-line `TextArea` for rationale, valuation and invalidation. They are one-line `Input`s today, which is poor for a journal.
  - Instrument autocomplete (C2).
  - A thesis *picker* instead of typing a thesis ID.
  - Horizon and review-date presets (`+3m`/`+6m`/`+1y`).
  - Enter moves to the next field and `ctrl+s` saves.
  - Consider a wider modal or a full-screen editor. At 64 columns, 7 fields feels cramped.
- **J14 (S)** Decision detail becomes a **timeline**: created, then each review, then next review due. Show price at decision against price now (feeds F7/F8). Use pane hints instead of the one-off `KeyStrip` row.

**Theme and polish**
- **J15 (S)** Dark-only: drop `delta-light` and the `f2` toggle (simplification). Optionally add a softer dark variant (e.g. `#0b0e14`) for terminals where pure black plus `#264b96` looks harsh.
- **J16 (M)** **Snapshot tests** (`pytest-textual-snapshot`) of every screen at 80×24, 120×40 and 200×50. Layout regressions (like the wrong hints in A1) become visible diffs, and they give a before/after for every J item.

## K. Watchlist "metrics" inspector: header and price chart (from your screenshot)

### What's wrong, with causes in the code
- **K-a: the line looks like scattered dashes, not a price line.** This is the main problem.
  - A month has about 22 closes, but the plot has about 170 dot columns. `BrailleGraph._sample` (`widgets.py:572`) takes a bucket mean, so each close repeats across about 8 dot columns as a flat run.
  - `_plot_cells` (`widgets.py:695`) sets only one dot per column, with nothing joining consecutive points vertically. The result is a staircase of dotted dashes with gaps.
- **K-b: only one Y label (320.00).**
  - `nice_ticks(low, high, 3)` returns ticks such as 310/320/330. The scale runs from the raw data min to max (309.90–338.98), so the outer ticks land between rows or off the edge.
  - The middle gridline runs straight through the line, so it reads as data.
- **K-c: axis jog at the bottom right.**
  - The x-rule is `"└" + rule(plot) + "┘"` (`widgets.py:814`). It starts one column left of the plot and ends one column past the gutter `│`.
  - That is an off-by-one: the `┘` never meets the Y axis (the visible step in the screenshot), and the `└` bracket sits under nothing.
- **K-d: the grey `$panel` slab** behind the chart (`targets.py:589`) makes it read as an input field. The axis labels sit inside the slab.
- **K-e: header rows are redundant or ambiguous.**
  - `AAPL US:AAPL · US · equity` repeats the ticker and the market.
  - `— today` means "no intraday change" but reads as a dash.
  - `high / low / vol` has no period, so it's unclear whether it's the day, the month or the range.
  - Three blank rows between blocks.
- **K-f: the range isn't visible.** It's buried in `price · month` and the `r` hint. There are only 3 ranges, and `r` cycles in a non-obvious order (month → all → day).

### Proposed redesign (mock at 72 columns)
```
┌ metrics · AAPL ─────────────────────────────────────────────────────┐
│ Apple Inc.                                  NASDAQ · equity · USD    │
│ 338.98  ▲ +2.14 (+0.64%) today     52w ▕██████████████▊▏ 97% of high │
│                                                                      │
│ 1D  5D [1M] 6M  YTD  1Y  ALL        ▲ +9.2%   hi 338.98   lo 309.90  │
│                                                          ⢀⣀⡠⠤● 338.98│
│                                             ⣀⡠⠔⠒⠉⠉⠒⠊⠉⠁        ├ 340 │
│                              ⢀⡠⠔⠊⠉⠑⠢⢄⣀⡠⠔⠊                    │     │
│ ┄┄┄┄┄┄┄┄┄┄┄┄⡠⠔⠊⠉⠒⠤⣀⡠⠔⠊┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄┄ ├ 320 │
│   ⢀⡠⠤⠒⠊                                                      │     │
│ ⠔⠊                                          E                ├ 310 │
│ ─────┬───────────────────────────┬──────────────────────┬────┘     │
│   24 Aug                       08 Sep                 21 Sep          │
│                                                                      │
│ PROFITABILITY                      BALANCE SHEET                     │
│ gross margin        46.2%          debt / equity           1.45      │
│ operating margin    31.5%          current ratio           0.87      │
└ r/R range  [ ] scrub  b benchmark  i glossary ───────────────────────┘
```

### Items
- **K1 (S) Connected line.** Interpolate when points are fewer than dot columns (linear resample). Otherwise keep the bucket mean, but also dot the vertical span between consecutive samples (Bresenham in braille). This fixes K-a, and the `BrailleGraph` sparklines on Home get it for free.
- **K2 (S) Nice-bounded Y scale.** Scale the plot to `[first tick, last tick]` from `nice_ticks` so every tick lands on a row, and show all 3–5 ticks. Gridlines become a faint `┄` on each tick row, drawn *behind* the line.
- **K3 (S) Fix the axis joins.** The x-rule starts under plot column 0 and ends in `┘` exactly under the gutter `│`. Use `├` ticks on the Y gutter.
- **K4 (S) Last-price marker.** A `●` at the end of the line with the price label in the gutter. The docstring at `widgets.py:649` argues against this, but with a connected line it no longer "fights the dots", and it answers "where is it now?"
- **K5 (S) Colour the line by direction over the range:** `$text-success` if the range is up, `$text-error` if down, accent if flat. An optional faint area fill below it (`fill=True` is already supported).
- **K6 (S) Transparent chart background.** Drop the `$panel` slab and let the gridlines give structure (`targets.py:589`).
- **K7 (S) Rewrite the header into two dense rows:**
  - Row 1: name, then exchange · class · currency, right-aligned.
  - Row 2: price, then `▲/▼ abs (pct) today`, or `closed · last 21 Sep` when there's no intraday move.
  - Plus a 52-week position bar.
  - Remove the duplicated ticker and market, and the blank rows.
- **K8 (S) Visible range tab strip:** `1D 5D 1M 6M YTD 1Y ALL`, with the active range highlighted like the `NavKey.-active` style. `r`/`R` go forward/back. The hi/lo for the range sits next to the change, so their period is unambiguous.
- **K9 (M) Keyboard crosshair scrub.** `[`/`]` step one bar and `{`/`}` step a week. A vertical cursor column is shown, and the header temporarily shows `date · close · Δ from range start`. `esc` leaves the scrub. This makes the chart readable without a mouse.
- **K10 (M) Event markers on the x-axis row:** `E` earnings, `D` ex-dividend, `◆` your decisions (F8), `!` falsifier hits. They use data already in the calendar and decisions tables, and the scrub cursor on a marker shows its detail.
- **K11 (M) Benchmark overlay (`b`):** the market index (e.g. `^GSPC` / `^AXJO` from the market plugin) normalised to the range start and drawn as a dim second line. The header shows `vs index +3.1%`, which answers the question "is this stock or the market?"
- **K12 (S) Flexible chart height:** `1fr` with `min-height: 6` and `max-height: 16`, so the chart grows on tall terminals (absorbs J7).
- **K13 (S)** Test the pure `_runs()` layout at a few widths and heights with an assertion that the axes join. `_runs` is already pure and testable without an App.

Files:
- `delta/tui/widgets.py` (`BrailleGraph`, `PriceChart`).
- `delta/tui/axes.py` (`nice_ticks`, `x_ticks`).
- `delta/tui/screens/targets.py` (hero/status/chart header, around lines 860–980; `action_cycle_range` at 1216).
- `delta/asset_metrics.py` (extra ranges, benchmark series).

---

## Suggested first batch (if you want a default)
- A1–A7
- B1, B2, B3, B4, B6, B7
- C1, C3, C8
- D1, D2, D4
- E1 or E2
- F1, F3, F7
- **Layout:** J16 first (to get baseline snapshots), then J1, J2, J4, J8, J13
- **Chart:** K1–K8 as one PR (all S), then K9 and K10

## Verification (per implemented item)
- `uv run ruff check .`, then `uv run mypy --strict delta/core delta/llm`, then `uv run pytest` (offline: respx and FakeLLM).
- For the performance items, run `uv run pytest --durations=20` before and after. Time Home refresh and ingest against a `tmp_path` database seeded from the fixtures.
- For TUI items, add Textual `run_test()` pilot tests for the new bindings, and do a manual check with `uv run delta`.
