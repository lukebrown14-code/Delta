from __future__ import annotations

from rigger.asset_metrics import _values, chart_window, profile_for
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
