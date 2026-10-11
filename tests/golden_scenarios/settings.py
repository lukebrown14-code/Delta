"""Settings screen scenarios (pane c)."""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = matrix("settings", ("c",), "A")

#: Populated states: the exporter boots the app over a copy of the shared
#: seed DB (``fixtures/golden_seed.db``) with a full config, and the Rust
#: live harness renders the same state from the same DB (see
#: ``crates/delta-tui/tests/live_golden.rs``). Each fixture embeds the seed
#: block (config, frozen clock, pinned sizes) the DB cannot hold.
POPULATED_SCENARIOS = matrix("live-settings", ("c",), "A")
