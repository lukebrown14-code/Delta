"""PriceChart tests: layout runs, dot grid, gutters and axis rows.

Fully offline except the one painting test, which follows the BrailleGraph
pattern in ``test_tui.py``: assert on a real app's exported frame, not just
the renderable, because a green layout has hidden a widget Textual never
painted before.
"""

from __future__ import annotations

import asyncio
import io
from datetime import datetime, timedelta

from rich.color import Color as RichColor
from rich.console import Console

from delta.tui.axes import nice_ticks, x_ticks
from delta.tui.widgets import BrailleGraph, PriceChart, _PriceChartRender

RISING = [float(i) for i in range(100)]
#: One ISO stamp per point of RISING: a ~99 day span, so x_ticks picks the
#: "dd Mon" label format and can afford a middle tick.
TIMES = [(datetime(2026, 8, 8) + timedelta(days=i)).isoformat() for i in range(100)]


def test_row_shape_and_gutter_labels():
    """Eight rows at height 8: six braille rows, rule and label rows, ticks."""
    chart = PriceChart(RISING)
    rows = chart.rows(40, 8)
    assert len(rows) == 8
    assert all(len(row) == 40 for row in rows)
    # ticks(0..99) is [0, 50, 100]; the gutter is "┤ " + the widest label.
    assert [int(t) for t in nice_ticks(0.0, 99.0, 3)] == [0, 50, 100]
    assert rows[0].endswith("┤ 100.00")
    assert rows[2].endswith("┤  50.00")
    assert rows[5].endswith("┤   0.00")
    # Non-tick braille rows carry the faint gutter rule, then padding.
    assert rows[1].endswith("│" + " " * 7)
    # The braille plot is the first 32 columns and holds braille glyphs only.
    for row in rows[:6]:
        assert all(0x2800 <= ord(ch) <= 0x28FF for ch in row[:32])


def test_rising_line_runs_bottom_left_to_top_right():
    """Dot-grid direction, BrailleGraph's assertion adapted to the plot area."""
    rows = PriceChart(RISING).rows(40, 8)
    top, bottom = rows[0][:32], rows[5][:32]
    assert top[0] == PriceChart.EMPTY and top[-1] != PriceChart.EMPTY
    assert bottom[0] != PriceChart.EMPTY and bottom[-1] == PriceChart.EMPTY


def test_degenerate_series_do_not_crash():
    """Flat, single point and empty data: sane output, no divide by zero."""
    rows = PriceChart([5.0] * 50).rows(40, 8)
    assert len(rows) == 8
    # Every tick is the flat value, so one gutter row owns all three labels.
    assert rows[5].endswith("┤ 5.00")
    assert all("┤" not in row for row in rows[:5])

    assert len(PriceChart([7.0]).rows(40, 8)) == 8

    rows = PriceChart([]).rows(40, 8)
    assert len(rows) == 8
    assert set(rows[0]) == {BrailleGraph.EMPTY}
    assert rows[6] == "└" + "─" * 38 + "┘"
    assert not rows[7].strip()

    # Under three rows there are no axes at all, just the bare line.
    bare = PriceChart(RISING).rows(20, 2)
    assert len(bare) == 2 and all(len(row) == 20 for row in bare)
    assert PriceChart([]).rows(20, 2) == [BrailleGraph.EMPTY * 20] * 2
    assert PriceChart(RISING).rows(0, 0) == []


def test_narrow_chart_hides_the_gutter():
    """At width 18 the labels would starve the plot below 12 cells: no gutter."""
    rows = PriceChart(RISING).rows(18, 8)
    assert all(len(row) == 18 for row in rows[:6])
    assert all("┤" not in row and "│" not in row for row in rows)
    assert all("0.00" not in row for row in rows)
    # The rule shrinks to the widget instead of overflowing; the line stays.
    assert rows[6] == "└" + "─" * 16 + "┘"
    assert any(ch != BrailleGraph.EMPTY for row in rows[:6] for ch in row)


def test_y_format_is_honoured():
    """The gutter labels come from y_format, and the gutter resizes to them."""

    def points(value: float) -> str:
        return f"{value:.0f}p"

    chart = PriceChart(RISING)
    chart.y_format = points
    rows = chart.rows(40, 8)
    assert rows[0].endswith("┤ 100p")
    assert rows[2].endswith("┤  50p")
    assert rows[5].endswith("┤   0p")
    assert all(len(row) == 40 for row in rows)


def test_x_axis_rows_with_and_without_times():
    """With times: ┬ rule and centred labels; without: a plain, silent rule."""
    chart = PriceChart(RISING)
    chart.times = TIMES
    rows = chart.rows(40, 8)
    xaxis = x_ticks(TIMES, 32)
    assert len(xaxis) == 3
    rule_row = rows[6].rstrip()
    assert rule_row.count("┬") == 3
    assert rule_row.startswith("└") and rule_row.endswith("┘")
    # The first label anchors at the plot's left edge.
    assert rows[7].startswith(xaxis[0][1])

    plain = PriceChart(RISING).rows(40, 8)
    assert plain[6].rstrip() == "└" + "─" * 32 + "┘"
    assert not plain[7].strip()


def test_mid_gridline_fills_the_gaps_the_line_leaves():
    """The gridline dots the mid tick's dot row, styled apart from the line.

    Asserted on the row runs of ``_PriceChartRender``: a braille row there is
    a list of (text, kind) runs, so "grid-styled cells carry no line dots" is
    checkable bit for bit, which a flat string could not express.
    """
    chart = PriceChart(RISING)
    runs = chart._runs(40, 8)
    low, high = min(RISING), max(RISING)
    span = (high - low) or 1.0
    dot_rows = 6 * 4
    grid_dot_row = (
        dot_rows - 1 - int(round((nice_ticks(low, high, 3)[1] - low) / span * (dot_rows - 1)))
    )
    mask = BrailleGraph.DOTS[0][grid_dot_row % 4] | BrailleGraph.DOTS[1][grid_dot_row % 4]
    row = runs[grid_dot_row // 4]
    grid_cells = [
        text for text, kind in row if kind == "grid" and text and 0x2800 <= ord(text[0]) <= 0x28FF
    ]
    assert grid_cells, "no gridline dots on the mid tick's dot row"
    assert all(ord(ch) - 0x2800 == mask for text in grid_cells for ch in text)
    line_cells = [text for text, kind in row if kind == "line"]
    assert line_cells, "the line should cross the mid tick's row here"
    # Cells that carry line dots are never grid-styled: grid dots only ever
    # land in cells whose line mask is empty.
    assert all(ord(ch) != 0x2800 for text in line_cells for ch in text)


def test_renderable_yields_one_run_per_span_and_a_line_per_row():
    """_PriceChartRender follows the segment-run convention Textual survives.

    Textual paints a widget line by line; only a renderable that yields runs
    per line followed by Segment.line() survives that, so pin the structure.
    """
    chart = PriceChart(RISING)
    renderable = _PriceChartRender(
        chart._runs(40, 8),
        low=RichColor.parse("#5b8def"),
        high=RichColor.parse("#d4d4d4"),
        grid=RichColor.parse("#5c5c5c"),
        axis=RichColor.parse("#8a8a8a"),
        fill=False,
    )
    console = Console(width=40, file=io.StringIO(), legacy_windows=False)
    segments = list(renderable.__rich_console__(console, console.options))
    runs = chart._runs(40, 8)
    # One text segment per styled span, one newline segment per row: the
    # shape Textual's line-by-line painting survives.
    assert sum(1 for segment in segments if segment.text != "\n") == sum(len(row) for row in runs)
    assert sum(1 for segment in segments if segment.text == "\n") == 8


def test_price_chart_paints_in_a_running_app():
    """The chart reaches the screen at its own height, gridline and axes on.

    Also pins DEFAULT_CSS ordering: BrailleGraph's ``height: 1`` must not win
    over PriceChart's ``height: 8`` — at one row the axes would vanish and
    this chart would silently degrade to a sparkline.
    """
    from textual.app import App, ComposeResult

    class ChartApp(App):
        CSS = "PriceChart { width: 40; height: 8; }"

        def compose(self) -> ComposeResult:
            chart = PriceChart(id="c")
            chart.times = TIMES
            chart.data = RISING
            yield chart

    async def run():
        app = ChartApp()
        async with app.run_test(size=(60, 12)) as pilot:
            await pilot.pause()
            chart = app.query_one("#c")
            assert chart.size.height == 8
            frame = app.export_screenshot()
            assert sum(1 for ch in frame if 0x2800 <= ord(ch) <= 0x28FF) > 20
            assert "└" in frame and "┬" in frame
            assert "100.00" in frame and "0.00" in frame
            # Colour resolution ran: render() returns the real renderable.
            assert isinstance(chart.render(), _PriceChartRender)

    asyncio.run(run())
