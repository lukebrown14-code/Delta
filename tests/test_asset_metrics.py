from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

import pandas as pd
import yfinance

from delta.asset_metrics import _values, chart_window, fetch_asset_metrics, profile_for
from delta.core.models import Instrument
from tests.conftest import seed_bars

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")


def _frame(closes: list[float]) -> pd.DataFrame:
    index = pd.date_range("2000-01-03", periods=len(closes), freq="7D")
    return pd.DataFrame({"Close": closes}, index=index)


def _fake_ticker(frames: dict[tuple[str, str], pd.DataFrame]) -> type:
    """A ``yfinance.Ticker`` stand-in that replays canned history frames."""

    class FakeTicker:
        info: dict[str, Any] = {}

        def __init__(self, symbol: str) -> None:
            self.symbol = symbol

        def history(self, period: str, interval: str, auto_adjust: bool) -> pd.DataFrame:
            return frames.get((period, interval), pd.DataFrame({"Close": []}))

    return FakeTicker


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
    assert chart_window(list(range(50))) == (list(range(20, 50)), [])


def test_chart_window_slices_times_with_the_same_offset():
    values = [10.0, 20.0, 30.0, 40.0]
    times = ["2026-08-01", "2026-08-02", "2026-08-03", "2026-08-04"]
    assert chart_window(values, days=2, times=times) == (
        [30.0, 40.0],
        ["2026-08-03", "2026-08-04"],
    )


def test_chart_window_with_days_none_returns_everything():
    values = [1.0, 2.0, 3.0]
    times = ["2026-08-01", "2026-08-02", "2026-08-03"]
    assert chart_window(values, days=None, times=times) == (values, times)


def test_chart_window_drops_mismatched_or_missing_times():
    values = [1.0, 2.0, 3.0]
    assert chart_window(values, days=2) == ([2.0, 3.0], [])
    assert chart_window(values, days=2, times=["2026-08-01", "2026-08-02"]) == ([2.0, 3.0], [])


def test_provider_history_populates_series_times_parallel_to_series(monkeypatch):
    monkeypatch.setattr(
        yfinance, "Ticker", _fake_ticker({("1mo", "1d"): _frame([10.0, 12.0, 11.0])})
    )
    metric = fetch_asset_metrics(AAPL, "month")
    assert len(metric.series_times) == len(metric.series) == 3
    for raw in metric.series_times:
        datetime.fromisoformat(raw)  # chart X labels need parseable ISO strings
    assert metric.series_times[0].startswith("2000-01-03")


def test_local_bar_fallback_populates_series_times(tmp_engine, monkeypatch):
    seed_bars(tmp_engine, AAPL.id, n=5)
    monkeypatch.setattr(yfinance, "Ticker", _fake_ticker({}))
    metric = fetch_asset_metrics(AAPL, "all", tmp_engine)
    assert len(metric.series_times) == len(metric.series) == 5
    for raw in metric.series_times:
        datetime.fromisoformat(raw)


def test_all_time_keeps_provider_order_not_local_first(tmp_engine, monkeypatch):
    """Local bars are recent: prepending them to the provider's max history
    made the all-time change read as the recent-window change, so switching
    the range never moved the number."""
    seed_bars(tmp_engine, AAPL.id, n=5, price_fn=lambda _i: 100.0)
    monkeypatch.setattr(
        yfinance, "Ticker", _fake_ticker({("max", "1wk"): _frame([10.0, 20.0, 40.0])})
    )
    metric = fetch_asset_metrics(AAPL, "all", tmp_engine)
    assert metric.series == [10.0, 20.0, 40.0]
    assert metric.change_label == "+300.0%"
    assert metric.period_high == 40.0
    assert metric.period_low == 10.0
    assert len(metric.series_times) == 3


def test_all_time_falls_back_to_local_bars_when_provider_is_empty(tmp_engine, monkeypatch):
    seed_bars(tmp_engine, AAPL.id, n=80)  # 100.0 rising 0.5/day → last close 139.50
    monkeypatch.setattr(yfinance, "Ticker", _fake_ticker({}))
    metric = fetch_asset_metrics(AAPL, "all", tmp_engine)
    assert metric.series[-1] == 139.5
    assert metric.series_times[-1].startswith(str(datetime.now(UTC).year))
    assert metric.values["Current price"] == "139.50"
    assert metric.change_label == "+39.5%"
