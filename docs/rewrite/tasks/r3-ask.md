# R3.2 — Ask + grounded chat

- **Branch:** `rewrite/r3-ask`
- **Runs:** parallel, after R3.1
- **Findings:** `docs/rewrite/findings/rust-services.md` (chat), `rust-screens.md` (screen)

## Owns

`crates/delta-services/src/chat.rs`, `crates/delta-tui/src/screens/ask.rs`,
`tests/golden_scenarios/ask.py`.

## Python sources

`delta/chat.py` (`ChatDraft`, `ChatMessage`, `SearchTool`, the chat prompt),
`delta/tui/screens/chat.py`. Python tests: `test_chat.py`.

## Work

- `chat.rs`: tool-use search over evidence, answer drafting, citation
  machine-check against evidence ids, message history persistence.
- Screen bindings as Python: `enter` ask, `y` confirm, `←`/`→` citation, plus
  pane keys `i`, `↑↓`, `z`, target toggles `space`/`a`.
- Multi-line input from R3.1a; answer body via `DeltaMarkdown`.

## Provenance (never weaken)

An answer citing an id outside the gathered set fails; prompt text copied verbatim.

## Golden scenarios

Empty, populated history, typing, waiting, answered with citations, citation
selected, confirm prompt, error.

## Done

- [ ] Same FakeLLM script gives the same stored messages and citations as Python.
- [ ] All scenarios green (answers Tier B, chrome Tier A) at 3 sizes.
