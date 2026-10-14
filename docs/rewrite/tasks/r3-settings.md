# R3.2 — Settings + pickers

- **Branch:** `rewrite/r3-settings`
- **Runs:** parallel, after R3.1
- **Findings:** `docs/rewrite/findings/rust-screens.md`

## Owns

`crates/delta-tui/src/screens/settings.rs`, new `crates/delta-tui/src/screens/{provider_picker,model_picker,source_setup,market_setup}.rs`,
`tests/golden_scenarios/settings.py`.

## Python sources

`delta/tui/screens/config.py`, `provider_picker.py` (`ProviderPicker`,
`KeyEntryModal`, `CustomFormModal`), `model_picker.py`, `source_setup.py`,
`market_setup.py`. Python tests: `test_provider_setup.py`, `test_market_setup.py`,
`test_catalog.py` (picker parts). Uses `setup.rs` (R3.1b), `config_ops.rs`,
`delta-llm` `catalog`/`providers` (`verify_key`).

## Work

- Settings bindings as Python: `d` diagnostics, `r` refresh, `l` plugins,
  `s` configure source, `a`/`e`/`x` add/edit/remove market, `esc`.
- Provider picker with key entry (masked; `.env` write) and custom endpoint form.
- Model picker with catalog, fuzzy filter, `ctrl+r` refresh. Opened app-wide by
  `m`/`p` (bindings wired in R3.1a).
- Source and market setup modals with Python's validation messages.

## Golden scenarios

Settings populated, diagnostics open, each modal open (provider, key entry,
custom, model, source, market), validation errors, key verify failure.

## Done

- [ ] `config.toml`/`.env` written by Rust load in Python, and the reverse; comments preserved (D6).
- [ ] No secret is ever drawn unmasked or logged.
- [ ] All scenarios green at 3 sizes.
