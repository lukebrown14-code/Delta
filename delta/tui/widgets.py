"""Design-system widgets: panes, status dots, pills, key hints, dialogs.

The layout grammar is the square terminal box: every ``Pane`` draws a
border with its hotkey and title in the top edge and its key hints in the
bottom edge, and buttons are one-row ``ActionChip`` ``[key] label`` chips.
Colour always comes from theme-token CSS, never hex literals.

Layout lives in ``DEFAULT_CSS`` on these classes rather than in
``delta.tcss`` so the screens that double as standalone panels (reports,
theses, chat) keep their shape when mounted under any App.
"""

from __future__ import annotations

from collections.abc import Callable, Sequence
from dataclasses import dataclass
from typing import Any, ClassVar

from rich.color import Color as RichColor
from rich.console import Console, ConsoleOptions
from rich.segment import Segment
from rich.style import Style
from textual.app import ComposeResult, RenderResult
from textual.binding import Binding
from textual.color import Color
from textual.containers import Horizontal, Vertical
from textual.markup import escape
from textual.reactive import reactive
from textual.screen import ModalScreen
from textual.widget import Widget
from textual.widgets import Button, DataTable, Static

from delta.tui.axes import nice_ticks, x_ticks

#: Keys whose Textual name is not what a user would recognise on a keycap.
KEY_DISPLAY = {"question_mark": "?", "escape": "esc", "slash": "/"}

DOT = "●"

#: The one modal width. Every dialog is this wide, so a modal never reads as a
#: different kind of window depending on what it holds.
MODAL_WIDTH = 64
#: The single documented exception: a four-column table (the model catalog's
#: id, context and two prices) truncates the id to uselessness at 64. Still
#: fits an 80-column terminal with the backdrop showing either side.
MODAL_WIDTH_WIDE = 72

_HEALTH_VARIANTS: dict[str, str] = {
    "emerging": "dim",
    "building": "ok",
    "mixed": "warn",
    "weakening": "warn",
    "challenged": "error",
    "idle": "dim",
}


def health_variant(state: str) -> str:
    """Map a thesis-health state to a Pill variant class."""
    return _HEALTH_VARIANTS.get(state, "dim")


def sentiment_variant(score: float) -> str:
    """Map a report sentiment (-1..1) to a Pill variant class.

    The bands match how the number is read on the desk: clearly negative, mixed
    (the honest default when evidence cuts both ways), clearly positive.
    """
    if score <= -0.3:
        return "error"
    if score >= 0.3:
        return "ok"
    return "warn"


def token_color(app: Any, token: str, default: str = "") -> str:
    """A theme token as a colour Rich can use, or ``default``.

    ``theme_variables`` carries values Rich cannot parse — Textual writes
    ``auto 87%`` for tokens whose colour depends on the background, and a
    screen mounted under a bare ``App`` (as the tests do) gets Textual's own
    defaults rather than Delta's. Passing one of those into a ``Text`` style
    raises ``MissingStyle`` at render time, which is a crash in a cell.
    """
    value = str(getattr(app, "theme_variables", {}).get(token, "") or "")
    if not value:
        return default
    try:
        RichColor.parse(value.split()[0])
    except Exception:
        return default
    return value


class Pane(Vertical):
    """Bordered panel: hotkey and title in the top border, key hints in the bottom.

    The square btop box is the app's one frame. ``key`` is the hotkey that
    reaches the pane (drawn bold in the title), ``badge`` a right-hand count
    or state (muted, after the title), ``hints`` the keys that act inside the
    pane (bottom border). A focused pane draws heavy in the accent so the eye
    finds it without a cursor.
    """

    DEFAULT_CSS = """
    Pane {
        height: 1fr;
        background: transparent;
        padding: 0;
        border: solid $border-blurred;
        border-title-color: $text-primary;
        border-title-style: bold;
        border-subtitle-color: $text-muted;
        border-subtitle-align: left;
    }
    Pane:focus-within {
        border: heavy $border;
    }
    Pane.-auto {
        height: auto;
    }
    """

    def __init__(
        self,
        *children,
        title: str = "",
        icon: str = "",
        badge: str = "",
        key: str = "",
        hints: str = "",
        id: str | None = None,
        classes: str = "",
    ) -> None:
        super().__init__(*children, id=id, classes=classes)
        # ``icon`` is accepted for compatibility and ignored: the frame has
        # no room for a glyph, and the Nerd Font icons render as boxes
        # without that font installed.
        self._title = title
        self._key = key
        self._badge = badge
        self._hints = hints
        self._paint()

    def _paint(self) -> None:
        parts = []
        if self._key:
            parts.append(f"[bold]{escape(self._key)}[/bold]")
        if self._title:
            parts.append(escape(self._title))
        if self._badge:
            parts.append(f"[$text-muted]· {escape(self._badge)}[/]")
        self.border_title = " ".join(parts)
        self.border_subtitle = self._hints

    def set_badge(self, text: str) -> None:
        self._badge = text
        self._paint()

    def set_title(self, title: str) -> None:
        self._title = title
        self._paint()

    def set_hints(self, hints: str) -> None:
        """``hints`` is markup: ``hint_markup`` builds it from key/label pairs."""
        self._hints = hints
        self._paint()


def hint_markup(*pairs: tuple[str, str]) -> str:
    """``[bold $text-primary]key[/] label`` pairs for a pane's bottom border."""
    return "  ".join(
        f"[bold $text-primary]{escape(key)}[/] {escape(label)}" for key, label in pairs
    )


class ActionChip(Button):
    """One-row ``[key] label`` button: the app's only button shape.

    Subclasses ``Button`` so ``Button.Pressed`` handlers and ``disabled``
    keep working; only the chrome changes. ``-active`` marks the selected
    tab in a strip. Chips advertise a key, so they never take focus: tab
    moves between panes, not across the hint row.
    """

    can_focus = False

    DEFAULT_CSS = """
    ActionChip {
        height: 1;
        width: auto;
        min-width: 0;
        border: none;
        padding: 0 1;
        margin: 0 1 0 0;
        background: $panel;
        color: $text-muted;
        text-style: none;
    }
    ActionChip:hover {
        background: $panel-lighten-1;
        border: none;
    }
    ActionChip.-active, ActionChip.-active:hover, ActionChip.-active:focus {
        background: $primary;
        color: $block-cursor-foreground;
    }
    ActionChip:disabled {
        color: $text-disabled;
        text-opacity: 100%;
    }
    ActionChip.-primary {
        color: $foreground;
    }
    """

    def __init__(
        self,
        key: str,
        label: str,
        id: str | None = None,
        classes: str = "",
        disabled: bool = False,
    ) -> None:
        self._key = key
        self._text = label
        super().__init__(self._markup(), id=id, classes=classes, disabled=disabled)

    def _markup(self) -> str:
        return f"[bold $text-primary]{escape(self._key)}[/] {escape(self._text)}"

    def set_text(self, label: str) -> None:
        self._text = label
        self.label = self._markup()


class PaneRow(Horizontal):
    """Horizontal split: panes separated by a gutter and a faint rule."""

    DEFAULT_CSS = """
    PaneRow {
        height: 1fr;
        background: transparent;
    }
    /* Boxes carry their own frame, so the row adds no gutter or rule.
       Stacked: panes share the height evenly and scroll their own
       overflow rather than clipping. */
    PaneRow.-narrow {
        layout: vertical;
    }
    PaneRow.-narrow > Pane {
        width: 1fr;
        height: 1fr;
        overflow-y: auto;
    }
    """

    #: Below this *row* width (not terminal width — the screen's own padding
    #: costs two columns) the row stacks its panes vertically. A side pane is
    #: ~34 columns, so an 80-column terminal still reads fine in two columns;
    #: stacking earlier costs more rows than it saves columns.
    NARROW_WIDTH = 72

    def __init__(self, *children, id: str | None = None, classes: str = "") -> None:
        super().__init__(*children, id=id, classes=f"{classes} pane-row".strip())

    def on_resize(self, event) -> None:
        self.set_class(event.size.width < self.NARROW_WIDTH, "-narrow")


class PaneStack(Vertical):
    """Vertical stack of bordered panes."""

    DEFAULT_CSS = """
    PaneStack {
        height: 1fr;
        background: transparent;
    }
    """


class StatusDot(Static):
    """A coloured bullet: ``-ok`` green, ``-warn`` amber, ``-error`` red."""

    def __init__(self, state: str = "ok", id: str | None = None) -> None:
        super().__init__(DOT, id=id, classes=f"-{state}")
        self._state = state

    @property
    def state(self) -> str:
        return self._state

    def set_state(self, state: str) -> None:
        if state != self._state:
            self.remove_class(f"-{self._state}")
            self.add_class(f"-{state}")
            self._state = state


class Pill(Static):
    """Small inline token for kinds, statuses and thesis health."""

    def __init__(
        self,
        text: str,
        variant: str | None = None,
        id: str | None = None,
        classes: str = "",
    ) -> None:
        variant_class = f"-{variant}" if variant else ""
        super().__init__(text, id=id, classes=f"{classes} {variant_class}".strip(), markup=False)
        self._variant = variant

    def set_variant(self, variant: str | None) -> None:
        if self._variant:
            self.remove_class(f"-{self._variant}")
        if variant:
            self.add_class(f"-{variant}")
        self._variant = variant


class KeyHint(Static):
    """The key alone, as a chip for empty states, prompts and key grids.

    The key is the bare keycap — no ``[ ]`` brackets, no ``<>``, no ``^``.
    Padding and a panel background are what make it read as a chip, so the app
    keeps exactly one key notation: the key, then what it does. ``hint_markup``
    and ``ActionChip`` spell the same pair inline.
    """

    def __init__(self, key: str, id: str | None = None) -> None:
        super().__init__(key, id=id, markup=False)


def shown_bindings(bindings: Sequence[Any]) -> list[Binding]:
    """Normalise a ``BINDINGS`` list to the ``Binding`` objects worth showing.

    ``BINDINGS`` entries may be 3-tuples or ``Binding`` instances; this hides
    the difference so a caller can render a keymap from either form without
    reaching into Textual's private binding registry.
    """
    shown: list[Binding] = []
    for entry in bindings:
        binding = entry if isinstance(entry, Binding) else Binding(*entry)
        if binding.show:
            shown.append(binding)
    return shown


def binding_key(binding: Binding) -> str:
    """The key as a user would recognise it on a keycap."""
    return KEY_DISPLAY.get(binding.key) or binding.key_display or binding.key


class KeyStrip(Horizontal):
    """One-line key hint strip generated from a ``BINDINGS`` list.

    Generated rather than written out, so a screen's hints cannot drift from
    the keys it actually binds. Chips are dropped from the right when the strip
    would not fit, because a wrapped strip costs a row of pane height.
    """

    DEFAULT_CSS = """
    KeyStrip {
        height: 1;
        width: 1fr;
        color: $text-muted;
    }
    KeyStrip Static {
        width: auto;
        height: 1;
        margin: 0 2 0 0;
        color: $text-muted;
    }
    KeyStrip .ks-key {
        color: $text-primary;
        text-style: bold;
        margin: 0 1 0 0;
    }
    """

    #: Margin columns a chip costs on top of its text: one between the key and
    #: its description, two after the pair. Mirrors the margins in DEFAULT_CSS.
    CHIP_GAP = 3

    def __init__(self, bindings: Sequence[Any], id: str | None = None) -> None:
        super().__init__(id=id)
        self._pairs = [
            (binding_key(binding), binding.description) for binding in shown_bindings(bindings)
        ]

    def compose(self) -> ComposeResult:
        for key, description in self._pairs:
            yield Static(key, classes="ks-key", markup=False)
            yield Static(description, markup=False)

    def on_resize(self, event) -> None:
        """Hide the chips that would not fit rather than wrapping the strip.

        Every chip is re-evaluated on every resize: an early "they all fit"
        return would never restore chips hidden at a narrower width.
        """
        chips = list(self.query(Static))
        used = 0
        for index, (key, description) in enumerate(self._pairs):
            used += len(key) + len(description) + self.CHIP_GAP
            fits = used <= event.size.width
            chips[index * 2].display = fits
            chips[index * 2 + 1].display = fits


class KeyGrid(Vertical):
    """Two-column key/description grid, the modal keymap pattern."""

    DEFAULT_CSS = """
    KeyGrid {
        height: auto;
        layout: grid;
        grid-size: 4;
        grid-columns: 8 1fr 8 1fr;
        grid-rows: 1;
        grid-gutter: 0 1;
    }
    KeyGrid KeyHint {
        width: auto;
        height: 1;
    }
    KeyGrid .kg-desc {
        height: 1;
        color: $foreground;
    }
    """

    def __init__(self, items: list[tuple[str, str]], id: str | None = None) -> None:
        super().__init__(id=id)
        self._items = items
        rows = (len(items) + 1) // 2
        self.styles.height = max(rows, 1)

    def compose(self) -> ComposeResult:
        rows = (len(self._items) + 1) // 2
        left = self._items[:rows]
        right = self._items[rows:]
        for index in range(rows):
            key, desc = left[index]
            yield KeyHint(key)
            yield Static(desc, classes="kg-desc", markup=False)
            if index < len(right):
                key, desc = right[index]
                yield KeyHint(key)
                yield Static(desc, classes="kg-desc", markup=False)
            else:
                yield Static("", classes="kg-desc", markup=False)
                yield Static("", classes="kg-desc", markup=False)


class Dialog(ModalScreen):
    """Centred floating dialog over a dimmed backdrop.

    Subclasses implement ``compose_dialog`` and set ``dialog_title`` /
    ``dialog_hint``; ``dialog_width`` sizes the frame.
    """

    BINDINGS = [("escape", "dismiss_dialog", "Close")]

    dialog_title: str = ""
    #: Console markup: build it with ``hint_markup`` so a dialog's hint row
    #: spells keys exactly the way every pane's bottom border does.
    dialog_hint: str = hint_markup(("esc", "close"))
    dialog_width: int = MODAL_WIDTH

    DEFAULT_CSS = """
    Dialog {
        align: center middle;
        background: $background 60%;
    }
    Dialog > #dialog-frame {
        /* Width is set inline from ``dialog_width`` (MODAL_WIDTH by default);
           CSS cannot read the constant. */
        height: auto;
        max-height: 90%;
        padding: 1 2;
        background: $surface;
        border: solid $border-blurred;
    }
    Dialog #dialog-title {
        height: 1;
        margin: 0 0 1 0;
        color: $text-primary;
        text-style: bold;
        content-align-horizontal: center;
    }
    Dialog #dialog-hint {
        height: 1;
        margin: 1 0 0 0;
        color: $text-muted;
        content-align-horizontal: center;
    }
    """

    def compose(self) -> ComposeResult:
        with Vertical(id="dialog-frame") as frame:
            frame.styles.width = self.dialog_width
            if self.dialog_title:
                yield Static(self.dialog_title, id="dialog-title", markup=False)
            yield from self.compose_dialog()
            if self.dialog_hint:
                yield Static(self.dialog_hint, id="dialog-hint")

    def compose_dialog(self) -> ComposeResult:
        raise NotImplementedError
        yield  # pragma: no cover

    def action_dismiss_dialog(self) -> None:
        self.dismiss(None)


class BrailleGraph(Widget):
    """A price graph drawn in braille: 2x4 dots per cell, not one block glyph.

    A block sparkline gives eight levels of height in one row. A braille cell
    addresses two dot columns and four dot rows, so a ``W x H`` box carries
    ``4H`` levels and ``2W`` sample points — enough for the shape of a month
    of closes to be legible in six rows, which is what the pane already spends.

    ``data`` is the series; assigning to it repaints. ``fill`` draws the area
    under the line (for one-row sparklines, where a bare line is too sparse);
    the default line reads as a chart. Colour comes from the two component
    classes, low to high, so CSS keeps the ``$primary``/``$text-primary`` pair
    the rest of the app uses.
    """

    COMPONENT_CLASSES: ClassVar[set[str]] = {
        "braille-graph--low-color",
        "braille-graph--high-color",
    }

    DEFAULT_CSS = """
    BrailleGraph {
        height: 1;
    }
    BrailleGraph > .braille-graph--low-color { color: $primary; }
    BrailleGraph > .braille-graph--high-color { color: $text-primary; }
    """

    #: Bit of the braille cell for each (dot column, dot row). The fourth row
    #: is the 8-dot extension, hence 0x40/0x80 rather than a run.
    DOTS: ClassVar[tuple[tuple[int, ...], ...]] = (
        (0x01, 0x02, 0x04, 0x40),
        (0x08, 0x10, 0x20, 0x80),
    )
    #: The empty braille cell. Deliberately not ``BLANK``: that name is a
    #: Textual ``Widget`` attribute meaning "paint nothing", and shadowing it
    #: with a truthy string makes the widget render blank without ever
    #: calling ``render()``.
    EMPTY = "\u2800"

    data: reactive[list[float]] = reactive(list, layout=True)

    def __init__(
        self,
        data: Sequence[float] | None = None,
        *,
        fill: bool = False,
        id: str | None = None,
        classes: str = "",
    ) -> None:
        super().__init__(id=id, classes=classes)
        self.fill = fill
        self.data = list(data or [])

    def _sample(self, columns: int) -> list[float]:
        """One value per dot column: the mean of its bucket, never a dropped point."""
        series = self.data
        per = len(series) / columns
        return [
            sum(chunk) / len(chunk)
            for x in range(columns)
            for chunk in (series[int(x * per) : max(int((x + 1) * per), int(x * per) + 1)],)
            if chunk
        ]

    def _resample(self, points: list[float], width: int) -> list[float]:
        """Linearly stretch ``points`` to exactly ``width`` values (K1).

        When there are fewer closes than dot columns, the bucket mean repeats
        each close across several columns as a flat, gappy run. Resampling
        instead interpolates a point per column so consecutive columns differ
        by at most a dot row and the line reads as continuous.
        """
        if len(points) == width:
            return points
        if len(points) < 2 or width < 1:
            return points
        step = (len(points) - 1) / (width - 1)
        return [self._lerp(points, index * step) for index in range(width)]

    @staticmethod
    def _lerp(points: list[float], pos: float) -> float:
        """Linear interpolation into ``points`` at fractional index ``pos``."""
        low = int(pos)
        high = min(low + 1, len(points) - 1)
        frac = pos - low
        return points[low] + (points[high] - points[low]) * frac

    @staticmethod
    def _bresenham(x0: int, y0: int, x1: int, y1: int, dots: dict) -> None:
        """Mark every dot on the integer line from (x0, y0) to (x1, y1) (K1)."""
        dx = abs(x1 - x0)
        dy = -abs(y1 - y0)
        sx = 1 if x0 < x1 else -1
        sy = 1 if y0 < y1 else -1
        err = dx + dy
        while True:
            dots[(x0, y0)] = True
            if x0 == x1 and y0 == y1:
                return
            double = 2 * err
            if double >= dy:
                err += dy
                x0 += sx
            if double <= dx:
                err += dx
                y0 += sy

    def _scaled_points(self, dot_columns: int) -> list[float]:
        """One value per dot column: resampled when sparse, else the bucket mean.

        Fewer closes than columns resample so the points join; more closes than
        columns fall back to the bucket mean (each column is one representative
        value). Either way the caller dots the vertical span between samples.
        """
        series = self.data
        if len(series) > dot_columns:
            return self._sample(dot_columns)
        return self._resample(list(series), dot_columns)

    def _dot_coords(self, dot_columns: int, dot_rows: int, low: float, span: float) -> dict:
        """Connected ``(dot column, dot row)`` coordinates for the whole series.

        Plots one dot per column and Bresenham-joins consecutive columns so the
        line is continuous (K1) regardless of how many source points exist.
        """
        dots: dict = {}
        previous: tuple[int, int] | None = None
        for column, value in enumerate(self._scaled_points(dot_columns)):
            row = max(
                0, min(dot_rows - 1, dot_rows - 1 - round((value - low) / span * (dot_rows - 1)))
            )
            if previous is None:
                dots[(column, row)] = True
            else:
                self._bresenham(previous[0], previous[1], column, row, dots)
            previous = (column, row)
        return dots

    def _cell_grid(self, columns: int, rows: int, low: float, span: float) -> list[list[int]]:
        """Braille cell bitmaps for a connected line over ``low``/``span``.

        Shared by :meth:`BrailleGraph.rows` and :meth:`PriceChart._runs`: the
        connected dot coordinates (with the optional area fill) OR-ed into
        ``columns`` braille cells of ``rows`` cells tall.
        """
        dot_rows = rows * 4
        dot_columns = columns * 2
        dots = self._dot_coords(dot_columns, dot_rows, low, span)
        if self.fill:
            column_tops: dict[int, int] = {}
            for column, row in dots:
                column_tops[column] = min(column_tops.get(column, dot_rows), row)
            for column, top in column_tops.items():
                for row in range(top, dot_rows):
                    dots[(column, row)] = True
        grid = [[0] * columns for _ in range(rows)]
        for column, row in dots:
            grid[row // 4][column // 2] |= self.DOTS[column % 2][row % 4]
        return grid

    def rows(self, width: int, height: int) -> list[str]:
        """The graph as ``height`` strings of ``width`` braille cells."""
        blank = [self.EMPTY * width for _ in range(height)]
        if not self.data or width < 1 or height < 1:
            return blank
        low, high = min(self.data), max(self.data)
        span = (high - low) or 1.0
        grid = self._cell_grid(width, height, low, span)
        return ["".join(chr(0x2800 + cell) for cell in row) for row in grid]

    def render(self) -> RenderResult:
        base = self.background_colors[1]
        return _BrailleRender(
            self.rows(self.size.width, self.size.height),
            low=(base + self.get_component_styles("braille-graph--low-color").color).rich_color,
            high=(base + self.get_component_styles("braille-graph--high-color").color).rich_color,
            fill=self.fill,
        )


@dataclass
class _BrailleRender:
    """Segments for :class:`BrailleGraph`.

    A Rich renderable rather than a multi-line ``Text``: Textual paints a
    widget line by line, and only a renderable that yields one segment run
    per line survives that (the convention ``Sparkline`` also follows).
    """

    rows: list[str]
    low: RichColor
    high: RichColor
    fill: bool

    def __rich_console__(self, console: Console, options: ConsoleOptions) -> RenderResult:
        rows = self.rows
        low, high = Color.from_rich_color(self.low), Color.from_rich_color(self.high)
        for index, row in enumerate(rows):
            # A filled area is a mass: shade it low to high up the box so its
            # bright top edge reads as the line. A bare line has no mass to
            # shade — gradient it and the bottom half drops to 2.5:1 and
            # vanishes — so it stays on the bright colour throughout.
            ratio = 1.0 if not self.fill or len(rows) == 1 else 1 - index / (len(rows) - 1)
            yield Segment(row, Style(color=low.blend(high, ratio).rich_color))
            yield Segment.line()


def _default_y_format(value: float) -> str:
    """The default Y tick label: thousands-separated, two decimals."""
    return f"{value:,.2f}"


class PriceChart(BrailleGraph):
    """A braille price line with a right-hand Y gutter and an X date axis.

    The inspector's chart: the same dot grid as :class:`BrailleGraph` plus the
    axes a price needs — Y ticks in a right gutter, a faint ``┄`` gridline on
    each tick row, and up to three date labels on a ``┬`` rule. A ``●`` marks
    the last close and carries its price in the gutter: with a connected line
    (K1) the marker no longer fights the dots, so it can answer "where is it
    now?" instead of leaving that to the figures above.

    Callers set ``times`` and ``y_format`` first, then assign ``data`` (or call
    ``refresh()``): the ``data`` assignment is the reactive that repaints, so a
    series set before its labels would repaint once under stale settings.
    ``times`` are ISO stamps aligned with ``data`` and may be empty (the rule
    row goes plain).

    Sizing: ``H`` rows of height give ``H - 2`` braille rows plus the rule and
    label rows; under three rows it degrades to a bare braille line. The
    gutter is ``2 + widest tick label`` cells wide and hides before it would
    squeeze the plot under 12 cells. The line reuses BrailleGraph's component
    classes; ``price-chart--grid`` draws the faint ``┄`` gridline and unused
    gutter rules, ``price-chart--axis`` the rule row and tick labels, and
    ``price-chart--marker`` the last-price bullet and its gutter label.
    """

    COMPONENT_CLASSES: ClassVar[set[str]] = {
        "price-chart--grid",
        "price-chart--axis",
        "price-chart--marker",
    }

    DEFAULT_CSS = """
    PriceChart {
        height: 8;
    }
    PriceChart > .braille-graph--low-color { color: $primary; }
    PriceChart > .braille-graph--high-color { color: $text-primary; }
    PriceChart > .price-chart--grid { color: $text-disabled; }
    PriceChart > .price-chart--axis { color: $text-muted; }
    PriceChart > .price-chart--marker { color: $text-primary; }
    """

    def __init__(
        self,
        data: Sequence[float] | None = None,
        *,
        id: str | None = None,
        classes: str = "",
    ) -> None:
        super().__init__(id=id, classes=classes)
        self.times: list[str] = []
        self.y_format: Callable[[float], str] = _default_y_format
        # Last: the reactive assignment is what schedules the first repaint.
        self.data = list(data or [])

    def rows(self, width: int, height: int) -> list[str]:
        """The chart as ``height`` strings of ``width`` columns.

        Unlike :class:`BrailleGraph` (whose width is braille cells), width
        here is widget columns: the braille plot plus the gutter when shown.
        """
        return ["".join(text for text, _ in row) for row in self._runs(width, height)]

    @staticmethod
    def _tick_count(plot_rows: int) -> int:
        """How many Y ticks fit the plot: at least 3, at most 5 (K2)."""
        return max(3, min(5, plot_rows))

    def _runs(self, width: int, height: int) -> list[list[tuple[str, str]]]:
        """Layout as per-line ``(text, style kind)`` runs, ready to paint.

        Pure — no app state — so tests assert layout without mounting an App.
        Kinds: ``line`` (the price line), ``marker`` (the last-price bullet and
        its gutter label), ``grid`` (faint gridline and unused gutter rules),
        ``axis`` (rule row, tick labels) and ``blank`` (empty cells).
        """
        if width < 1 or height < 1:
            return []
        if height < 3:
            # No room for the rule and label rows: a bare line, BrailleGraph's
            # own shape.
            if not self.data:
                return [[(self.EMPTY * width, "line")] for _ in range(height)]
            low, high = min(self.data), max(self.data)
            cells = self._cell_grid(width, height, low, (high - low) or 1.0)
            return [[("".join(chr(0x2800 + cell) for cell in row), "line")] for row in cells]

        plot_rows = height - 2
        dot_rows = plot_rows * 4
        if not self.data:
            # Nothing to scale: blank rows over a bare rule keep the pane's
            # height rhythm instead of inventing an axis for nothing.
            return (
                [[(self.EMPTY * width, "blank")] for _ in range(plot_rows)]
                + [[("└" + "─" * max(width - 2, 0) + "┘", "axis")]]
                + [[(" " * width, "blank")]]
            )

        low, high = min(self.data), max(self.data)
        tick_count = self._tick_count(plot_rows)
        ticks = nice_ticks(low, high, tick_count)
        # K2: scale to the outer ticks so every tick lands exactly on a row.
        plot_low, plot_high = ticks[0], ticks[-1]
        span = (plot_high - plot_low) or 1.0
        labels = [self.y_format(tick) for tick in ticks]
        gutter = 2 + max(map(len, labels))
        plot = width - gutter
        if plot < 12:
            # A gutter that starves the line hides entirely: the shape carries
            # the meaning, the labels are secondary. Real price labels make
            # this every width under 20.
            plot, gutter = width, 0
        cells = self._cell_grid(plot, plot_rows, plot_low, span)
        # One label per text row: a tick owns the gutter row its dot lands on.
        tick_rows: dict[int, str] = {}
        for tick, label in zip(ticks, labels, strict=True):
            row = (dot_rows - 1 - round((tick - plot_low) / span * (dot_rows - 1))) // 4
            tick_rows[row] = label
        # The last close marks its gutter row with a bullet and price (K4).
        marker_row = (dot_rows - 1 - round((self.data[-1] - plot_low) / span * (dot_rows - 1))) // 4
        marker_row = max(0, min(plot_rows - 1, marker_row))
        marker_label = self.y_format(self.data[-1])
        runs: list[list[tuple[str, str]]] = []
        for index in range(plot_rows):
            on_tick = index in tick_rows
            row_runs: list[tuple[str, str]] = []
            parts: list[str] = []
            kind = ""
            for cell in cells[index]:
                # A braille cell is one glyph with one style, so a cell the
                # line dotted yields to the line whole: the gridline stops at
                # those cells rather than repainting line dots as grid.
                text, cell_kind = (
                    (chr(0x2800 + cell), "line")
                    if cell
                    else ("┄" if on_tick else self.EMPTY, "grid" if on_tick else "blank")
                )
                if cell_kind == kind:
                    parts.append(text)
                    continue
                if parts:
                    row_runs.append(("".join(parts), kind))
                parts, kind = [text], cell_kind
            if parts:
                row_runs.append(("".join(parts), kind))
            if gutter:
                if index == marker_row:
                    row_runs.append(("● " + marker_label.rjust(gutter - 2), "marker"))
                else:
                    label = tick_rows.get(index)
                    row_runs.append(
                        ("├ " + label.rjust(gutter - 2), "axis")  # K3: ├ ticks, not ┤
                        if label is not None
                        else ("│" + " " * (gutter - 1), "grid")
                    )
            runs.append(row_runs)
        # Rule and label rows. Without the gutter the rule shrinks to fit the
        # widget; the line stays full width.
        xaxis = x_ticks(self.times, plot)
        rule_cells = plot if gutter else max(width - 2, 0)
        rule = ["─"] * rule_cells
        for column, _ in xaxis:
            if column < rule_cells:
                rule[column] = "┬"
        # K3: with a gutter the ┘ meets the Y gutter │ exactly under column 0;
        # without one the └ roots an otherwise free-standing rule.
        rule_row = "".join(rule) + "┘" if gutter else "└" + "".join(rule) + "┘"
        runs.append([(rule_row, "axis"), (" " * max(width - len(rule_row), 0), "blank")])
        canvas: list[str] = [" "] * plot
        cursor = 0
        for column, label in xaxis:
            # Centre each label on its tick, clamped inside the plot and past
            # the label before it; x_ticks spaces ticks so this rarely bites.
            start = max(min(column - len(label) // 2, plot - len(label)), cursor)
            canvas[start : start + len(label)] = label
            cursor = start + len(label)
        runs.append([("".join(canvas) + " " * max(width - plot, 0), "axis")])
        return runs

    def _direction(self) -> str:
        """``-up`` / ``-down`` / ``-flat`` over the displayed range (K5)."""
        if len(self.data) < 2:
            return "-flat"
        first, last = self.data[0], self.data[-1]
        if last > first:
            return "-up"
        if last < first:
            return "-down"
        return "-flat"

    def render(self) -> RenderResult:
        base = self.background_colors[1]
        # K5: the line takes the direction colour (up/down/flat) instead of the
        # neutral primary, so a glance reads the range without the figures.
        token = {
            "-up": "text-success",
            "-down": "text-error",
            "-flat": "text-accent",
        }[self._direction()]
        direction = token_color(self.app, token, "#d4d4d4")
        try:
            line = (base + Color.parse(direction)).rich_color
        except Exception:
            line = base.rich_color
        return _PriceChartRender(
            self._runs(self.size.width, self.size.height),
            # A single colour on both ends keeps a bare line flat rather than a
            # gradient that fades its bottom half to nothing.
            low=line,
            high=line,
            marker=line,
            grid=(base + self.get_component_styles("price-chart--grid").color).rich_color,
            axis=(base + self.get_component_styles("price-chart--axis").color).rich_color,
            fill=self.fill,
        )


@dataclass
class _PriceChartRender:
    """Segments for :class:`PriceChart`.

    The same per-line convention as :class:`_BrailleRender` — one segment run
    per styled span, one ``Segment.line()`` per row — because Textual paints a
    widget line by line and only that survives. Rows carry ``(text, kind)``
    runs: a gridline breaking around the line means a braille row can need two
    differently-styled runs, which a single-colour string cannot express.
    """

    rows: list[list[tuple[str, str]]]
    low: RichColor
    high: RichColor
    grid: RichColor
    axis: RichColor
    marker: RichColor
    fill: bool

    def __rich_console__(self, console: Console, options: ConsoleOptions) -> RenderResult:
        low, high = Color.from_rich_color(self.low), Color.from_rich_color(self.high)
        flat = {
            "grid": Style(color=self.grid),
            "axis": Style(color=self.axis),
            "marker": Style(color=self.marker),
            "blank": Style(),
        }
        for index, row in enumerate(self.rows):
            # BrailleGraph's per-row blend; a bare line has no mass to shade,
            # so it rides the bright end unless filled.
            ratio = (
                1.0 if not self.fill or len(self.rows) == 1 else 1 - index / (len(self.rows) - 1)
            )
            line = Style(color=low.blend(high, ratio).rich_color)
            for text, kind in row:
                yield Segment(text, flat.get(kind, line))
            yield Segment.line()


class DeltaTable(DataTable):
    """DataTable with the desk defaults: zebra stripes and a row cursor."""

    def on_mount(self) -> None:
        self.zebra_stripes = True
        self.cursor_type = "row"
