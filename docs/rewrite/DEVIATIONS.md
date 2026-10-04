# DEVIATIONS from exact golden parity

Per `docs/RUST_REWRITE_PLAN.md` Rule 1: an exact match must exist unless the
deviation is approved. Pre-approved classes (glyph fallbacks, Tier B
wrap-point differences, trailing-whitespace cells) do not need a gate.

| # | Where | Deviation | Class | Approval |
|---|---|---|---|---|
| 1 | glossary body (cols 90-91) | ~~The `VerticalScroll` scrollbar is not ported~~ **Closed**: the scrollbar is ported (track `#3a3a3a`, thumb `$foreground`, half-block `▄` cap when the proportional thumb position rounds up) and the glossary gates Tier A at 120x40 | widget gap | resolved |

The static painter oracle has scenarios for the 3-size matrix (80x24, 120x40,
200x50 — default, range-cycled, narrow, glossary, home, research, theses,
ask, decisions, settings). The harness compares character, fg, bg, bold,
reverse, italic and underline. The full static matrix and footer check pass.
The glossary is Tier B at 80x24/200x50 per the manifest.


Only populated Settings is currently compared cell by cell at all three sizes
(`tests/settings_live.rs`). Populated Research, Theses, Ask and Decisions, plus
dialogs and other live focus states, still need comparison; static painter
results do not approve live deviations.
