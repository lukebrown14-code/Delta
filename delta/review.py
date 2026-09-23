"""Evidence-quality audits and a deterministic, non-advisory review queue.

The module is deliberately read-only.  It turns stored facts and the user's
active thesis falsifiers into things worth reviewing; it never scores a
security or suggests an action.
"""

from __future__ import annotations

from collections.abc import Iterable, Sequence
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from typing import Any, Literal

from sqlalchemy.engine import Engine

from delta.evidence import PRIMARY_FILING_SOURCES, EvidenceItem, evidence, falsifier_hit

PRICE_STALE_AFTER = timedelta(days=3)
NEWS_STALE_AFTER = timedelta(days=14)
PRIMARY_STALE_AFTER = timedelta(days=30)
EVIDENCE_WINDOW = timedelta(days=30)
MIN_NON_PRICE_ITEMS = 3
MIN_SOURCES = 2

PrimaryCoverage = Literal["not_configured", "absent", "fresh", "stale"]
ReviewKind = Literal["primary_disclosure", "falsifier", "stale", "thin_evidence"]


@dataclass(frozen=True)
class EvidenceAudit:
    instrument_id: str
    price_at: datetime | None
    news_at: datetime | None
    primary_at: datetime | None
    primary_coverage: PrimaryCoverage
    non_price_items: int
    source_count: int
    warnings: tuple[str, ...]


@dataclass(frozen=True)
class ReviewItem:
    """One navigable reason to inspect evidence, ranked by ``review_queue``."""

    kind: ReviewKind
    instrument_id: str
    title: str
    detail: str
    evidence_id: str | None = None
    thesis_id: str | None = None
    ts: datetime | None = None


def primary_sources_for(delta: Any, instrument_id: str) -> frozenset[str]:
    """Enabled primary sources that actually apply to an instrument.

    A source which is merely installed is not enough: disabled plugins and a
    US-only source for an ASX instrument must not make coverage look missing.
    The fallback market mapping supports the old plugin objects which did not
    expose a ``market`` attribute in tests or third-party integrations.
    """
    inst = next((item for item in delta.universe() if item.id == instrument_id), None)
    if inst is None:
        return frozenset()
    return _primary_sources(delta.plugins, inst)


def _primary_sources(plugins: Any, inst: Any) -> frozenset[str]:
    """Core of :func:`primary_sources_for` without the ``delta.universe()`` lookup.

    Lets a caller that already holds the instrument (and has a plugins map handy)
    skip rebuilding the universe, which the review queue does once per call.
    """
    fallback_markets = {"sec_edgar": "us", "asx_announcements": "asx"}
    configured: set[str] = set()
    for name, plugin in plugins.items():
        if name not in PRIMARY_FILING_SOURCES or not getattr(plugin, "enabled", False):
            continue
        market = getattr(plugin, "market", fallback_markets.get(name))
        if market is None or str(market).lower() == inst.market.lower():
            configured.add(name)
    return frozenset(configured)


def evidence_audit(
    engine: Engine,
    instrument_id: str,
    *,
    now: datetime | None = None,
    primary_sources: Iterable[str] = (),
    items: Sequence[EvidenceItem] | None = None,
) -> EvidenceAudit:
    """Summarise recency and diversity without inferring an exchange schedule.

    ``items`` lets a caller that has already fetched the pool (``review_queue``)
    hand it in, avoiding a second full read per instrument.
    """
    now = _now(now)
    primary_sources = frozenset(primary_sources)
    if items is None:
        items = evidence(engine, target=instrument_id, limit=10_000)
    prices = [item for item in items if item.kind == "bar" and item.ts <= now]
    news = [item for item in items if item.kind == "news" and item.ts <= now]
    primary = [
        item
        for item in items
        if item.kind == "filing" and item.ts <= now and item.source in primary_sources
    ]
    recent = [
        item for item in items if item.kind != "bar" and now - EVIDENCE_WINDOW <= item.ts <= now
    ]
    price_at = _latest(prices)
    news_at = _latest(news)
    primary_at = _latest(primary)
    if not primary_sources:
        coverage: PrimaryCoverage = "not_configured"
    elif primary_at is None:
        coverage = "absent"
    elif now - primary_at > PRIMARY_STALE_AFTER:
        coverage = "stale"
    else:
        coverage = "fresh"
    warnings: list[str] = []
    _append_stale(warnings, "price", price_at, now, PRICE_STALE_AFTER)
    _append_stale(warnings, "news", news_at, now, NEWS_STALE_AFTER)
    if coverage == "not_configured":
        warnings.append("no primary disclosure source configured")
    elif coverage == "absent":
        warnings.append("no primary disclosure found")
    elif coverage == "stale":
        warnings.append(f"latest primary disclosure is {_days(now - primary_at)} days old")
    if len(recent) < MIN_NON_PRICE_ITEMS:
        warnings.append("thin recent evidence")
    source_count = len({item.source for item in recent})
    if source_count < MIN_SOURCES:
        warnings.append("low source diversity")
    return EvidenceAudit(
        instrument_id, price_at, news_at, primary_at, coverage, len(recent), source_count, tuple(warnings)
    )


def review_queue(
    delta: Any,
    *,
    instrument_ids: Sequence[str] = (),
    since: datetime | None = None,
    now: datetime | None = None,
) -> list[ReviewItem]:
    """Return a stable priority queue of factual review prompts.

    ``since`` controls the "new disclosure" window.  A caller passes the
    session's captured last-seen value, rather than this service mutating it.

    One pass over each instrument's evidence and a single memoised universe /
    source map: the previous implementation fetched the pool twice per
    instrument (once here, once inside :func:`evidence_audit`) and re-built
    ``delta.universe()`` on every loop iteration.
    """
    now = _now(now)
    universe = delta.universe()
    ids = tuple(instrument_ids) or tuple(item.id for item in universe)
    since = _as_utc(since) if since is not None else now - timedelta(days=7)
    sources: dict[str, frozenset[str]] = {
        inst.id: _primary_sources(delta.plugins, inst) for inst in universe
    }
    primary_items: list[ReviewItem] = []
    falsifiers: list[ReviewItem] = []
    stale: list[ReviewItem] = []
    thin: list[ReviewItem] = []
    active = _active_theses(delta.engine)
    for instrument_id in ids:
        items = evidence(delta.engine, target=instrument_id, limit=10_000)
        primary_sources = sources.get(instrument_id, frozenset())
        for item in items:
            if item.kind == "filing" and item.source in primary_sources and since <= item.ts <= now:
                primary_items.append(_item("primary_disclosure", instrument_id, item))
            if since <= item.ts <= now:
                for thesis_id, terms in active:
                    if (not terms[0] or instrument_id in terms[0]) and falsifier_hit(item, terms[1]):
                        falsifiers.append(_item("falsifier", instrument_id, item, thesis_id=thesis_id))
        audit = evidence_audit(
            delta.engine, instrument_id, now=now, primary_sources=primary_sources, items=items
        )
        stale_warnings = tuple(w for w in audit.warnings if w.startswith(("no primary", "latest", "no price", "latest price", "no news", "latest news")))
        if stale_warnings:
            stale.append(ReviewItem("stale", instrument_id, "Coverage needs review", "; ".join(stale_warnings)))
        thin_warnings = tuple(w for w in audit.warnings if w in {"thin recent evidence", "low source diversity"})
        if thin_warnings:
            thin.append(ReviewItem("thin_evidence", instrument_id, "Evidence coverage is thin", "; ".join(thin_warnings)))
    return _dedupe_and_sort(primary_items, falsifiers, stale, thin)


def _active_theses(engine: Engine) -> list[tuple[str, tuple[tuple[str, ...], tuple[str, ...]]]]:
    from delta.theses import list_theses

    return [
        (thesis.id, (thesis.targets, tuple(term.casefold() for term in thesis.falsifiers if term.strip())))
        for thesis in list_theses(engine)
        if thesis.status == "active" and thesis.falsifiers
    ]


def _item(kind: ReviewKind, instrument_id: str, item: EvidenceItem, *, thesis_id: str | None = None) -> ReviewItem:
    detail = item.source if kind == "primary_disclosure" else "matches an active thesis falsifier"
    return ReviewItem(kind, instrument_id, item.title, detail, item.id, thesis_id, item.ts)


def _dedupe_and_sort(*groups: list[ReviewItem]) -> list[ReviewItem]:
    """Deduplicate exact navigation targets while preserving priority groups."""
    result: list[ReviewItem] = []
    seen: set[tuple[ReviewKind, str, str | None, str | None]] = set()
    for group in groups:
        for item in sorted(group, key=lambda value: (value.ts is None, -(value.ts or datetime.min.replace(tzinfo=UTC)).timestamp(), value.instrument_id, value.evidence_id or "", value.thesis_id or "")):
            key = (item.kind, item.instrument_id, item.evidence_id, item.thesis_id)
            if key not in seen:
                seen.add(key)
                result.append(item)
    return result


def _latest(items: Sequence[EvidenceItem]) -> datetime | None:
    return max((item.ts for item in items), default=None)


def _append_stale(warnings: list[str], label: str, value: datetime | None, now: datetime, threshold: timedelta) -> None:
    if value is None:
        warnings.append(f"no {label} data")
    elif now - value > threshold:
        warnings.append(f"latest {label} is {_days(now - value)} days old")


def _days(age: timedelta) -> int:
    return max(0, age.days)


def _now(value: datetime | None) -> datetime:
    return _as_utc(value or datetime.now(UTC))


def _as_utc(value: datetime) -> datetime:
    return value if value.tzinfo is not None else value.replace(tzinfo=UTC)
