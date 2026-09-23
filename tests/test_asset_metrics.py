from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

import pandas as pd
import yfinance

from delta.asset_metrics import (
    _TABLES,
    METRIC_HELP,
    _group_values,
    _values,
    chart_window,
    fetch_asset_metrics,
    profile_for,
)
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


def test_dividend_yield_is_already_a_percent_not_a_ratio():
    """Yahoo ships ``dividendYield`` pre-scaled: 0.32 means 0.32%, not 32%."""
    assert _values("equity", {"dividendYield": 0.32})["Dividend yield"] == "0.3%"


def test_margin_ratios_multiply_by_one_hundred():
    assert _values("equity", {"grossMargins": 0.48653})["Gross margin"] == "48.7%"


def test_cash_yield_is_a_ratio():
    assert _values("cash", {"yield": 0.0376})["Yield"] == "3.8%"


def test_money_count_date_and_fx_formats():
    values = _values(
        "equity",
        {
            "marketCap": 4958372495360,
            "freeCashflow": 108810000000,
            "sharesOutstanding": 14594180000,
            "exDividendDate": 1786320000,
        },
    )
    assert values["Market cap"] == "$4.96T"
    assert values["Free cash flow"] == "$108.81B"
    assert values["Shares out"] == "14.59B"
    stamp = datetime.fromtimestamp(1786320000, tz=UTC)
    assert values["Ex-dividend"] == f"{stamp:%-d %b %Y}"

    assert _values("fx", {"bid": 0.71214926})["Bid"] == "0.7121"


def test_equity_groups_fit_the_eight_card_pool():
    grouped = _group_values(
        "equity",
        {
            "trailingPE": 39.0,
            "grossMargins": 0.48,
            "marketCap": 4958372495360,
            "recommendationKey": "buy",
            "targetMeanPrice": 328.22,
            "fiftyTwoWeekHigh": 345.34,
            "earningsTimestamp": 1785441600,
            "dividendYield": 0.32,
            "freeCashflow": 108810000000,
            "beta": 1.09,
        },
    )
    assert len(grouped) <= 8
    assert set(grouped) <= {
        "Profitability",
        "Valuation",
        "Balance Sheet",
        "Shareholder Returns",
        "Size",
        "Trading & Ownership",
        "Analyst View",
        "Price Context",
    }
    assert {"Valuation", "Analyst View", "Price Context", "Size", "Trading & Ownership"} <= set(grouped)


def test_bond_fund_style_keys_fire_for_etf_bonds():
    """ETF-traded bonds (TLT) have no coupon/maturity on Yahoo; fund keys do."""
    grouped = _group_values(
        "bond",
        {
            "yield": 0.0473,
            "navPrice": 81.77,
            "totalAssets": 47046328320,
            "fiftyTwoWeekHigh": 92.19,
            "fiftyTwoWeekLow": 80.46,
        },
    )
    assert grouped["Income"] == {"Distribution yield": "4.7%"}
    assert grouped["Fund Scale"]["NAV"] == "81.77"
    assert grouped["Price Context"]["52w high"] == "92.19"


def test_analyst_estimates_merge_into_the_analyst_view(monkeypatch):
    est = pd.DataFrame(
        {"avg": [1.98, 8.82], "growth": [0.0689, 0.1822]},
        index=["+1q", "0y"],
    )
    rev = pd.DataFrame({"avg": [113624521680, 477832817030]}, index=["+1q", "0y"])
    base = _fake_ticker({("1mo", "1d"): _frame([100.0, 110.0])})

    class TickerWithEstimates(base):
        earnings_estimate = est
        revenue_estimate = rev

    monkeypatch.setattr(yfinance, "Ticker", TickerWithEstimates)
    metric = fetch_asset_metrics(AAPL, "month")
    view = metric.groups["Analyst View"]
    assert view["EPS est (next q)"] == "1.98"
    assert view["EPS growth est"] == "6.9%"
    assert view["Revenue est (fy)"] == "$477.83B"


def test_missing_estimate_endpoints_do_not_fail_the_fetch(monkeypatch):
    monkeypatch.setattr(
        yfinance, "Ticker", _fake_ticker({("1mo", "1d"): _frame([100.0, 110.0])})
    )
    metric = fetch_asset_metrics(AAPL, "month")
    assert metric.error is None
    assert "EPS est (next q)" not in metric.groups.get("Analyst View", {})


def test_distance_from_52w_high_is_computed(monkeypatch):
    class TickerWithHigh(_fake_ticker({("1mo", "1d"): _frame([80.0, 90.0])})):
        info = {"fiftyTwoWeekHigh": 100.0}

    monkeypatch.setattr(yfinance, "Ticker", TickerWithHigh)
    metric = fetch_asset_metrics(AAPL, "month")
    assert metric.groups["Price Context"]["From 52w high"] == "-10.0%"


def test_every_metric_label_has_help_text():
    for _profile, (keys, _groups) in _TABLES.items():
        for label in keys:
            assert label in METRIC_HELP, label
    for injected in (
        "Current price",
        "Current yield",
        "From 52w high",
        "EPS est (next q)",
        "EPS growth est",
        "Revenue est (fy)",
    ):
        assert injected in METRIC_HELP, injected


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
