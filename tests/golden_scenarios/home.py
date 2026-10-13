"""Home screen scenarios (pane 1)."""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = matrix("home", (), "A")

POPULATED_SCENARIOS = matrix("live-home", (), "A") + matrix("live-home-first-run", (), "A")
