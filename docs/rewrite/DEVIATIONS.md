# DEVIATIONS from exact golden parity

Per `docs/RUST_REWRITE_PLAN.md` Rule 1: an exact match must exist unless the
deviation is approved. Pre-approved classes (glyph fallbacks, Tier B
wrap-point differences, trailing-whitespace cells) do not need a gate.

| # | Where | Deviation | Class | Approval |
|---|---|---|---|---|
| 1 | glossary body (cols 90-91) | The `VerticalScroll` scrollbar (2 cells: `▄` thumb glyphs in `#3a3a3a`/`#d4d4d4`) is not ported; excluded from the Tier B check in `tests/golden.rs`. Porting the scrollbar widget closes it | widget gap | pending (logged per Rule 1; colour diffs are never pre-approved) |

No other deviations: every other cell of the five passing goldens
(home, default, range-cycled, narrow, glossary) matches character, fg, bg
and attrs exactly.
