# Fresh schema; Python parity retired

V2 starts with a fresh database and versioned migrations. It doesn't open V1 databases or migrate their data. The Python parity scaffolding (golden-screen oracle, Python reference fixtures, parity tests) is retired. TUI screens are tested with Rust-native snapshots on the existing cell-grid screen model.

The V1 schema has no instruments table, no Spaces and Python-specific storage formats, and the screens it was checked against no longer exist. Parity with a frozen Python app protected the cutover; it has nothing to say about V2 screens. Supersedes the parity-gate parts of [0001](0001-canonical-rust-line.md). See [Audit](https://github.com/lukebrown14-code/Delta/issues/59).
