"""Research screen scenarios (pane 3)."""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = matrix("research", ("3",), "A")

# Populated report and evidence from the shared golden seed.
POPULATED_SCENARIOS = (
    matrix("live-research", ("3",), "B")
    + matrix("live-research-search", ("3", "/"), "B")
    + matrix("live-research-citation", ("3", "r"), "B")
)
