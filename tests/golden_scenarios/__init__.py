"""Golden scenarios, one module per screen (collected by ``export_golden.py``).

Each module owns its screen's scenario list: the key states, the three-size
matrix (80x24, 120x40, 200x50) and the declared tier. ``SCENARIOS`` here is
the ordered concatenation, which is exactly the pre-split list, so the
committed goldens stay byte-identical.
"""

from __future__ import annotations

from dataclasses import dataclass

#: The full three-size matrix (R4 gates on 120x40 only during R3).
SIZES: tuple[tuple[int, int], ...] = ((80, 24), (120, 40), (200, 50))


@dataclass(frozen=True)
class Scenario:
    name: str
    size: tuple[int, int]
    tier: str  # declared: "A" (status bar, chart, tables) or "B" (prose)
    keys: tuple[str, ...]

    @property
    def size_label(self) -> str:
        return f"{self.size[0]}x{self.size[1]}"

    @property
    def filename(self) -> str:
        return f"{self.name}-{self.size_label}.json"

    @property
    def gate_tier(self) -> str:
        """The tier the harness enforces; Research report prose is Tier B."""
        return "A" if self.size == (120, 40) and not self.name.startswith("live-research") else self.tier


def matrix(name: str, keys: tuple[str, ...], tier: str) -> tuple[Scenario, ...]:
    """One state exported at every matrix size."""
    return tuple(Scenario(name, size, tier, keys) for size in SIZES)


from . import (  # noqa: E402
    ask,
    decisions,
    home,
    research,
    settings,
    shell,
    theses,
    watchlist,
)

#: All scenarios in export order (the manifest and file order follow this).
SCENARIOS: tuple[Scenario, ...] = (
    home.SCENARIOS
    + watchlist.SCENARIOS
    + research.SCENARIOS
    + theses.SCENARIOS
    + ask.SCENARIOS
    + decisions.SCENARIOS
    + settings.SCENARIOS
    + shell.SCENARIOS
)

#: Populated scenarios (``live-*``): exported against the shared seed DB and
#: listed in ``live-manifest.json``; each screen module appends its own.
LIVE_SCENARIOS: tuple[Scenario, ...] = (
    settings.POPULATED_SCENARIOS
    + theses.POPULATED_SCENARIOS
    + research.POPULATED_SCENARIOS
    + ask.POPULATED_SCENARIOS
)
