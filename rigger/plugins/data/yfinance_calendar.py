"""yfinance calendar data plugin: upcoming earnings and ex-dividend dates as Event rows.

The brief's calendar section reads ``EventTable`` where ``ts > as_of``, so
these rows surface as "Upcoming events" without any further wiring.
"""

from __future__ import annotations

import asyncio
import hashlib
import logging
from datetime import UTC, date, datetime
from typing import Any

from rigger.core.models import Event, Instrument
from rigger.core.plugin import DataPlugin, Plugin

log = logging.getLogger(__name__)

EARNINGS_KEY = "Earnings Date"
EX_DIVIDEND_KEY = "Ex-Dividend Date"


def calendar_event_id(instrument_id: str, kind: str, day: date) -> str:
    return hashlib.sha256(f"{instrument_id}{kind}{day.isoformat()}".encode()).hexdigest()


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


class YFinanceCalendar(DataPlugin):
    name = "yfinance_calendar"
    market = None  # any market; the market plugin supplies the yfinance ticker

    def __init__(self) -> None:
        # Market plugins by name, used for `yf_symbol(instrument)` when present.
        # Populated lazily from entry points; tests set it directly.
        self.markets: dict[str, Plugin] | None = None

    def _market_plugins(self) -> dict[str, Plugin]:
        if self.markets is None:
            try:
                from rigger.core.plugin import MarketPlugin, discover_plugins

                self.markets = {
                    n: p for n, p in discover_plugins().items() if isinstance(p, MarketPlugin)
                }
            except Exception:  # pragma: no cover - discovery is best effort
                log.exception("plugin discovery failed; using raw symbols")
                self.markets = {}
        return self.markets

    def yf_symbol(self, inst: Instrument) -> str:
        market = self._market_plugins().get(inst.market)
        mapper = getattr(market, "yf_symbol", None)
        if callable(mapper):
            return str(mapper(inst))
        return inst.symbol

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

    async def fetch(  # type: ignore[override]
        self, instruments: list[Instrument], since: datetime
    ) -> list[Event]:
        today = datetime.now(UTC).date()
        events: list[Event] = []
        for inst in instruments:
            symbol = self.yf_symbol(inst)
            try:
                cal = await asyncio.to_thread(self._read_calendar, symbol)
            except Exception:
                log.exception("calendar fetch failed for %s (%s)", inst.id, symbol)
                continue
            if not cal:
                continue
            for kind, key, label in (
                ("earnings", EARNINGS_KEY, "Earnings expected"),
                ("dividend", EX_DIVIDEND_KEY, "Ex-dividend"),
            ):
                day = _first_date(cal.get(key))
                if day is None or day < today:
                    continue
                events.append(
                    Event(
                        id=calendar_event_id(inst.id, kind, day),
                        instrument_id=inst.id,
                        ts=datetime(day.year, day.month, day.day, tzinfo=UTC),
                        kind=kind,  # type: ignore[arg-type]
                        summary=f"{label} {day.isoformat()}",
                        sentiment=0.0,
                        evidence_ids=[],
                        extracted_by="yfinance",
                        prompt_version="n/a",
                    )
                )
        return events
