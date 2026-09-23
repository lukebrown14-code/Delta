"""PriceChart tests: connected line, bounded scale, axes join, marker, colour.

Fully offline except the one painting test, which follows the BrailleGraph
pattern in ``test_tui.py``: assert on a real app's exported frame, not just
the renderable, because a green layout has hidden a widget Textual never
painted before.

These cover K1 (connected line), K2 (nice-bounded scale + gridlines), K3
(axis joins + ├ ticks), K4 (last-price marker), K5 (direction colour) and
K13 (pure ``_runs`` layout and the axes join).
"""

from __future__ import annotations

import asyncio
import io
from datetime import datetime, timedelta

from rich.color import Color as RichColor
from rich.console import Console

from delta.tui.axes import nice_ticks, plot_scale
from delta.tui.widgets import BrailleGraph, PriceChart, _PriceChartRender

RISING = [float(i) for i in range(100)]
#: One ISO stamp per point of RISING: a ~99 day span, so x_ticks picks the
#: "dd Mon" label format and can afford a middle tick.
TIMES = [(datetime(2026, 8, 8) + timedelta(days=i)).isoformat() for i in range(100)]


def _runs(chart: PriceChart, width: int = 40, height: int = 8) -> list[list[tuple[str, str]]]:
    return chart._runs(width, height)


def _row_text(chart: PriceChart, width: int, height: int) -> list[str]:
    return ["".join(text for text, _ in row) for row in _runs(chart, width, height)]


def test_runs_is_pure_and_structured():
    """K13: ``_runs`` returns per-line ``(text, kind)`` runs with no app state."""
    runs = _runs(PriceChart(RISING))
    assert len(runs) == 8  # 6 braille rows + rule row + label row
    assert all(isinstance(row, list) for row in runs)
    kinds = {kind for row in runs for _text, kind in row}
    assert kinds <= {"line", "marker", "grid", "axis", "blank"}


def test_connected_line_has_no_gaps():
    """K1: consecutive dot columns are joined, so the line is one stroke.

    A sparse series (fewer points than dot columns) is resampled so adjacent
    columns differ by at most a dot row — no vertical staircase of dashes.
    """
    for series in (RISING, RISING[:22], [10.0, 60.0, 20.0, 90.0, 40.0]):
        chart = PriceChart(series)
        dots = chart._dot_coords(64, 24, min(series), (max(series) - min(series)) or 1.0)
        columns = sorted({column for column, _row in dots})
        # Every column carries at least one dot (resampled, not bucketed flat).
        assert columns == list(range(64)), (len(series), columns)


def test_vertical_span_is_bridged_when_points_are_bucketed():
    """K1: a value that jumps between buckets is bridged by Bresenham dots."""
    # 200 points over the dot columns of a small plot: bucket mean kicks in,
    # and the vertical span between consecutive samples must be dotted.
    series = [0.0] * 100 + [100.0] * 100
    chart = PriceChart(series)
    dots = chart._dot_coords(40, 16, 0.0, 100.0)
    # The step from the flat 0 run to the flat 100 run is bridged vertically:
    # some column around the middle holds more than one dot row.
    by_column: dict[int, set[int]] = {}
    for column, row in dots:
        by_column.setdefault(column, set()).add(row)
    assert any(len(rows) > 1 for rows in by_column.values())


def test_bounded_scale_ticks_land_on_rows():
    """K2: scaling to the outer ticks puts every tick on a distinct row."""
    rows = _row_text(PriceChart(RISING), 40, 8)
    low, high = plot_scale(0.0, 99.0, 5)
    assert low == 0.0 and high == 100.0
    ticks = nice_ticks(0.0, 99.0, 5)
    assert ticks == [0.0, 25.0, 50.0, 75.0, 100.0]
    # The bottom and top rows carry the outer ticks (or the marker for the top,
    # since the last close lands there); the middle ticks sit on their rows.
    assert rows[5].rstrip().endswith("0.00")
    assert "├  50.00" in rows[2]
    assert "├  25.00" in rows[4]


def test_gridlines_drawn_behind_the_line():
    """K2: every tick row is a faint ``┄`` gridline the line dots ride over."""
    runs = _runs(PriceChart(RISING))
    grid_rows = {index for index, row in enumerate(runs) if any(k == "grid" for _t, k in row)}
    # The mid-tick rows carry ┄ grid cells; the line's own cells stay ``line``.
    for index in (0, 1, 2, 4, 5):
        row = runs[index]
        assert any("┄" in text for text, kind in row if kind == "grid")
    assert grid_rows


def test_axis_join_meets_the_gutter():
    """K3: the x-rule starts under plot column 0 and its ┘ meets the │ gutter."""
    rows = _row_text(PriceChart(RISING), 40, 8)
    rule_row = rows[6].rstrip()
    # With a gutter the rule is the plot width, no leading └, and ┘ sits under
    # the gutter's │ (column `plot` = width - gutter).
    assert rule_row.startswith("─") or rule_row.startswith("┬")
    assert rule_row.endswith("┘")
    # Braille rows carry the ├ tick (never ┤) on labelled rows.
    assert any("├" in row for row in rows[:6])


def test_last_price_marker_in_gutter():
    """K4: a ● marks the last close and carries its price in the gutter."""
    runs = _runs(PriceChart(RISING))
    marker_rows = [
        (index, text) for index, row in enumerate(runs) for text, kind in row if kind == "marker"
    ]
    assert marker_rows, "no last-price marker"
    _, text = marker_rows[-1]
    assert "●" in text and "99.00" in text


def test_direction_colour_classification():
    """K5: flat / rising / falling series classify their direction."""
    assert PriceChart([])._direction() == "-flat"
    assert PriceChart([1.0])._direction() == "-flat"
    assert PriceChart(RISING)._direction() == "-up"
    assert PriceChart(list(reversed(RISING)))._direction() == "-down"
    assert PriceChart([5.0, 5.0, 5.0])._direction() == "-flat"


def test_degenerate_series_do_not_crash():
    """Flat, single point and empty data: sane output, no divide by zero."""
    rows = PriceChart([5.0] * 50).rows(40, 8)
    assert len(rows) == 8
    # A flat series: every tick is the flat value, so a single gutter row owns
    # the ticks; the marker shows the flat price.
    assert "●" in "".join(rows)
    assert "5.00" in "".join(rows)

    assert len(PriceChart([7.0]).rows(40, 8)) == 8

    rows = PriceChart([]).rows(40, 8)
    assert len(rows) == 8
    assert set(rows[0]) == {BrailleGraph.EMPTY}
    assert rows[6].rstrip() == "└" + "─" * 38 + "┘"
    assert not rows[7].strip()

    # Under three rows there are no axes at all, just the bare line.
    bare = PriceChart(RISING).rows(20, 2)
    assert len(bare) == 2 and all(len(row) == 20 for row in bare)
    assert PriceChart([]).rows(20, 2) == [BrailleGraph.EMPTY * 20] * 2
    assert PriceChart(RISING).rows(0, 0) == []


def test_narrow_chart_hides_the_gutter():
    """At width 18 the labels would starve the plot below 12 cells: no gutter."""
    rows = PriceChart(RISING).rows(18, 8)
    assert all(len(row) == 18 for row in rows)
    assert all("├" not in row and "│" not in row for row in rows)
    assert all("0.00" not in row for row in rows)
    # The rule keeps a └…┘ frame instead of overflowing; the line stays.
    assert rows[6] == "└" + "─" * 16 + "┘"
    assert any(ch != BrailleGraph.EMPTY for row in rows[:6] for ch in row)


def test_y_format_is_honoured():
    """The gutter labels come from y_format, and the gutter resizes to them."""

    def points(value: float) -> str:
        return f"{value:.0f}p"

    chart = PriceChart(RISING)
    chart.y_format = points
    rows = chart.rows(40, 8)
    assert any("p" in row for row in rows[:6])
    assert all(len(row) == 40 for row in rows)


def test_x_axis_rows_with_and_without_times():
    """With times: ┬ rule and centred labels; without: a plain, silent rule."""
    chart = PriceChart(RISING)
    chart.times = TIMES
    rows = chart.rows(40, 8)
    rule_row = rows[6].rstrip()
    assert rule_row.count("┬") == 3
    assert rule_row.endswith("┘")

    plain = PriceChart(RISING).rows(40, 8)
    assert plain[7].strip() == ""


def test_renderable_yields_one_run_per_span_and_a_line_per_row():
    """_PriceChartRender follows the segment-run convention Textual survives."""
    chart = PriceChart(RISING)
    runs = chart._runs(40, 8)
    renderable = _PriceChartRender(
        runs,
        low=RichColor.parse("#5b8def"),
        high=RichColor.parse("#d4d4d4"),
        grid=RichColor.parse("#5c5c5c"),
        axis=RichColor.parse("#8a8a8a"),
        marker=RichColor.parse("#22c55e"),
        fill=False,
    )
    console = Console(width=40, file=io.StringIO(), legacy_windows=False)
    segments = list(renderable.__rich_console__(console, console.options))
    assert sum(1 for segment in segments if segment.text != "\n") == sum(len(row) for row in runs)
    assert sum(1 for segment in segments if segment.text == "\n") == 8


def test_axes_join_dimensions_are_consistent():
    """K13: at several widths/heights the ├ ticks and ┘ all line up under the
    gutter's │ and never overflow the requested width."""
    for width, height in ((40, 8), (60, 10), (30, 6), (80, 16)):
        rows = _row_text(PriceChart(RISING), width, height)
        assert all(len(row) <= width for row in rows)
        assert len(rows) == height
        # The rule row and the gutter │ rows agree: ┘ sits at column width-col.
        rule = rows[height - 2]
        gutter_rows = [row for row in rows[: height - 2] if "│" in row]
        if gutter_rows:
            gutter_col = next(i for i, ch in enumerate(gutter_rows[0]) if ch == "│")
            assert rule[gutter_col] == "┘", (width, height, rule, gutter_col)


def test_price_chart_paints_in_a_running_app():
    """The chart reaches the screen with gridline, axes and marker on."""
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
            assert "┬" in frame
            assert "●" in frame
            assert "50.00" in frame and "0.00" in frame
            assert isinstance(chart.render(), _PriceChartRender)

    asyncio.run(run())
