"""Decisions screen scenarios (pane 6)."""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = matrix("decisions", ("6",), "A")

POPULATED_SCENARIOS = matrix("live-decisions", ("6",), "A") + matrix(
    "live-decisions-review", ("6", "down"), "A"
) + matrix("live-decisions-confirm", ("6", "d"), "A") + matrix(
    "live-decisions-review-form", ("6", "r"), "A"
)
