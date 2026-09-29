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

## Remaining R3 work

| # | State | Tier | Notes |
|---|---|---|---|
| 5 | gap | `narrow-80x24` (Tier A) | Needs the `-narrow` breakpoint layout (list-only pane + detail-open flow). The list pane painter is shared; the narrow frame geometry still needs capturing from the golden |
| 6 | gap | `glossary-120x40` (Tier B) | Prose modal; Tier B diffs are logged as findings rather than failures. Needs the DeltaMarkdown-style text wrapping rules first |
| 7 | gap | other screens | Home, Research, Theses, Ask, Decisions, Settings need their own exporter scenarios before they can be ported against goldens (extend `SCENARIOS` in `tests/export_golden.py`) |
| 8 | simplify | `screens.rs` | The painter is layout-hardcoded to the captured geometry; as more states land, factor shared pieces (pane hints, hero row) behind the data they render. Deliberately not generalised ahead of the second screen |
