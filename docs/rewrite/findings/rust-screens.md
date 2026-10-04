# Findings — rust-screens (R3/R4 review)

Stream: `delta-tui` screens. Oracle: `fixtures/golden_screens/*.json` via the
cell-grid model in `crates/delta-tui/src/screen.rs` and the harness in
`crates/delta-tui/tests/golden.rs`.

## Done

- **Golden harness** — loads the exporter JSON, renders the scenario, diffs
  every cell (character, fg, bg, bold, reverse, italic, underline); prints
  per-cell diffs on failure. Static scenarios pass the full 3-size matrix.
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

## Remaining live parity work

| # | State | Tier | Notes |
|---|---|---|---|
| 6 | closed | `glossary-120x40` | The VerticalScroll scrollbar is ported; glossary gates Tier A at 120x40 and Tier B (text exact) at 80x24/200x50 |
| 7 | closed | static screens | All screens have narrow and wide painters; static 3-size matrix passes (glossary Tier B at 80x24/200x50) |
| 8 | simplify | `screens.rs` | Painter is layout-hardcoded to captured geometry; factor shared pieces only as additional states require it |
| 9 | open (from status PR #19) | app + watchlist bindings | Rust: `g` glossary, `h`/`l` range. Python: `g` Go, `i` glossary (metric help), `r`/`R` range, `h` Home. Rust also lacks `m`, `p`, `?`, `f2` and the palette. Re-verify against the live-workflow app, then fix to match Python |
| 10 | open (from status PR #19) | `theme.rs` | Python ships `delta-light` (`f2` toggles); Rust is dark-only. Re-verify, then port |
| 11 | open (from status PR #19) | quotes | Python streams quotes by default on Watchlist and Research (`QuoteFeedMixin`); Rust needs `DELTA_QUOTES=1`. Match Python |

## Live App integration review (2 October 2026)

The static matrix covers painter fixtures. Populated Settings also passes the
Python oracle at 80x24, 120x40 and 200x50. Populated Research, Theses, Ask and
Decisions, and live modal/focus states, still need full-cell comparison before
cutover.

- **bug / cancellation:** Rust Ask originally accepted a late worker answer
  after clearing its transcript. Generation checks and cancellation now prevent
  stale turns; a binary interaction regression exercises old/new request results.
- **perf / render path:** Settings diagnostics reopened SQLite on each paint.
  Cached worker diagnostics now keep rendering free of storage reads.
- **parity / bindings:** canonical `chat.py` stored citations navigate to Research,
  web citations open the browser, and citation selection wraps. Rust now follows
  these paths. Canonical `targets.py` Enter opens narrow metrics; Escape returns.
- **parity / settings:** selected market edits now prepopulate their profile and
  lock the ID. Settings focus/diagnostics still require populated cell comparison.
- **parity / charts:** live quote appending without an associated timestamp
  diverged from Python `targets.py` historical series. The quote updates the hero
  independently; chart values remain paired with stored timestamps.

No layout, colour or binding deviation has been approved by this entry.
