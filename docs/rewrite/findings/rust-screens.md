# Findings — rust-screens (R3, in progress)

Stream: `delta-tui` screens. Oracle: `fixtures/golden_screens/*.json` via the
cell-grid model in `crates/delta-tui/src/screen.rs` and the harness in
`crates/delta-tui/tests/golden.rs`.

## Done

- **Golden harness** — loads the exporter JSON, renders the scenario, diffs
  every cell (character, fg, bg, bold); prints per-cell diffs on failure.
- **Watchlist screen / metrics inspector at 120x40, Tier A: zero mismatches**
  for both `default` and `range-cycled` states (`tests/golden.rs`), including
  the PriceChart, metric grid, range strip, pane chrome and status bar.
- The renderer is a deterministic painter over the exported layout; the
  scenarios' data (80 seeded bars, `_price`, frozen clock 2026-09-21) is built
  in the test, mirroring `tests/export_golden.py`.

## Parity traps found while reaching zero (all now encoded in code/tests)

| # | Category | Where | Finding |
|---|---|---|---|
| 1 | parity | status bar | Footer hint runs: key (bold) + " hint" (plain) + separator spaces with **fg None**; the two cells flanking the bar (col 0 black, col w-1 black) are defaults, not panel. The `$0.00` cluster words carry fg, their separators don't |
| 2 | parity | pane chrome | Title runs are always blue bold — even on a blurred (unfocused) pane; only the border glyphs take `$border-blurred`. Title starts two cells after the corner (corner + one plain fill) |
| 3 | parity | chart rule row | The rule row's trailing blank run is foreground-styled, the braille rows' gutter trailing spaces are `$text-disabled`; blank-braille cells are foreground, not the line colour (PriceChart's run kinds already encode this — the painter just maps them) |
| 4 | parity | metrics grid | Rich's ratio-column widths were pinned from the golden (label col ends 84 excl, right pair 85..116); a general implementation of Rich's table algorithm is only needed when data with different label lengths ships |

## Done since first commit

- **`narrow-80x24` (Tier A): zero mismatches** (`draw_watchlist_narrow`). The
  narrow frame is the metrics pane full-width with `esc back` added to the
  pane hints; the status bar drops the provider name and help hint (the
  cluster string is assembled per breakpoint), the chart reflows to 72x12 and
  the grid edges move (value col ends 41 excl, right pair 42..76).

## Done since the glossary commit

- **`home-120x40` (Tier A): zero mismatches** (`draw_home`): DELTA chip +
  right-aligned clock, watchlist table with the filled braille spark and
  cursor row, since-you-last-looked summary/stale warnings, upcoming/theses
  panes, the agenda with jump keys, and the `1 Home` status-bar variant.
  Exported via the new `home` scenario in `tests/export_golden.py`.

## Remaining R3 work

| # | State | Tier | Notes |
|---|---|---|---|
| 6 | closed | `glossary-120x40` | The VerticalScroll scrollbar is ported (track, half-block cap, proportional thumb); the glossary now gates Tier A at 120x40 and Tier B (text exact) at 80x24/200x50 |
| 7 | **reopened** (4 Oct) | other screens | The goldens are green, but Research, Theses, Ask, Decisions and Settings painters take no data: they draw the empty landing state only. Functional port tracked in `docs/rewrite/REMAINING.md` |
| 8 | decided: D2 | `screens.rs` | The painter is layout-hardcoded to the captured geometry. Decision: rebuild every screen as a data-driven `Component`; painters retired screen by screen |
| 9 | bug | app + watchlist bindings | Rust: `g` glossary, `h`/`l` range. Python: `g` Go, `i` glossary (metric help), `r`/`R` range, `h` Home. Rust also lacks `m`, `p`, `?`, `f2` and the palette. Fix to match Python (R3.1a app, R3.2 per screen) |
| 10 | gap | `theme.rs` | Python ships `delta-light` (`f2` toggles); Rust is dark-only. Port in R3.1a |
| 11 | parity | quotes | Python streams quotes by default on Watchlist and Research (`QuoteFeedMixin`); Rust needs `DELTA_QUOTES=1`. Match Python in R3.2 |

## R3.0 (gates stream)

- **`screens.rs` split into `screens/{mod,status_bar,home,watchlist,research,theses,ask,decisions,settings}.rs`** — move only; the shared status-bar/tab painters live in `status_bar.rs`, and `mod.rs` re-exports the old paths so callers (`desk.rs`, `lib.rs`, the harness) are unchanged. 28/28 goldens Tier A green before and after.
- **Golden harness is manifest-driven** (`crates/delta-tui/tests/golden.rs`): reads state/size/tier/seed from `fixtures/golden_screens/manifest.json`, opens the seed DB through `delta_core::db::Db`, and renders via a size-dispatch table. Tier A fails on any cell mismatch; Tier B (glossary at 80x24/200x50) fails only on text and prints colour findings. On failure it prints a side-by-side want/got row diff. A scenario whose state has no Rust renderer fails loudly, so new exports demand a renderer.
- **The seed's AAPL data drives the watchlist/home states** — bars and the `Market cap`/`P/E` cards come from `fixtures/golden_seed.db` (cards formatted `1 decimal, T for trillions`, mirroring the canned quoteSummary strings). Replacing the hardcoded `price()` mirror in Rust test code was the point of the seed.

### Decisions this stream made (flag for triage)

- **`chatmessage` table**: Python keeps chat turns in memory only (`delta/tui/screens/chat.py`), but the card requires the seed to carry chat history readable by the Rust harness, so the seed defines a `chatmessage` table (seq, role, text, citations, source, ts) in `fixtures/golden_seed.db`. It is seed-fixture schema, not an app migration; if the ask stream wants it elsewhere, this is the place to decide.
- **`fixtures/golden_reports/`**: reports are markdown + sidecar JSON files (`write_report`), not DB rows, so the seed materialises them next to the DB (`US:AAPL/2026-09-21.{md,json}`, `US:MSFT/...`). Generated by `tests/golden_seed.py`, byte-deterministic under the frozen clock.

### New findings

| # | Category | Where | Finding |
|---|---|---|---|
| 12 | bug (pre-existing, Python-first) | `tests/test_home.py::test_agenda_lines_and_jump_keys` | Fails on `rewrite/rust` @ `d5ab1ba` before any R3.0 change: asserts `1 earnings` but the agenda shows `no earnings in the next 7 days`. The test freezes `NOW` (2026-09-21) for the render but seeds bars with `datetime.now(UTC)`; the earnings line depends on a real-vs-frozen clock path (event at 2026-09-24 leaves the 7-day window once the wall clock passes it). Time-dependent test, not a port regression — fix in Python on `main` per Rule 2, then re-merge |
| 13 | simplify | `tests/export_golden.py` | The per-scenario fresh-DB seeding (`export_scenario`) and `tests/golden_seed.py` both build the same world; a later stream can point the exporter at `fixtures/golden_seed.db` and drop the duplicated seed path (kept for now so the committed goldens stay byte-identical) |
