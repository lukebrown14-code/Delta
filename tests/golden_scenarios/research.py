"""Research screen scenarios (pane 3)."""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = matrix("research", ("3",), "A")

# Populated report and evidence from the shared golden seed.
POPULATED_SCENARIOS = (
    matrix("live-research", ("3",), "B")
    + matrix("live-research-search", ("3", "/"), "B")
    + matrix("live-research-citation", ("3", "r"), "B")
    + matrix("live-research-no-report", ("3",), "B")
    + matrix("live-research-kind-news", ("3", "k"), "B")
    + matrix("live-research-zoom-report", ("3", "r", "z"), "B")
    + matrix("live-research-zoom-evidence", ("3", "e", "z"), "B")
    + matrix("live-research-generating", ("3",), "B")
    + matrix("live-research-error", ("3",), "B")
)
