"""Theses screen scenarios (pane 4)."""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = matrix("theses", ("4",), "A")
POPULATED_SCENARIOS = matrix("live-theses", ("4",), "A")
POPULATED_SCENARIOS += matrix("live-theses-empty", ("4",), "A")
POPULATED_SCENARIOS += matrix("live-theses-detail", ("4", "t"), "A")
POPULATED_SCENARIOS += matrix("live-theses-evidence", ("4", "e"), "A")
POPULATED_SCENARIOS += matrix("live-theses-new", ("4", "n"), "A")
POPULATED_SCENARIOS += matrix("live-theses-edit", ("4", "d"), "A")
POPULATED_SCENARIOS += matrix("live-theses-validation", ("4", "n", "enter"), "A")
POPULATED_SCENARIOS += matrix("live-theses-filter", ("4", "/", "a"), "A")
POPULATED_SCENARIOS += matrix("live-theses-summary", ("4", "s"), "A")
