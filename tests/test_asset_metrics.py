from __future__ import annotations

from datetime import UTC, datetime, timedelta

import pandas as pd
from sqlmodel import Session, SQLModel, create_engine

from rigger.asset_metrics import _values, chart_window, fetch_asset_metrics, profile_for
from rigger.core.db import BarTable
from rigger.core.models import Instrument


def test_each_asset_class_has_its_own_profile():
    for asset in ("equity", "etf", "commodity", "bond", "fx", "crypto", "cash", "other"):
        instrument = Instrument(
            id=f"X:{asset}", market="us", symbol=asset, currency="USD", asset_class=asset
        )
        assert profile_for(instrument) == asset


def test_equity_values_and_missing_fields_are_omitted():
    values = _values("equity", {"trailingPE": 16.8, "operatingMargins": 0.214})
    assert values == {"Operating margin": "21.4%", "P/E": "16.8x"}


def test_non_equity_profiles_do_not_show_company_ratios():
    info = {"trailingPE": 16.8, "volume": 100, "couponRate": 0.04}
    assert "P/E" not in _values("commodity", info)
    assert "P/E" not in _values("bond", info)
    assert _values("bond", info)["Coupon"] == "4.0%"


def test_chart_window_keeps_last_thirty_values():
    assert chart_window(list(range(50))) == list(range(20, 50))


def _provider_history(monkeypatch):
    import yfinance

    index = pd.date_range("2020-01-01", periods=6, freq="7D", tz="UTC")
    history = pd.DataFrame({"Close": [10.0, 11.0, 12.0, 12.5, 13.0, 14.0]}, index=index)

    class Ticker:
        def __init__(self, symbol):
            pass

        info = {}

        def history(self, **kwargs):
            return history

    monkeypatch.setattr(yfinance, "Ticker", Ticker)
    return history


def test_all_range_merges_local_bars_without_duplicating_the_overlap(monkeypatch):
    _provider_history(monkeypatch)
    engine = create_engine("sqlite://")
    SQLModel.metadata.create_all(engine)
    # Local dailies inside the provider's weekly window, one colliding with
    # a weekly timestamp exactly.
    first = datetime(2020, 2, 5, tzinfo=UTC)
    with Session(engine) as session:
        for offset, close in enumerate((12.2, 12.3, 12.4)):
            session.add(
                BarTable(
                    instrument_id="X:a",
                    ts=first + timedelta(days=7 * offset),
                    open=close,
                    high=close,
                    low=close,
                    close=close,
                    volume=1,
                    source="yfinance",
                )
            )
        session.commit()

    metrics = fetch_asset_metrics(instrument(), "all", engine=engine)
    # Pre-window weeklies stay, the window's weeklies give way to the local
    # dailies (local wins the exact collision with 14.0), and the result is
    # ascending and unduplicated instead of the overlap appearing twice.
    assert [round(value, 6) for value in metrics.series] == [10, 11, 12, 12.5, 13, 12.2, 12.3, 12.4]
    assert metrics.change_label == "+24.0%"


def test_all_range_falls_back_to_local_bars_when_provider_is_empty(monkeypatch):
    import yfinance

    class EmptyTicker:
        def __init__(self, symbol):
            pass

        info = {}

        def history(self, **kwargs):
            return None

    monkeypatch.setattr(yfinance, "Ticker", EmptyTicker)
    engine = create_engine("sqlite://")
    SQLModel.metadata.create_all(engine)
    with Session(engine) as session:
        session.add(
            BarTable(
                instrument_id="X:a",
                ts=datetime(2020, 2, 5, tzinfo=UTC),
                open=12.2,
                high=12.2,
                low=12.2,
                close=12.2,
                volume=1,
                source="yfinance",
            )
        )
        session.commit()

    metrics = fetch_asset_metrics(instrument(), "all", engine=engine)
    assert metrics.series == [12.2]
    assert metrics.error is None


def instrument():
    return Instrument(id="X:a", market="us", symbol="A", currency="USD", asset_class="equity")
