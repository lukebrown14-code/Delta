# R3.2 — Theses

- **Branch:** `rewrite/r3-theses`
- **Runs:** parallel, after R3.1
- **Findings:** `docs/rewrite/findings/rust-screens.md`

## Owns

`crates/delta-services/src/thesis_summary.rs`, `crates/delta-tui/src/screens/theses.rs`,
`tests/golden_scenarios/theses.py`.

## Python sources

`delta/thesis_summary.py` (prompt `thesis_summary_v1.j2`, `thesis_v1.j2`),
`delta/tui/screens/theses.py`, `thesis_form.py` (`ThesisForm`), `thesis_panes.py`.
Python tests: `test_thesis_summary.py`, theses parts of `test_tui.py`.
Uses `theses.rs`, `evidence.rs` and `thesis_health.rs` from R3.1b.

## Work

- Screen bindings as Python: `n` new, `d` edit, `f` find, `s` summarise, `t`
  thesis, `e` evidence, `a` accept, `x` reject, `u` un-accept, `/` filter,
  `shift+↑/↓` scroll note, `esc`.
- `ThesisForm` on the R3.1a form framework, with Python's validation messages.
- AI summary via `thesis_summary.rs`.

## Golden scenarios

Fleet list, empty, thesis detail, evidence pane, new form, edit form, form
validation error, summary shown, filter active.

## Done

- [ ] Create/edit/accept/reject from Rust reads back identically in Python.
- [ ] Summary output matches Python on the seed + FakeLLM.
- [ ] All scenarios green at 3 sizes.
