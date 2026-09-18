"""Deterministic thesis health: an evidence-based read, never a truth claim.

The state is computed by a pure function over ACCEPTED evidence — the LLM may
draft prose about it later, but it never decides the state.
"""

from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass, field
from datetime import datetime, timedelta
from typing import Literal

from delta.evidence import EvidenceItem
from delta.theses import EvidenceSide, Thesis

HealthState = Literal["emerging", "building", "mixed", "weakening", "challenged", "idle"]

#: Items at most this old count fully; older ones count half.
RECENT_DAYS = 7

_STYLES: dict[str, str] = {
    "emerging": "cyan",
    "building": "green",
    "mixed": "yellow",
    "weakening": "orange1",
    "challenged": "red",
    "idle": "dim",
}


@dataclass(frozen=True)
class HealthResult:
    state: HealthState
    tilt: float
    support: int
    against: int
    neutral: int
    computed_at: datetime
    drivers: list[str] = field(default_factory=list)


def compute_health(
    thesis: Thesis,
    linked: Sequence[tuple[EvidenceItem, EvidenceSide]],
    *,
    now: datetime,
    stale_days: int = 14,
    min_coverage: int = 3,
) -> HealthResult:
    """Score a thesis over its accepted ``(evidence item, side)`` pairs.

    Rules, in precedence order:

    1. Coverage — fewer than ``min_coverage`` items forces ``emerging``.
    2. Freshness — nothing newer than ``stale_days`` forces ``idle`` (a single
       fresh item prevents idle).
    3. Falsifiers — an item whose kind or title contains a falsifier string
       (case-insensitive) forces ``challenged``.
    4. Balance — support vs against, weighted 1.0 within the last
       ``RECENT_DAYS`` and 0.5 older, mapped to tilt in ``[-1, 1]``:
       ``>= 0.25`` building, ``<= -0.25`` weakening, otherwise mixed.

    ``drivers`` are the evidence ids that moved the result, most important
    first (falsifier hits, then contribution and recency), capped at five.
    """
    support = against = neutral = 0
    weighted_support = 0.0
    weighted_against = 0.0
    freshest: datetime | None = None
    falsified = False
    ranked: list[tuple[float, datetime, str]] = []

    recent_cutoff = now - timedelta(days=RECENT_DAYS)
    stale_cutoff = now - timedelta(days=stale_days)
    falsifiers = [text.lower() for text in thesis.falsifiers if text]

    for item, side in linked:
        if freshest is None or item.ts > freshest:
            freshest = item.ts
        if side == "support":
            support += 1
        elif side == "against":
            against += 1
        else:
            neutral += 1

        weight = 1.0 if item.ts >= recent_cutoff else 0.5
        hit = _falsifier_hit(item, falsifiers)
        falsified = falsified or hit
        if side == "support":
            weighted_support += weight
        elif side == "against":
            weighted_against += weight

        importance = (2.0 if hit else 0.0) + (weight if side in ("support", "against") else 0.0)
        ranked.append((importance, item.ts, item.id))

    total = weighted_support + weighted_against
    tilt = 0.0 if total == 0 else (weighted_support - weighted_against) / total

    if len(linked) < min_coverage:
        state: HealthState = "emerging"
    elif freshest is None or freshest <= stale_cutoff:
        state = "idle"
    elif falsified:
        state = "challenged"
    elif tilt >= 0.25:
        state = "building"
    elif tilt <= -0.25:
        state = "weakening"
    else:
        state = "mixed"

    ranked.sort(key=lambda entry: (-entry[1].timestamp(), entry[2]))
    ranked.sort(key=lambda entry: -entry[0])
    drivers = [evidence_id for _, _, evidence_id in ranked[:5]]
    return HealthResult(state, tilt, support, against, neutral, now, drivers)


def _falsifier_hit(item: EvidenceItem, falsifiers: list[str]) -> bool:
    if not falsifiers:
        return False
    haystack = f"{item.kind} {item.title}".lower()
    return any(text in haystack for text in falsifiers)


def state_style(state: HealthState) -> str:
    """Textual markup colour for a state's badge."""
    return _STYLES[state]


def badge_text(result: HealthResult) -> str:
    """The health badge as the desk shows it.

    State and tilt only: the per-side counts sit beside the evidence ledger as
    its legend, and repeating them in the badge said the same thing twice on
    one screen.
    """
    return f"{result.state} · tilt {result.tilt:+.2f}"
