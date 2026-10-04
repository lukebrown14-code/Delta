# R3.2 — Decisions

- **Branch:** `rewrite/r3-decisions`
- **Runs:** parallel, after R3.1
- **Findings:** `docs/rewrite/findings/rust-screens.md`

## Owns

`crates/delta-tui/src/screens/decisions.rs`, `tests/golden_scenarios/decisions.py`.

## Python sources

`delta/tui/screens/decisions.py` (`Decisions`, `DecisionForm`, `ReviewForm`).
Python tests: `test_decisions_tui.py`. Uses `decisions.rs` and `theses.rs` from R3.1b.

## Work

- Screen bindings as Python: `n` new, `e` edit, `d` delete (with `y` confirm),
  `r` review, `o` open research, `/` filter, `ctrl+s` save, `esc`.
- `DecisionForm` and `ReviewForm` on the R3.1a form framework; thesis link and
  relink; recent closes in the detail pane.

## Golden scenarios

List, empty, detail with review history, new form, edit form, review form,
validation error, delete confirm, filter active.

## Done

- [ ] CRUD and reviews from Rust read back identically in Python.
- [ ] All scenarios green at 3 sizes.
