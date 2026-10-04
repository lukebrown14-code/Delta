# R3.0 — Gates, file split, shared seed

- **Branch:** `rewrite/r3-gates`
- **Runs:** serial, before everything else
- **Findings:** `docs/rewrite/findings/rust-screens.md`

## Owns

- `.github/workflows/ci.yml`
- `crates/delta-tui/src/screens.rs` → split into `crates/delta-tui/src/screens/`
- `crates/delta-tui/src/main.rs` (move the app struct, worker startup and the quotes switch into `app.rs`; `main.rs` keeps arg parsing and terminal setup only)
- `crates/delta-services/src/lib.rs` (module declarations)
- `tests/export_golden.py`, new `tests/golden_scenarios/`, new `tests/golden_seed.py`
- `fixtures/golden_screens/`, new `fixtures/golden_seed.db`, new `fixtures/llm/`
- `crates/delta-tui/tests/golden.rs`

## Work

1. **CI:** add a Rust job to `ci.yml` alongside the Python job: fmt check, clippy
   `-D warnings`, `cargo test --workspace`. Cache `~/.cargo` and `target/`.
2. **Split `screens.rs`** into `screens/{mod,status_bar,home,watchlist,research,theses,ask,decisions,settings}.rs`.
   This is a move only, with no behaviour change; all 28 goldens stay green.
3. **Declare empty service modules** in `delta-services/src/lib.rs` so R3.1/R3.2
   streams own whole files: `evidence`, `theses`, `decisions`, `review`,
   `setup`, `reports`, `chat`, `thesis_summary`.
4. **Split the exporter's scenario list** into per-screen modules
   `tests/golden_scenarios/<screen>.py`. `export_golden.py` collects them.
5. **Shared populated seed** (`tests/golden_seed.py` → `fixtures/golden_seed.db`):
   - watchlist instruments with bars, news, events and fundamentals
   - one generated report per research target (with sidecar)
   - chat history
   - two theses with evidence and health
   - decisions with a due review
   - LLM call rows for costs
   Frozen clock 2026-09-21, the same as the existing goldens. The Rust harness loads this DB file
   directly instead of rebuilding the data in Rust test code.
6. **FakeLLM fixtures:** canned responses under `fixtures/llm/<task>/`, loaded by
   both the Python `FakeLLM` and the Rust `FakeLlm`.
7. **Golden harness:** read the scenario tier and seed from `manifest.json`; keep
   the side-by-side diff on failure.

## Done

- [ ] CI runs and gates the Rust job on the PR.
- [ ] `screens/` split merged; 28/28 goldens still Tier A green.
- [ ] Empty service modules declared.
- [ ] `golden_seed.db` + `fixtures/llm/` committed; the seed script is deterministic (re-running gives a byte-identical DB, or the test compares contents).
- [ ] Rust harness loads `golden_seed.db`; Python suite green.
