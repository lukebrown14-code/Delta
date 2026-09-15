"""Watch targets: the things a user follows, not just shares.

A target is a company, sector, industry, market, or theme. Company, sector,
industry, and theme targets name tickers in one market; a market target names
only the market and contributes no instruments yet. Legacy ``[watchlists]``
tables keep working: their kind is inferred from shape when modelled.
"""

from __future__ import annotations

from typing import Any, Literal, cast

from pydantic import BaseModel

TargetKind = Literal["company", "sector", "industry", "market", "theme"]

#: Sorted for stable "known kinds are ..." error messages.
KNOWN_KINDS: tuple[str, ...] = ("company", "industry", "market", "sector", "theme")

#: Kind assumed for a ``[targets.<name>]`` table that omits ``kind``.
DEFAULT_KIND = "company"

#: Kind of legacy ``[watchlists.<name>]`` tables in the plugin registry.
LEGACY_KIND = "tickers"


class WatchTarget(BaseModel):
    """A thing the user follows, with the tickers that trade it (if any)."""

    id: str
    kind: TargetKind
    name: str
    markets: tuple[str, ...] = ()
    tickers: tuple[str, ...] = ()
    tags: frozenset[str] = frozenset()
    notes: str = ""


def _kind_of(name: str, spec: dict[str, Any], *, legacy: bool = False) -> TargetKind:
    raw = str(spec.get("kind", "")).lower()
    if raw in ("", LEGACY_KIND) or (legacy and raw not in KNOWN_KINDS):
        tickers = [t for t in spec.get("tickers", []) if t]
        return "company" if len(tickers) == 1 else "theme"
    if raw not in KNOWN_KINDS:
        raise ValueError(
            f"target {name!r} names unknown kind {raw!r}; known kinds are {', '.join(KNOWN_KINDS)}"
        )
    return cast(TargetKind, raw)


def target_from_spec(name: str, spec: dict[str, Any], *, legacy: bool = False) -> WatchTarget:
    """Build the domain model from a ``[targets.<name>]`` (or legacy) table.

    ``legacy`` marks a ``[watchlists.<name>]`` table. Its ``kind`` may name a
    third-party kind from the ``rigger.watchlists`` entry-point group, which is
    still honoured, so such a kind is modelled by shape rather than rejected.
    """
    market = str(spec.get("market", "")).lower()
    return WatchTarget(
        id=name,
        kind=_kind_of(name, spec, legacy=legacy),
        name=str(spec.get("label", name)),
        markets=(market,) if market else (),
        tickers=tuple(str(t).upper() for t in spec.get("tickers", []) if t),
        tags=frozenset(str(tag) for tag in spec.get("tags", []) if tag),
        notes=str(spec.get("notes", "")),
    )
