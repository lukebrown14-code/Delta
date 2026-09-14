"""yfinance calendar data plugin: upcoming earnings and ex-dividend dates as Event rows.

The brief's calendar section reads ``EventTable`` where ``ts > as_of``, so
these rows surface as "Upcoming events" without any further wiring. Ticker
mapping is shared with the yfinance bars plugin via ``[plugins.<name>].suffixes``.
"""

from __future__ import annotations

import asyncio
import logging
from datetime import UTC, date, datetime
from typing import Any

from rigger.core.ids import stable_id
from rigger.core.models import Bar, Event, EventKind, Fundamental, Instrument, NewsItem
from rigger.plugins.data.yfinance import YFinanceSymbols

log = logging.getLogger(__name__)

# (event kind, yfinance calendar key, summary label)
CALENDAR_KINDS: tuple[tuple[EventKind, str, str], ...] = (
    ("earnings", "Earnings Date", "Earnings expected"),
    ("dividend", "Ex-Dividend Date", "Ex-dividend"),
)


def calendar_event_id(instrument_id: str, kind: str, day: date) -> str:
    return stable_id(instrument_id, kind, day.isoformat())


def _to_date(value: Any) -> date | None:
    """Normalise yfinance's date-like values (date, datetime, pandas Timestamp)."""
    if value is None:
        return None
    if isinstance(value, datetime):
        return value.date()
    if isinstance(value, date):
        return value
    to_py = getattr(value, "to_pydatetime", None)
    if callable(to_py):
        return _to_date(to_py())
    return None


def _first_date(value: Any) -> date | None:
    """yfinance gives a list of candidate dates for earnings; take the earliest."""
    if isinstance(value, (list, tuple)):
        dates = [d for d in (_to_date(v) for v in value) if d is not None]
        return min(dates) if dates else None
    return _to_date(value)


class YFinanceCalendar(YFinanceSymbols):
    name = "yfinance_calendar"

    @staticmethod
    def _read_calendar(symbol: str) -> dict[str, Any]:
        import yfinance as yf

        cal = yf.Ticker(symbol).calendar
        if cal is None:
            return {}
        if isinstance(cal, dict):
            return cal
        to_dict = getattr(cal, "to_dict", None)  # older yfinance returned a DataFrame
        if callable(to_dict):
            return {str(k): v for k, v in to_dict().items()}
        return {}

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental | Event]:
        today = datetime.now(UTC).date()
        # Independent blocking yfinance reads: run them in threads concurrently.
        calendars = await asyncio.gather(
            *(asyncio.to_thread(self._read_calendar, self.yf_symbol(inst)) for inst in instruments),
            return_exceptions=True,
        )
        events: list[Bar | NewsItem | Fundamental | Event] = []
        for inst, cal in zip(instruments, calendars, strict=True):
            if isinstance(cal, BaseException):
                log.error("calendar fetch failed for %s: %s", inst.id, cal)
                continue
            if not cal:
                continue
            for kind, key, label in CALENDAR_KINDS:
                day = _first_date(cal.get(key))
                if day is None or day < today:
                    continue
                events.append(
                    Event(
                        id=calendar_event_id(inst.id, kind, day),
                        instrument_id=inst.id,
                        ts=datetime(day.year, day.month, day.day, tzinfo=UTC),
                        kind=kind,
                        summary=f"{label} {day.isoformat()}",
                        sentiment=0.0,
                        evidence_ids=[],
                        extracted_by="yfinance",
                        prompt_version="n/a",
                    )
                )
        return events
