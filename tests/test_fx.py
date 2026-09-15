"""FX instruments and rate lookup."""

from __future__ import annotations

from datetime import UTC, datetime

from rigger.core.models import Instrument
from rigger.paper.fx import fx_instruments, latest_fx_rate, market_of
from tests.conftest import seed_bars

US = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")
US2 = Instrument(id="US:MSFT", market="us", symbol="MSFT", currency="USD")
ASX = Instrument(id="ASX:BHP", market="asx", symbol="BHP", currency="AUD")


def test_fx_instruments_one_per_foreign_currency():
    fx = fx_instruments([US, US2, ASX], "AUD")
    assert [i.id for i in fx] == ["FX:USDAUD"]
    assert fx[0].symbol == "USDAUD=X"
    assert fx[0].market == "fx"
    assert fx_instruments([ASX], "AUD") == []


def test_market_of():
    assert market_of("US:AAPL") == "us"
    assert market_of("ASX:BHP") == "asx"
    assert market_of("AAPL") == "us"


def test_latest_fx_rate(tmp_engine):
    assert latest_fx_rate(tmp_engine, "AUD", "AUD") == 1.0
    assert latest_fx_rate(tmp_engine, "USD", "AUD") is None
    seed_bars(
        tmp_engine,
        "FX:USDAUD",
        n=3,
        start=datetime(2026, 9, 1, tzinfo=UTC),
        price_fn=lambda i: [1.4, 1.45, 1.5][i],
    )
    assert latest_fx_rate(tmp_engine, "USD", "AUD") == 1.5
