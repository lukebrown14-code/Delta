"""Turn unprocessed news into structured :class:`Event` rows via the ``extract`` route.

Reads ``NewsItemTable`` rows not yet cited by any ``EventTable.evidence_ids``,
batches them per instrument, asks the extract model for discrete events, and
persists them. The brief's events section then reads them from ``EventTable``.
"""

from __future__ import annotations

import logging
from collections import defaultdict
from datetime import datetime

from pydantic import BaseModel, Field, ValidationError
from sqlmodel import Session, select

from delta.core.db import EventTable, NewsItemTable, store_items
from delta.core.ids import stable_id
from delta.core.json import from_json
from delta.core.models import Event, EventKind
from delta.core.plugin import Context
from delta.core.time import to_utc
from delta.llm.router import model_for

EXTRACT_TEMPLATE = "extract_v1.j2"
PROMPT_VERSION = EXTRACT_TEMPLATE.removesuffix(".j2")

log = logging.getLogger(__name__)


class EventDraft(BaseModel):
    kind: EventKind
    summary: str
    sentiment: float = Field(ge=-1, le=1)
    evidence_ids: list[str] = Field(default_factory=list)


class EventBatch(BaseModel):
    events: list[EventDraft] = Field(default_factory=list)


def event_id(instrument_id: str, kind: str, summary: str) -> str:
    return stable_id(instrument_id, kind, summary)


def _uncovered_items(session: Session, since: datetime) -> tuple[list[NewsItemTable], set[str]]:
    """News not yet cited by any event, plus the ids of all existing events."""
    covered: set[str] = set()
    existing_ids: set[str] = set()
    for eid, evidence in session.exec(select(EventTable.id, EventTable.evidence_ids)).all():
        existing_ids.add(eid)
        covered.update(from_json(evidence))
    rows = session.exec(
        select(NewsItemTable)
        .where(NewsItemTable.published >= since)
        .order_by(NewsItemTable.published.asc())  # type: ignore[attr-defined]
    ).all()
    return [r for r in rows if r.id not in covered], existing_ids


async def extract_events(ctx: Context, since: datetime, batch_size: int = 20) -> list[Event]:
    """Extract and persist events from news published on or after ``since``.

    Returns the newly stored events. Items already cited by an event are not
    re-sent; items mapped to no instrument are skipped for now.
    """
    from delta.llm import structured as structured_mod

    model = model_for(ctx.config, "extract")
    symbols = {inst.id: inst.symbol for inst in ctx.universe}

    with Session(ctx.engine) as session:
        items, existing_ids = _uncovered_items(session, since)

    by_instrument: dict[str, list[NewsItemTable]] = defaultdict(list)
    for item in items:
        for instrument_id in from_json(item.instrument_ids):
            by_instrument[instrument_id].append(item)

    stored: list[Event] = []
    for instrument_id, rows in by_instrument.items():
        symbol = symbols.get(instrument_id, instrument_id.split(":", 1)[-1])
        for start in range(0, len(rows), batch_size):
            batch = rows[start : start + batch_size]
            by_id = {r.id: r for r in batch}
            try:
                draft, _result = await structured_mod.structured(
                    ctx.llm,
                    task="extract",
                    model=model,
                    template=EXTRACT_TEMPLATE,
                    vars={
                        "symbol": symbol,
                        "instrument_id": instrument_id,
                        "items": [
                            {
                                "id": r.id,
                                "published": to_utc(r.published).isoformat(),
                                "source": r.source,
                                "title": r.title,
                                "body": r.body or "",
                            }
                            for r in batch
                        ],
                    },
                    schema=EventBatch,
                )
            except ValidationError:
                log.exception(
                    "invalid extract output for %s from %s; skipping batch", instrument_id, model
                )
                continue

            new_events: list[Event] = []
            for ev in draft.events:
                evidence = [e for e in ev.evidence_ids if e in by_id]
                if not evidence:
                    # An event must be traceable to the items it was extracted from.
                    log.warning("event for %s cites no provided items; dropped", instrument_id)
                    continue
                eid = event_id(instrument_id, ev.kind, ev.summary)
                if eid in existing_ids:
                    continue
                existing_ids.add(eid)
                new_events.append(
                    Event(
                        id=eid,
                        instrument_id=instrument_id,
                        ts=max(to_utc(by_id[e].published) for e in evidence),
                        kind=ev.kind,
                        summary=ev.summary,
                        sentiment=ev.sentiment,
                        evidence_ids=evidence,
                        extracted_by=model,
                        prompt_version=PROMPT_VERSION,
                    )
                )
            # Persist per batch so a failure later in the run keeps what was
            # already paid for.
            if new_events:
                store_items(ctx.engine, new_events)
                stored.extend(new_events)

    return stored
