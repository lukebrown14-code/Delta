# R3.1a — TUI foundations

- **Branch:** `rewrite/r3-tui-foundations`
- **Runs:** parallel with R3.1b and R3.1c, after R3.0
- **Findings:** `docs/rewrite/findings/rust-widgets.md`

## Owns

`crates/delta-tui/src/` except `screens/<screen>.rs`: `app.rs`, `lib.rs`,
`components.rs`, `dialog.rs`, `theme.rs`, `table.rs`, new `input.rs`, `form.rs`,
`markdown.rs`, `wrap.rs`, `keymap.rs`, `screens/mod.rs`, `screens/status_bar.rs`.
Also `tests/golden_scenarios/shell.py`, `crates/delta-tui/Cargo.toml`.

## Python sources

`delta/tui/app.py`, `shell.py`, `components.py`, `widgets.py` (chrome),
`theme.py` (dark + light), `screens/help.py`, `delta.tcss`.

## Work

1. **Screen framework:** a `Screen` component trait (state in, `draw(Frame, Rect)`,
   `handle_key → Option<Action>`), breakpoints from width (narrow/normal/wide)
   and a pane/zoom helper. Port Home's chrome first as the reference.
2. **App bindings, matching Python:** `1`–`6`, `c`, `h` home, `m` model picker,
   `p` provider picker, `g` Go, `?` help, `q` quit, `f2` theme, ctrl+k palette.
   Wire `CommandPalette` (nucleo), `WhichKey` footer, help and Go modals.
3. **Inputs:** `tui-textarea` single- and multi-line inputs drawn like Textual's
   `Input`/`TextArea` (cursor, placeholder, focus border).
4. **Forms:** field list, focus order, validation messages under fields,
   submit/cancel, error toast. These are the building blocks for the thesis, decision,
   market, source and provider forms.
5. **`DeltaMarkdown`:** pulldown-cmark plus Textual's heading, list, code-block
   and link rules. Tier B.
6. **Wrap parity:** Rich word-wrap plus `unicode-width` cell widths; dedicated
   wrap-parity tests (emoji, CJK, long words).
7. **Widgets:** scrollbars, OptionList highlight, DataTable scroll state,
   `ActionChip`, `StatusDot`, `Pill`, `KeyGrid`.
8. **Light theme:** generate `delta-light` from the exporter's resolved tokens;
   `f2` toggles it.

## Golden scenarios (`shell.py`)

palette-open, help-open, go-open, home in light theme. Three sizes each.

## Done

- [ ] Framework documented in `agent/codemap/rust.md`; Home chrome rebuilt on it with goldens green.
- [ ] All app bindings match Python `DeltaApp.BINDINGS`.
- [ ] Input, form and markdown components have unit tests and a demo scenario.
- [ ] Wrap-parity tests pass against Rich outputs captured in Python.
- [ ] Shell scenarios Tier A green at 3 sizes; light theme matches.
