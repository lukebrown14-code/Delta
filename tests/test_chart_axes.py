"""Offline tests for the pure chart-axis helpers in delta.tui.axes."""

from __future__ import annotations

import re
from datetime import datetime, timedelta

from delta.tui.axes import format_price, nice_ticks, plot_scale, x_ticks


def _times(start: str, count: int, step: timedelta) -> list[str]:
    first = datetime.fromisoformat(start)
    return [(first + index * step).isoformat(sep=" ") for index in range(count)]


def _assert_no_overlap(ticks: list[tuple[int, str]]) -> None:
    """Each label needs its own width plus a two-cell gutter of clearance."""
    for (col_a, label_a), (col_b, label_b) in zip(ticks, ticks[1:], strict=False):
        assert col_b - col_a >= max(len(label_a), len(label_b)) + 2


def test_nice_ticks_use_clean_steps():
    ticks = nice_ticks(76.2, 78.2)
    assert ticks == [76.0, 78.0, 80.0]
    assert ticks[0] <= 76.2 and ticks[-1] >= 78.2


def test_flat_series_repeats_the_value():
    assert nice_ticks(42.0, 42.0) == [42.0, 42.0, 42.0]
    assert nice_ticks(42.0, 42.0, count=4) == [42.0] * 4


def test_wide_range_steps_in_thousands():
    assert nice_ticks(1200.0, 8400.0) == [0.0, 5000.0, 10000.0]


def test_round_bounds_stay_round():
    assert nice_ticks(100.0, 200.0) == [100.0, 150.0, 200.0]


def test_count_is_respected_and_ends_cover_the_range():
    for count in (2, 4, 5):
        ticks = nice_ticks(10.0, 20.0, count=count)
        assert len(ticks) == count
        assert ticks == sorted(ticks)
        assert ticks[0] <= 10.0 and ticks[-1] >= 20.0


def test_plot_scale_bounds_to_outer_ticks():
    """K2: the plot bounds are the first/last nice tick, never the raw extremes."""
    low, high = plot_scale(309.90, 338.98, 3)
    assert low == 300.0 and high == 340.0
    assert low <= 309.90 and high >= 338.98


def test_plot_scale_spans_every_tick():
    """A series scaled to its bounds places first/last data on first/last tick."""
    low, high = plot_scale(0.0, 99.0, 5)
    assert (low, high) == (0.0, 100.0)
    ticks = nice_ticks(low, high, 5)
    assert ticks[0] == low and ticks[-1] == high


def test_price_format_adds_thousands_separators():
    assert format_price(78.2) == "78.20"
    assert format_price(1842.5) == "1,842.50"


def test_yield_format_skips_separators():
    assert format_price(4.2, kind="yield") == "4.20"
    assert format_price(1234.5, kind="yield") == "1234.50"


def test_currency_is_appended_only_when_present():
    assert format_price(78.2, currency="USD") == "78.20 USD"
    assert format_price(4.2, kind="yield", currency="GBP") == "4.20 GBP"
    assert format_price(78.2) == "78.20"


def test_x_ticks_needs_two_points_and_room():
    stamps = ["2026-08-12 09:00:00+00:00", "2026-08-12 15:00:00+00:00"]
    assert x_ticks([], 40) == []
    assert x_ticks(stamps[:1], 40) == []
    assert x_ticks(stamps, 7) == []


def test_unparseable_entries_are_skipped():
    stamps = ["nonsense", "2026-08-12 09:00:00+00:00", "", "2026-08-12 15:00:00+00:00"]
    ticks = x_ticks(stamps, 40)
    assert [column for column, _ in ticks] == [0, 39]
    assert ticks[0][1] == "09:00"


def test_intraday_span_uses_clock_labels():
    stamps = _times("2026-08-12 09:00:00+00:00", 7, timedelta(hours=1))
    ticks = x_ticks(stamps, 40)
    assert len(ticks) == 3
    assert ticks[0] == (0, "09:00")
    assert ticks[-1] == (39, "15:00")
    assert all(re.fullmatch(r"\d{2}:\d{2}", label) for _, label in ticks)
    _assert_no_overlap(ticks)


def test_month_span_uses_day_month_labels():
    stamps = _times("2026-07-12 00:00:00+00:00", 30, timedelta(days=1))
    ticks = x_ticks(stamps, 60)
    assert len(ticks) == 3
    assert ticks[0] == (0, "12 Jul")
    assert ticks[-1] == (59, "10 Aug")
    assert all(re.fullmatch(r"\d{2} [A-Z][a-z]{2}", label) for _, label in ticks)
    _assert_no_overlap(ticks)


def test_multi_year_span_uses_month_year_labels():
    stamps = _times("2022-08-12 00:00:00+00:00", 5, timedelta(days=365))
    ticks = x_ticks(stamps, 60)
    assert len(ticks) == 3
    assert ticks == [(0, "Aug 22"), (30, "Aug 24"), (59, "Aug 26")]
    _assert_no_overlap(ticks)


def test_middle_tick_is_dropped_when_labels_would_collide():
    stamps = _times("2026-08-12 09:00:00+00:00", 7, timedelta(hours=1))
    ticks = x_ticks(stamps, 8)
    assert [column for column, _ in ticks] == [0, 7]
    _assert_no_overlap(ticks)
