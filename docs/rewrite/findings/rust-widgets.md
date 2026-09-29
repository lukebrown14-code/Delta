# Findings — rust-widgets (R1d)

Stream: `delta-tui` widgets. Reviewed against `delta/tui/axes.py`, `widgets.py`, `components.py`, `theme.py`.

## Ported

| Python | Rust | Notes |
|---|---|---|
| `axes.py` | `src/axes.rs` | `nice_ticks` / `plot_scale` / `format_price` / `x_ticks` — pinned to Python golden outputs |
| `theme.py` | `src/theme.rs` | resolved `delta-dark` token table (dark-only), `token_color` fallback |
| `BrailleGraph` | `src/braille.rs` | sample / resample / lerp / Bresenham / fill; `rows()` pinned to goldens |
| `PriceChart._runs` | `src/chart.rs` | full layout: nice Y ticks, `┄` gridlines, `├` gutter ticks (K3), `●` last-price marker (K4), `┬` rule + centred date labels, `direction()` colour tokens (K5), narrow/no-data/short-height degradations — pinned to goldens at 40×8 and 18×8 |
| `DeltaTable` | `src/table.rs` | zebra stripes + `$primary` row cursor, hand-styled |
| `Dialog` | `src/dialog.rs` | centred bordered box on the content, `ModalStack`, Esc/Enter answers |
| `EmptyState`, `SectionHeading` | `src/components.rs` | glyph semantics (`✓`/`!`), block-plus-bold heading |
| `SuggestionList` | `src/components.rs` | type-ahead filter, cursor reset on filter change, Enter accepts |
| command palette | `src/components.rs` (`CommandPalette`) | overlay + suggestion list |
| `KeyStrip`/`hint_markup` | `src/components.rs` (`WhichKey`) | `key hint · key hint` footer line |

## Parity notes for the oracle (R3 will diff against `fixtures/golden_screens/`)

| # | Category | Where | Finding |
|---|---|---|---|
| 1 | bug-risk | axes/braille/chart rounding | Python `round()` is banker's rounding; the dot-row and tick-row math hits `.5` ties. Rust uses a `py_round` (half-to-even) and, critically, rounds the **scaled term before subtracting** — `(11 - round(x))`, not `round(11 - x)`. The second one silently shifts dots by a row. Any future port must keep this order |
| 2 | dev | `x_ticks` mid-column | Python `round(middle * last_col / (n-1))` needs the same banker's rounding; ported via `py_round` |
| 3 | gap | `PriceChart` benchmark/scrub | The plan's done-list names benchmark series and scrub; the Python `PriceChart` itself has neither (they live in the inspector screen's overlay). Deferred to R3 with the screen that owns them — not silently dropped |
| 4 | simplify | `DeltaTable` | Python inherits Textual's DataTable (sorting, scrolling); the Rust port is a thin zebra/cursor styler over ratatui's `Table`. Scrolling state is the caller's `TableState` — screens must wire it |
| 5 | gap | `components.py` helpers | `QuoteFeedMixin`, `require_selection`, `goto`, `thesis_from_citations`, `quote_suffixes` are app/state glue, not widgets — they port with `delta-services` (R2) and the screens (R3) |
| 6 | gap | `widgets.py` chrome | `ActionChip`, `StatusDot`, `Pill`, `PaneRow`/`PaneStack`, `KeyGrid` are layout containers that only make sense with the R3 shell; the styled primitives (chips/dots/pills) port per-screen |
