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

## R3.1a (TUI foundations) — new findings

| # | Category | Where | Finding |
|---|---|---|---|
| 7 | parity (decided here, flag for triage) | `delta/tui/app.py` vs `docs/RUST_REWRITE_PLAN.md` | The palette key: Python opens Textual's command palette with `ctrl+p` (`App.COMMAND_PALETTE_BINDING`, textual 8.2.8); the plan and the task card say `ctrl+k`. The Rust app binds **both** (`app.rs::handle_shell_key`), so the plan's key works and Python's is preserved. If only one should survive, say which at triage |
| 8 | observation | `delta/tui/app.py:168-170` | `action_toggle_theme` calls `self.notify(f"Theme: {self.theme}")`, but the exported `home-light` goldens show **no toast** at any size (Textual 8's notification rail never paints under the exporter). The goldens are the oracle, so the Rust `f2` sets the status line instead of drawing a toast. If the Python app should visibly toast, that's a Python-first fix |
| 9 | parity | `delta/tui/theme.py` (DELTA_LIGHT) + `delta/tui/screens/home.py:374` | In `delta-light` the dark `#5b8def` token (text-primary = border = footer-key in the dark theme) splits: bold runs and the agenda `▸` glyph resolve to the derived `text-primary` `#095261`, non-bold pane-border glyphs to `$border` `#0f7d93` (verified across home-light at all three sizes). The cell model cannot see which token a painter meant, so `screen.rs::remap_token` splits on `bold` + glyph. Consequence for R3.2: paint light-theme-visible **text** in blue bold, or it will remap as a border colour |
| 10 | simplify | `crates/delta-tui/src/app.rs` (was `g` glossary, `h`/`l` range, `,`/`.` instrument) | The Rust-only app bindings had no Python counterpart (`delta/tui/app.py::DeltaApp.BINDINGS`); removed. `desk.rs`'s `cycle_range`/`cycle_instrument` are now app-unused — the R3.2 watchlist stream should re-home them behind the Python screen keys (`r`/`R` range, arrows/enter) instead of app-level keys |
| 11 | bug-risk (doc) | `textual/_wrap.py::compute_wrap_offsets` vs `rich/_wrap.py::divide_line` | Textual has **two** wrap algorithms. `_wrap.compute_wrap_offsets` fits the chunk *including* its trailing space; `Content._wrap_and_format` actually calls Rich's `divide_line`, which fits the *rstripped* word and may let the trailing space overflow the line. The first port broke `…the companies you` onto the next line (`help-open` goldens). `wrap.rs` now ports `divide_line`; anyone wrapping prose must use it, not the `_wrap` module |
| 12 | parity (encoded) | `fixtures/golden_screens/{go,help,palette}-open-*.json` | The modal dim rule, pinned three times: fg-None cells resolve to the default foreground **after** dimming (undimmed `#d4d4d4`) while every explicitly painted colour dims by `int(v*0.4)`; modal-dialog borders/int interiors keep the *un*-dimmed surface/background. `screen.rs::dim` + `dialog.rs::dialog_frame` encode this — do not "simplify" the fg exemption away |
| 13 | simplify | `components.rs::CommandPalette::draw_screen` | The exported palette input row has two Textual internals reproduced literally: the placeholder's first cell renders `#000000` on `#d4d4d4` and the row under the input is a full-width `#00ff00`-on-scrim run. They look like rendering artefacts of the Input/cursor layering; replicated because the goldens say so. If Textual ever changes them, regenerate the goldens and these cells |
| 14 | gap | `app.rs` m/p | `m` (model picker) and `p` (provider picker) are bound and raise `Action::ShowModelPicker`/`ShowProviderPicker` (the app shows a status line); the picker screens themselves land with R3.2 settings per `REMAINING.md` |
| 15 | gap (Tier B) | `markdown.rs` | DeltaMarkdown ports headings (h1 centred bold / h2 underline / h3+ bold, all `$text-primary`), paragraphs with strong/em/softbreak, inline code tint, hr, bullet+ordered lists, fences. **Not** ported: tables (the help tutorial's table is below the fold at all three golden sizes — port before the ask/research streams render tables), blockquote chrome (text flows unstyled), strikethrough (cell model has no strike attr; text renders plain), link styling (Textual 8 gives links only a click action). All Tier B wrap/structure territory |
| 16 | perf | `screen.rs::dim` | The R3.0 dim lookup was replaced by the equivalent per-channel formula `int(v*0.4)` over a `match` of the two palettes' token hexes (verified against the exporter values) — no allocation, same output, and it now covers the light palette too |
