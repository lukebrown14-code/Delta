"""Unified read model over the source tables: one pool of cited evidence.

The existing tables (``bar``, ``newsitem``, ``event``, ``fundamental``) keep
their write paths and data plugins untouched; this module flattens them into
``EvidenceItem`` rows that reports and chat read. Web hits are a chat-stage
search tool rather than stored evidence, so they never appear here.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from typing import Any, Literal

from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from rigger.core.db import BarTable, EventTable, FundamentalTable, NewsItemTable
from rigger.core.json import from_json
from rigger.core.time import parse_date, to_utc

EvidenceKind = Literal["bar", "news", "filing", "fundamental", "event", "web", "note"]

FILING_SOURCE = "sec_edgar"


@dataclass
class EvidenceItem:
    """One sourced fact, normalised from any source table."""

    id: str
    target_ids: tuple[str, ...]
    ts: datetime
    kind: EvidenceKind
    title: str
    body: str | None
    source: str
    url: str | None
    sentiment: float | None
    raw: dict[str, Any] = field(default_factory=dict)


def cite(item: EvidenceItem) -> str:
    """Deterministic cite text: ``[source] title <url>``, url omitted when absent."""
    text = f"[{item.source}] {item.title}"
    if item.url is not None:
        text += f" <{item.url}>"
    return text


def evidence(
    engine: Engine,
    *,
    target: str | None = None,
    since: str | None = None,
    kind: str | None = None,
    limit: int = 200,
) -> list[EvidenceItem]:
    """Read evidence across all source tables.

    ``target`` keeps items whose ``target_ids`` contain it; ``since`` is an
    inclusive ``YYYY-MM-DD`` floor on ``ts``; ``kind`` filters the mapped kind.
    Results are ordered ``ts`` desc then ``id`` asc, truncated to ``limit``.
    """
    limit = max(limit, 0)
    with Session(engine) as session:
        items: list[EvidenceItem] = []
        if kind is None or kind == "bar":
            items += _bars(session, target, since, limit)
        if kind is None or kind in ("news", "filing"):
            items += _news(session, target, since, kind, limit)
        if kind is None or kind == "event":
            items += _events(session, target, since, limit)
        if kind is None or kind == "fundamental":
            items += _fundamentals(session, target, since, limit)
    floor = parse_date(since) if since is not None else None
    matched = [
        item
        for item in items
        if (target is None or target in item.target_ids)
        and (floor is None or item.ts >= floor)
        and (kind is None or item.kind == kind)
    ]
    return _ordered(matched)[:limit]


def _ordered(items: list[EvidenceItem]) -> list[EvidenceItem]:
    """Newest first; items sharing a timestamp keep ascending id order."""
    items.sort(key=lambda item: item.id)
    items.sort(key=lambda item: item.ts, reverse=True)
    return items


def _bars(
    session: Session, target: str | None, since: str | None, limit: int
) -> list[EvidenceItem]:
    stmt = select(BarTable)
    if target is not None:
        stmt = stmt.where(BarTable.instrument_id == target)
    if since is not None:
        stmt = stmt.where(BarTable.ts >= parse_date(since))
    stmt = stmt.order_by(BarTable.ts.desc()).limit(limit)  # type: ignore[attr-defined]
    return [_bar_item(row) for row in session.exec(stmt).all()]


def _news(
    session: Session, target: str | None, since: str | None, kind: str | None, limit: int
) -> list[EvidenceItem]:
    stmt = select(NewsItemTable)
    if target is not None:
        stmt = stmt.where(
            NewsItemTable.instrument_ids.contains(f'"{target}"', autoescape=True)  # type: ignore[attr-defined]
        )
    if since is not None:
        stmt = stmt.where(NewsItemTable.published >= parse_date(since))
    if kind == "news":
        stmt = stmt.where(NewsItemTable.source != FILING_SOURCE)
    elif kind == "filing":
        stmt = stmt.where(NewsItemTable.source == FILING_SOURCE)
    stmt = stmt.order_by(NewsItemTable.published.desc()).limit(limit)  # type: ignore[attr-defined]
    return [_news_item(row) for row in session.exec(stmt).all()]


def _events(
    session: Session, target: str | None, since: str | None, limit: int
) -> list[EvidenceItem]:
    stmt = select(EventTable)
    if target is not None:
        stmt = stmt.where(EventTable.instrument_id == target)
    if since is not None:
        stmt = stmt.where(EventTable.ts >= parse_date(since))
    stmt = stmt.order_by(EventTable.ts.desc()).limit(limit)  # type: ignore[attr-defined]
    return [_event_item(row) for row in session.exec(stmt).all()]


def _fundamentals(
    session: Session, target: str | None, since: str | None, limit: int
) -> list[EvidenceItem]:
    stmt = select(FundamentalTable)
    if target is not None:
        stmt = stmt.where(FundamentalTable.instrument_id == target)
    if since is not None:
        stmt = stmt.where(FundamentalTable.as_of >= parse_date(since).date())
    stmt = stmt.order_by(FundamentalTable.as_of.desc()).limit(limit)  # type: ignore[attr-defined]
    return [_fundamental_item(row) for row in session.exec(stmt).all()]


def _bar_item(row: BarTable) -> EvidenceItem:
    return EvidenceItem(
        id=f"bar:{row.id}",
        target_ids=(row.instrument_id,),
        ts=to_utc(row.ts),
        kind="bar",
        title=f"{row.instrument_id} close {row.close:.2f}",
        body=None,
        source=row.source,
        url=None,
        sentiment=None,
        raw={
            "open": row.open,
            "high": row.high,
            "low": row.low,
            "close": row.close,
            "volume": row.volume,
        },
    )


def _news_item(row: NewsItemTable) -> EvidenceItem:
    instrument_ids = from_json(row.instrument_ids)
    kind: EvidenceKind = "filing" if row.source == FILING_SOURCE else "news"
    return EvidenceItem(
        id=f"{kind}:{row.id}",
        target_ids=tuple(instrument_ids),
        ts=to_utc(row.published),
        kind=kind,
        title=row.title,
        body=row.body,
        source=row.source,
        url=row.url,
        sentiment=None,
        raw={"instrument_ids": instrument_ids},
    )


def _event_item(row: EventTable) -> EvidenceItem:
    return EvidenceItem(
        id=f"event:{row.id}",
        target_ids=(row.instrument_id,),
        ts=to_utc(row.ts),
        kind="event",
        title=f"{row.kind}: {row.summary}",
        body=None,
        source=row.extracted_by,
        url=None,
        sentiment=row.sentiment,
        raw={
            "kind": row.kind,
            "summary": row.summary,
            "evidence_ids": from_json(row.evidence_ids),
            "extracted_by": row.extracted_by,
            "prompt_version": row.prompt_version,
        },
    )


def _fundamental_item(row: FundamentalTable) -> EvidenceItem:
    as_of = f"{row.as_of:%Y-%m-%d}"
    return EvidenceItem(
        id=f"fundamental:{row.id}",
        target_ids=(row.instrument_id,),
        ts=parse_date(as_of),
        kind="fundamental",
        title=f"{row.metric}: {row.value:.2f} ({as_of}, {row.source})",
        body=None,
        source=row.source,
        url=None,
        sentiment=None,
        raw={"metric": row.metric, "value": row.value, "as_of": as_of},
    )
