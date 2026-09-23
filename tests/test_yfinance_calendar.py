"""Tests for the yfinance calendar data plugin (no network)."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

import yfinance

from delta.core.models import Instrument
from delta.core.plugin import apply_config
from delta.plugins.data.yfinance import YFinanceData
from delta.plugins.data.yfinance_calendar import YFinanceCalendar, calendar_event_id

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")
BHP = Instrument(id="ASX:BHP", market="asx", symbol="BHP", currency="AUD")
SINCE = datetime(2026, 1, 1, tzinfo=UTC)

TODAY = datetime.now(UTC).date()
EARNINGS = TODAY + timedelta(days=30)
EX_DIV = TODAY + timedelta(days=10)


class _FakeTicker:
    calendars: dict[str, dict] = {}
    requested: list[str] = []

    def __init__(self, symbol: str) -> None:
        self.symbol = symbol
        _FakeTicker.requested.append(symbol)

    @property
    def calendar(self) -> dict:
        return _FakeTicker.calendars.get(self.symbol, {})


def _plugin(suffixes: dict[str, str] | None = None) -> YFinanceCalendar:
    plugin = YFinanceCalendar()
    plugin.configure({"suffixes": suffixes or {}})
    return plugin


def _patch(monkeypatch, calendars: dict[str, dict]) -> None:
    _FakeTicker.calendars = calendars
    _FakeTicker.requested = []
    monkeypatch.setattr(yfinance, "Ticker", _FakeTicker)


def test_emits_earnings_and_dividend_events(monkeypatch):
    _patch(
        monkeypatch,
        {
            "AAPL": {
                "Earnings Date": [EARNINGS, EARNINGS + timedelta(days=4)],
                "Ex-Dividend Date": EX_DIV,
                "Dividend Date": EX_DIV + timedelta(days=14),
                "Earnings Average": 1.5,
            }
        },
    )

    events = asyncio.run(_plugin().fetch([AAPL], SINCE))

    assert [e.kind for e in events] == ["earnings", "dividend"]
    now = datetime.now(UTC)
    for e in events:
        assert e.instrument_id == AAPL.id
        assert e.ts > now
        assert e.ts.tzinfo is UTC and (e.ts.hour, e.ts.minute) == (0, 0)
        assert e.sentiment == 0.0
        assert e.evidence_ids == []
        assert e.extracted_by == "yfinance"
        assert e.prompt_version == "n/a"

    earnings, dividend = events
    assert earnings.ts.date() == EARNINGS  # earliest of the candidate dates
    assert earnings.summary == f"Earnings expected {EARNINGS.isoformat()}"
    assert earnings.id == calendar_event_id(AAPL.id, "earnings", EARNINGS)
    assert dividend.ts.date() == EX_DIV
    assert dividend.summary == f"Ex-dividend {EX_DIV.isoformat()}"
    assert dividend.id == calendar_event_id(AAPL.id, "dividend", EX_DIV)


def test_no_calendar_yields_nothing(monkeypatch):
    _patch(monkeypatch, {"AAPL": {}})

    assert asyncio.run(_plugin().fetch([AAPL], SINCE)) == []


def test_past_dates_are_dropped(monkeypatch):
    _patch(monkeypatch, {"AAPL": {"Ex-Dividend Date": TODAY - timedelta(days=5)}})

    assert asyncio.run(_plugin().fetch([AAPL], SINCE)) == []


def test_market_suffix_maps_asx_tickers(monkeypatch):
    _patch(monkeypatch, {"BHP.AX": {"Earnings Date": [EARNINGS]}, "AAPL": {}})

    events = asyncio.run(_plugin().fetch([BHP, AAPL], SINCE))

    assert sorted(_FakeTicker.requested) == ["AAPL", "BHP.AX"]
    assert [e.instrument_id for e in events] == [BHP.id]


def test_ticker_errors_do_not_abort_the_batch(monkeypatch):
    class _Boom(_FakeTicker):
        @property
        def calendar(self) -> dict:
            if self.symbol == "AAPL":
                raise RuntimeError("yahoo down")
            return super().calendar

    _FakeTicker.calendars = {"BHP.AX": {"Ex-Dividend Date": EX_DIV}}
    _FakeTicker.requested = []
    monkeypatch.setattr(yfinance, "Ticker", _Boom)

    events = asyncio.run(_plugin().fetch([AAPL, BHP], SINCE))

    assert [e.instrument_id for e in events] == [BHP.id]


def test_suffixes_are_shared_with_the_bars_plugin(monkeypatch) -> None:
    """A market added to [plugins.yfinance].suffixes must reach the calendar plugin too."""
    bars, cal = YFinanceData(), YFinanceCalendar()
    apply_config(
        {bars.name: bars, cal.name: cal},
        {"yfinance": {"suffixes": {"lse": ".L"}}, "yfinance_calendar": {"enabled": True}},
    )
    shel = Instrument(id="LSE:SHEL", market="lse", symbol="SHEL", currency="GBP")
    assert bars.yf_symbol(shel) == "SHEL.L"
    assert cal.yf_symbol(shel) == "SHEL.L"
