"""Paper portfolio arithmetic across currencies."""

from __future__ import annotations

from datetime import UTC, datetime

import pytest

from rigger.core.models import Order
from rigger.paper.portfolio import PaperPortfolio
from tests.conftest import seed_bars


def _buy(qty: float, inst: str = "US:AAPL") -> Order:
    return Order(
        id=f"o-{inst}-{qty}",
        signal_id="s1",
        instrument_id=inst,
        side="buy",
        qty=qty,
        type="market",
        submitted_ts=datetime.now(UTC),
        broker="paper",
    )


def test_fill_converts_to_base_currency(tmp_engine):
    seed_bars(tmp_engine, "FX:USDAUD", n=1, price_fn=lambda i: 1.5)
    pf = PaperPortfolio(tmp_engine, "AUD", slippage_bps=0.0, currencies={"us": "USD"})
    fill = pf.fill(_buy(10.0), market="us", fill_price=100.0)

    assert fill.price == 100.0  # fill stays in USD
    assert pf.cash() == pytest.approx(100_000.0 - 1_500.0)  # 1,000 USD x 1.5
    assert pf.equity() == pytest.approx(100_000.0)  # buying does not change equity
    assert pf.positions()[0].avg_price == 100.0
    assert pf.latest_price_base("FX:USDAUD") is not None


def test_latest_price_base(tmp_engine):
    seed_bars(tmp_engine, "FX:USDAUD", n=1, price_fn=lambda i: 1.5)
    seed_bars(tmp_engine, "US:AAPL", n=1, price_fn=lambda i: 200.0)
    pf = PaperPortfolio(tmp_engine, "AUD", currencies={"us": "USD"})
    assert pf.latest_price("US:AAPL") == 200.0
    assert pf.latest_price_base("US:AAPL") == pytest.approx(300.0)


def test_missing_fx_rate_raises(tmp_engine):
    pf = PaperPortfolio(tmp_engine, "AUD", currencies={"us": "USD"})
    with pytest.raises(ValueError, match="no FX rate for USD/AUD"):
        pf.fill(_buy(1.0), market="us", fill_price=100.0)
    assert pf.cash() == 100_000.0  # nothing was debited


def test_base_currency_instrument_needs_no_rate(tmp_engine):
    pf = PaperPortfolio(tmp_engine, "AUD", slippage_bps=0.0, currencies={"asx": "AUD"})
    pf.fill(_buy(10.0, "ASX:BHP"), market="asx", fill_price=40.0)
    # 400 AUD notional + ASX fee min $10
    assert pf.cash() == pytest.approx(100_000.0 - 400.0 - 10.0)


def test_no_currency_map_keeps_legacy_arithmetic(tmp_engine):
    pf = PaperPortfolio(tmp_engine, "AUD", slippage_bps=0.0)
    pf.fill(_buy(10.0), market="us", fill_price=100.0)
    assert pf.cash() == pytest.approx(99_000.0)
    assert pf.equity() == pytest.approx(100_000.0)
