"""Ask (chat) screen scenarios (pane 5)."""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = matrix("ask", ("5",), "A")
POPULATED_SCENARIOS = matrix("live-ask", ("5",), "B")
POPULATED_SCENARIOS += tuple(
    scenario
    for state in (
        "typing",
        "waiting",
        "answered",
        "citation-selected",
        "confirm",
        "error",
    )
    for scenario in matrix(f"live-ask-{state}", ("5",), "B")
)
