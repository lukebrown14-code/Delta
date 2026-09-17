"""Design-system widgets: panes, status dots, pills, key hints, dialogs.

The layout grammar is the square terminal box: every ``Pane`` draws a
border with its hotkey and title in the top edge and its key hints in the
bottom edge, and buttons are one-row ``ActionChip`` ``[key] label`` chips.
Colour always comes from theme-token CSS, never hex literals.

Layout lives in ``DEFAULT_CSS`` on these classes rather than in
``rigger.tcss`` so the screens that double as standalone panels (reports,
theses, chat) keep their shape when mounted under any App.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.markup import escape
from textual.screen import ModalScreen
from textual.widgets import Button, DataTable, Static

#: Keys whose Textual name is not what a user would recognise on a keycap.
KEY_DISPLAY = {"question_mark": "?", "escape": "esc", "slash": "/"}

DOT = "●"

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

    Subclasses ``Button`` so ``Button.Pressed`` handlers, ``disabled`` and
    focus keep working; only the chrome changes. ``-active`` marks the
    selected tab in a strip.
    """

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
    ActionChip:focus {
        background: $panel-lighten-2;
        text-style: none;
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
    """A ``[ key ]`` chip for empty states and prompts (markup disabled)."""

    def __init__(self, key: str, id: str | None = None) -> None:
        super().__init__(f"[ {key} ]", id=id, markup=False)


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
    dialog_hint: str = "<Esc>: close"
    dialog_width: int = 64

    DEFAULT_CSS = """
    Dialog {
        align: center middle;
        background: $background 60%;
    }
    Dialog > #dialog-frame {
        width: 64;
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
                yield Static(self.dialog_hint, id="dialog-hint", markup=False)

    def compose_dialog(self) -> ComposeResult:
        raise NotImplementedError
        yield  # pragma: no cover

    def action_dismiss_dialog(self) -> None:
        self.dismiss(None)


class RiggerTable(DataTable):
    """DataTable with the desk defaults: zebra stripes and a row cursor."""

    def on_mount(self) -> None:
        self.zebra_stripes = True
        self.cursor_type = "row"
