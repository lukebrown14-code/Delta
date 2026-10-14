# DEVIATIONS from exact golden parity

Per `docs/RUST_REWRITE_PLAN.md` Rule 1: an exact match must exist unless the
deviation is approved. Pre-approved classes (glyph fallbacks, Tier B
wrap-point differences, trailing-whitespace cells) do not need a gate.

| # | Where | Deviation | Class | Approval |
|---|---|---|---|---|
| 1 | glossary body (cols 90-91) | ~~The `VerticalScroll` scrollbar is not ported~~ **Closed**: the scrollbar is ported (track `#3a3a3a`, thumb `$foreground`, half-block `▄` cap when the proportional thumb position rounds up) and the glossary gates Tier A at 120x40 | widget gap | resolved |

No open deviations: every cell of the full 3-size matrix (80x24, 120x40,
200x50 — default, range-cycled, narrow, glossary, home, research, theses,
ask, decisions, settings) matches character, fg, bg and attrs exactly,
(the glossary gates Tier B at 80x24/200x50 per the manifest, and its
text layer and colours match exactly there too).
