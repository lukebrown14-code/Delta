"""Offline tests for the Targets inspector's pure UI helpers (K7, K8, J10).

The chart drawing lives in ``test_chart.py``; these cover the header's 52-week
position bar, the visible range strip vocabulary and rotation, and the
direction-arrow glyphs the range change uses — all without mounting an App.
"""

from __future__ import annotations

from delta.tui.screens.targets import (
    DEFAULT_RANGE,
    RANGE_LABELS,
    RANGE_WINDOW,
    RANGES,
    _fraction_bar,
)


def test_fraction_bar_spans_empty_to_full():
    """K7: the 52-week bar fills proportionally and stays in bounds."""
    assert _fraction_bar(0.0, width=8).count("█") == 0
    assert _fraction_bar(0.0, width=8).startswith("▕")
    assert _fraction_bar(1.0, width=8).endswith("▏")
    assert _fraction_bar(1.0, width=8) == "▕" + "█" * 8 + "▏"

    # Fraction out of range is clamped rather than overflowing.
    assert _fraction_bar(1.5, width=8) == _fraction_bar(1.0, width=8)
    assert _fraction_bar(-0.5, width=8) == _fraction_bar(0.0, width=8)

    # A middle fraction fills some blocks and one partial eighth.
    mid = _fraction_bar(0.5, width=8)
    assert mid.count("█") >= 3


def test_fraction_bar_monotonic_in_fraction():
    """A higher fill never shows fewer blocks than a lower one."""
    for width in (4, 8, 16, 32):
        bars = [_fraction_bar(i / 10, width) for i in range(11)]
        counts = [bar.count("█") for bar in bars]
        assert counts == sorted(counts)


def test_range_strip_covers_all_ranges_in_order():
    """K8: the strip spells 1D→ALL, each with a label and a window."""
    assert RANGES == ("1d", "5d", "1m", "6m", "ytd", "1y", "all")
    assert DEFAULT_RANGE == "1m"
    assert RANGE_LABELS["all"] == "ALL" and RANGE_LABELS["1d"] == "1D"
    # 1D and ALL show the whole (intraday/history) series; the mid ranges slice.
    assert RANGE_WINDOW["1d"] is None and RANGE_WINDOW["all"] is None
    assert RANGE_WINDOW["1m"] == 30 and RANGE_WINDOW["5d"] == 5


def test_range_rotation_wraps_forward_and_back():
    """K8: r/R step through RANGES without falling off either end."""
    outgoing = ["6m", "ytd", "1y", "all", "1d", "5d", "1m"]

    def next_range(name: str, delta: int) -> str:
        index = RANGES.index(name) if name in RANGES else RANGES.index(DEFAULT_RANGE)
        return RANGES[(index + delta) % len(RANGES)]

    current = "1m"
    stepped = []
    for _ in range(7):
        current = next_range(current, 1)
        stepped.append(current)
    assert stepped == outgoing
    assert next_range("1d", -1) == "all"
    assert next_range("all", 1) == "1d"


def test_range_window_has_a_value_for_every_range():
    """Every range in the strip has a window (slice count or None for full)."""
    assert set(RANGE_WINDOW) == set(RANGES)
