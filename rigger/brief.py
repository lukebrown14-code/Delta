"""Facts-only instrument brief assembled from the database.

Every Phase 2 workstream either feeds one of these sections (by writing to its
table) or consumes the rendered brief (strategies). Each section reads exactly
one table so new data sources never need to touch this module.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta

from sqlmodel import Session, select

from rigger.core.db import BarTable, EventTable, FundamentalTable, NewsItemTable
from rigger.core.json import from_json
from rigger.core.models import Instrument
from rigger.core.plugin import Context
from rigger.core.time import to_utc

NEWS_WINDOW_DAYS = 14
EVENT_WINDOW_DAYS = 14
NEWS_LIMIT = 20
BAR_WINDOW = 120
EVIDENCE_BARS = 60


@dataclass
class Section:
    title: str
    lines: list[str] = field(default_factory=list)
    evidence_ids: list[str] = field(default_factory=list)

    def render(self) -> str:
        if not self.lines:
            return f"{self.title}: none available"
        return "\n".join(self.lines)


@dataclass
class Brief:
    instrument: Instrument
    as_of: datetime
    prices: Section
    news: Section
    events: Section
    fundamentals: Section
    calendar: Section

    @property
    def sections(self) -> tuple[Section, ...]:
        # Price lines first with no title, matching the Phase 1 brief exactly.
        return (self.prices, self.fundamentals, self.events, self.calendar, self.news)

    def render(self) -> str:
        return "\n".join(section.render() for section in self.sections)

    @property
    def evidence_ids(self) -> list[str]:
        """Ordered union of every section's evidence ids."""
        return list(dict.fromkeys(eid for s in self.sections for eid in s.evidence_ids))


def build_brief(
    ctx: Context, instrument: Instrument, as_of: datetime | None = None
) -> Brief | None:
    """Assemble a brief for one instrument. Returns None when there are no bars."""
    as_of = as_of or datetime.now(UTC)
    with Session(ctx.engine) as session:
        prices = _prices(session, instrument, as_of)
        if prices is None:
            return None
        return Brief(
            instrument=instrument,
            as_of=as_of,
            prices=prices,
            news=_news(session, instrument, as_of),
            events=_events(session, instrument, as_of),
            fundamentals=_fundamentals(session, instrument, as_of),
            calendar=_calendar(session, instrument, as_of),
        )


def _prices(session: Session, inst: Instrument, as_of: datetime) -> Section | None:
    rows = session.exec(
        select(BarTable)
        .where(BarTable.instrument_id == inst.id)
        .where(BarTable.ts <= as_of)
        .order_by(BarTable.ts.desc())  # type: ignore[attr-defined]
        .limit(BAR_WINDOW)
    ).all()
    if not rows:
        return None

    rows = list(reversed(rows))
    closes = [r.close for r in rows]
    evidence_ids = [str(r.id) for r in rows[-EVIDENCE_BARS:]]

    latest = rows[-1].close
    change_20 = (latest / closes[-21] - 1) * 100 if len(closes) > 21 else 0.0
    sma20 = sum(closes[-20:]) / min(20, len(closes))
    sma50 = sum(closes[-50:]) / min(50, len(closes))
    momentum = (latest / closes[0] - 1) * 100 if len(closes) > 1 else 0.0
    high = max(closes[-EVIDENCE_BARS:])
    low = min(closes[-EVIDENCE_BARS:])

    lines = [
        f"Latest close: {latest:.2f} {inst.currency}",
        f"20-day return: {change_20:+.2f}%",
        f"Return over window: {momentum:+.2f}%",
        f"20-day SMA: {sma20:.2f}, 50-day SMA: {sma50:.2f}",
        f"60-day range: {low:.2f} - {high:.2f}",
    ]
    if inst.sector:
        lines.append(f"Sector: {inst.sector}")
    return Section(title="Prices", lines=lines, evidence_ids=evidence_ids)


def _news(session: Session, inst: Instrument, as_of: datetime) -> Section:
    since = as_of - timedelta(days=NEWS_WINDOW_DAYS)
    rows = session.exec(
        select(NewsItemTable)
        .where(NewsItemTable.published >= since)
        .where(NewsItemTable.published <= as_of)
        .order_by(NewsItemTable.published.desc())  # type: ignore[attr-defined]
    ).all()
    section = Section(title="News")
    for r in rows:
        if inst.id not in from_json(r.instrument_ids):
            continue
        section.lines.append(f"{to_utc(r.published):%Y-%m-%d} [{r.source}] {r.title}")
        section.evidence_ids.append(r.id)
        if len(section.lines) >= NEWS_LIMIT:
            break
    return section


def _events(session: Session, inst: Instrument, as_of: datetime) -> Section:
    since = as_of - timedelta(days=EVENT_WINDOW_DAYS)
    rows = session.exec(
        select(EventTable)
        .where(EventTable.instrument_id == inst.id)
        .where(EventTable.ts >= since)
        .where(EventTable.ts <= as_of)
        .order_by(EventTable.ts.desc())  # type: ignore[attr-defined]
    ).all()
    section = Section(title="Events")
    for r in rows:
        section.lines.append(
            f"{to_utc(r.ts):%Y-%m-%d} {r.kind}: {r.summary} (sentiment {r.sentiment:+.1f})"
        )
        section.evidence_ids.append(r.id)
    return section


def _fundamentals(session: Session, inst: Instrument, as_of: datetime) -> Section:
    rows = session.exec(
        select(FundamentalTable)
        .where(FundamentalTable.instrument_id == inst.id)
        .where(FundamentalTable.as_of <= as_of.date())
        .order_by(FundamentalTable.as_of.desc())  # type: ignore[attr-defined]
    ).all()
    section = Section(title="Fundamentals")
    seen: set[str] = set()
    for r in rows:
        if r.metric in seen:
            continue
        seen.add(r.metric)
        section.lines.append(f"{r.metric}: {r.value:g} (as of {r.as_of:%Y-%m-%d}, {r.source})")
        section.evidence_ids.append(f"fundamental:{r.id}")
    return section


def _calendar(session: Session, inst: Instrument, as_of: datetime) -> Section:
    rows = session.exec(
        select(EventTable)
        .where(EventTable.instrument_id == inst.id)
        .where(EventTable.ts > as_of)
        .order_by(EventTable.ts.asc())  # type: ignore[attr-defined]
    ).all()
    section = Section(title="Upcoming events")
    for r in rows:
        section.lines.append(f"{to_utc(r.ts):%Y-%m-%d} {r.kind}: {r.summary}")
        section.evidence_ids.append(r.id)
    return section
