"""Pure helpers for chart axes: Y tick placement and X/Y label formatting.

No Textual or network imports: chart widgets call these on the UI thread and
the tests run offline.
"""

from __future__ import annotations

import math
from datetime import datetime, timedelta

# Heckbert's nice step multipliers; 2.5 keeps steps like 0.25 and 2,500 available.
_NICE_STEPS = (1.0, 2.0, 2.5, 5.0, 10.0)

# Label format switches: past 36h a clock time is noise, past ~18 months the
# day number is too small to read on a chart that wide.
_HOURS_36 = timedelta(hours=36)
_MONTHS_18 = timedelta(days=548)


def _ceil_to(value: float, step: float) -> float:
    """Smallest multiple of ``step`` >= ``value``; the round() absorbs FP dust
    so a mathematically exact grid point does not jump a whole step."""
    return math.ceil(round(value / step, 9)) * step


def nice_ticks(low: float, high: float, count: int = 3) -> list[float]:
    """Tick values for a Y axis: 1/2/2.5/5 x 10^n steps covering [low, high].

    Returns exactly ``count`` ticks, first <= low, last >= high, ascending.
    Ticks sit on one step grid so labels read as round numbers; the grid is
    anchored at the top (last tick >= high), so the step must leave enough
    slack that rounding the top up does not push the first tick past ``low``.
    A flat series has no span to step over, so every tick is the flat value.
    """
    if low == high:
        return [low] * count
    if low > high:  # defensive; callers pass min()/max() of a series
        low, high = high, low
    raw_step = (high - low) / (count - 1)
    magnitude = 10.0 ** math.floor(math.log10(raw_step))
    for base in (magnitude, magnitude * 10.0):
        for multiplier in _NICE_STEPS:
            step = multiplier * base
            if step < raw_step:
                continue
            first = _ceil_to(high, step) - (count - 1) * step
            if first <= low:
                return [round(first + index * step, 10) for index in range(count)]
    # No grid point lands both ends inside the range: split linearly so the
    # coverage contract still holds.
    step = (high - low) / (count - 1)
    return [round(low + index * step, 10) for index in range(count)]


def format_price(value: float, *, kind: str = "price", currency: str = "") -> str:
    """Format one axis label.

    ``kind="price"`` gets thousands separators (1,842.50); a yield never does,
    because a separator reads like a thousands-scaled rate. Currency rides
    after a space only when given, so the default label stays column-compact.
    """
    body = f"{value:,.2f}" if kind == "price" else f"{value:.2f}"
    return f"{body} {currency}" if currency else body


def x_ticks(times: list[str], columns: int) -> list[tuple[int, str]]:
    """At most three X axis ticks for a chart ``columns`` braille cells wide.

    ``times`` are ISO timestamp strings; unparseable entries are skipped
    rather than aborting the chart. Ends anchor at columns 0 and columns-1 so
    the labels line up with the chart edges; a middle tick is added only when
    the span is wide enough that its label cannot collide with the ends
    (label width estimated as ``len(label) + 2`` cells).
    """
    if not times or len(times) < 2 or columns < 8:
        return []
    parsed: list[datetime] = []
    for entry in times:
        try:
            parsed.append(datetime.fromisoformat(entry))
        except (TypeError, ValueError):
            continue
    if len(parsed) < 2:
        return []
    try:
        span = parsed[-1] - parsed[0]
    except TypeError:  # mixed naive/aware stamps cannot be subtracted
        return []
    if span < _HOURS_36:
        fmt = "%H:%M"
    elif span < _MONTHS_18:
        fmt = "%d %b"
    else:
        fmt = "%b %y"
    labels = [point.strftime(fmt) for point in parsed]
    last_col = columns - 1
    ticks = [(0, labels[0]), (last_col, labels[-1])]
    middle = min(range(len(parsed)), key=lambda i: abs(parsed[i] - (parsed[0] + span / 2)))
    mid_col = round(middle * last_col / (len(parsed) - 1))
    if 0 < mid_col < last_col:
        # A middle label needs clearance from both anchored end labels.
        width = max(len(labels[0]), len(labels[middle]), len(labels[-1])) + 2
        if mid_col >= width and last_col - mid_col >= width:
            ticks.insert(1, (mid_col, labels[middle]))
    return ticks
