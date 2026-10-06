"""Watchlist screen scenarios (pane 2): inspector, range cycling, glossary.

Scenario note: the plan's ``scrubbed`` state assumes the K9 chart scrub from
docs/UI_UX_AUDIT.md, which is **not implemented** in the Python app (no scrub
key exists in ``delta/tui/screens/targets.py``). The closest implemented
inspector state — the glossary modal (``i``) — is exported instead, under the
name ``glossary``, Tier B (prose). When K9 lands, add the scrub state here.
"""

from __future__ import annotations

from golden_scenarios import Scenario, matrix

SCENARIOS = (
    matrix("default", ("2", "enter"), "A")
    + matrix("range-cycled", ("2", "enter", "r"), "A")
    + matrix("glossary", ("2", "enter", "i"), "B")
    + (Scenario("narrow", (80, 24), "A", ("2", "enter")),)
)
