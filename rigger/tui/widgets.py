"""Design-system widgets: bordered panes, chips, key hints, dialogs.

One visual language across every screen: square btop boxes with the title in
the top border (hotkey accented) and hints in the bottom border, plus
``[key] label`` action chips instead of stock Textual buttons.

Layout lives in ``DEFAULT_CSS`` on these classes rather than in
``rigger.tcss`` so the screens that double as standalone panels (reports,
theses, chat) keep their shape when mounted under any App.
"""

from __future__ import annotations

from collections.abc import Sequence
from typing import Any, TypeVar

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.screen import ModalScreen
from textual.widgets import DataTable, Static

#: Keys whose Textual name is not what a user would recognise on a keycap.
KEY_DISPLAY = {"question_mark": "?", "escape": "esc", "slash": "/"}


def key_chip(key: str) -> str:
    """Render a key in the app-wide ``[ a ]`` notation."""
    return f"[ {KEY_DISPLAY.get(key, key)} ]"


#: One width for every dialog frame. Previously six (64/64/64/72/80/84).
MODAL_WIDTH = 64

#: The single responsive breakpoint for pane rows, in columns. Replaces the
#: four scattered values (72 / 81 / 100 / 109) with one rule: below this row
#: width the panes stack vertically.
BREAKPOINT_NARROW = 100

#: Evidence-kind colours. Names match ``evidence.py``'s kind axis; values are
#: literal hexes only as a Python source of truth — styling still goes
#: through theme tokens (``$news`` etc.), never hex in CSS.
EVIDENCE_KIND_COLORS: dict[str, str] = {
    "news": "#5ccfe6",
    "filing": "#ffd580",
    "bar": "#7fd962",
    "fundamental": "#cbccc6",
    "event": "#c39ac9",
}

#: Internal ``raw`` keys kept out of the evidence preview body.
PREVIEW_META_KEYS = ("prompt_version", "extracted_by")

#: Hash-looking values longer than this are truncated in the preview, with the
#: head kept so the reader still gets a fingerprint to grep for.
PREVIEW_HASH_WIDTH = 12

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


class PaneBar(Horizontal):
    """Legacy inline title row, kept for backwards compatibility.

    New code uses ``Pane``'s native ``border_title`` / ``border_subtitle``
    instead; this remains so older standalone mounts do not break.
    """

    DEFAULT_CSS = """
    PaneBar {
        height: 1;
        background: transparent;
    }
    PaneBar .pane-title {
        width: auto;
        color: $primary;
        text-style: bold;
    }
    PaneBar .pane-badge {
        width: 1fr;
        content-align-horizontal: right;
        color: $text-muted;
    }
    """

    def __init__(self, title: str, icon: str = "", badge: str = "") -> None:
        super().__init__()
        label = f"{icon} {title}".strip()
        self._title = Static(label, classes="pane-title", markup=False)
        self._badge = Static(badge, classes="pane-badge", markup=False)

    def compose(self) -> ComposeResult:
        yield self._title
        yield self._badge

    def set_badge(self, text: str) -> None:
        self._badge.update(text)


class Pane(Vertical, can_focus=True):
    """Bordered btop box: title in the top border, hints in the bottom.

    The hotkey is accented via console markup in ``border_title`` — no custom
    rendering needed. Focus switches the frame to ``heavy $primary``. The
    badge (counts, state) renders right-aligned in the bottom border.
    """

    DEFAULT_CSS = """
    Pane {
        height: 1fr;
        background: transparent;
        border: solid $panel;
        padding: 0 1;
    }
    Pane:focus-within {
        border: heavy $primary;
    }
    Pane.-auto {
        height: auto;
    }
    """

    def __init__(
        self,
        *children: Any,
        title: str = "",
        key: str = "",
        hints: str = "",
        badge: str = "",
        icon: str = "",
        id: str | None = None,
        classes: str = "",
    ) -> None:
        self._title_text = f"{icon} {title}".strip() if icon else title
        self._key = key
        self._hints = hints
        self._badge_text = badge
        # Passed as first children (not composed) so the ``with Pane(...)``
        # context-manager form appends content after any explicit children.
        super().__init__(*children, id=id, classes=classes)
        self._refresh_borders()

    def _refresh_borders(self) -> None:
        if self._key and self._title_text:
            title = self._title_text.removeprefix(f"{self._key} ").removeprefix(self._key)
            title = title.strip()
            self.border_title = f"[$accent]{self._key}[/] {title}"
        elif self._title_text:
            self.border_title = self._title_text
        else:
            self.border_title = ""
        left = self._hints
        right = self._badge_text
        if left and right:
            gap = "   "
            self.border_subtitle = f"{left}{gap}{right}"
        else:
            self.border_subtitle = left or right

    def set_badge(self, text: str) -> None:
        self._badge_text = text
        self._refresh_borders()

    def set_hints(self, hints: str) -> None:
        self._hints = hints
        self._refresh_borders()


class PaneRow(Horizontal):
    """Two bordered panes side by side, sharing the row's height.

    Below ``NARROW_WIDTH`` the panes stack; nested ``PaneStack`` children drop
    their top border so stacked panes never double up.
    """

    DEFAULT_CSS = """
    PaneRow {
        height: 1fr;
        background: transparent;
    }
    PaneRow > Pane {
        margin: 0 1 0 0;
    }
    PaneRow > Pane:last-of-type {
        margin: 0;
    }
    /* Stacked: panes share the height evenly and scroll their own overflow
       rather than clipping. */
    PaneRow.-narrow {
        layout: vertical;
    }
    PaneRow.-narrow > Pane {
        width: 1fr;
        height: 1fr;
        margin: 0 0 1 0;
        overflow-y: auto;
    }
    PaneRow.-narrow > Pane:last-of-type {
        margin: 0;
    }
    """

    #: Below this *row* width the row stacks its panes vertically. Set above
    #: the old 72 so an 80-column terminal stacks instead of squeezing two
    #: bordered panes into unreadable slivers.
    NARROW_WIDTH = BREAKPOINT_NARROW

    def __init__(self, *children: Any, id: str | None = None, classes: str = "") -> None:
        super().__init__(*children, id=id, classes=f"{classes} pane-row".strip())

    def on_resize(self, event: Any) -> None:
        self.set_class(event.size.width < self.NARROW_WIDTH, "-narrow")


class PaneStack(Vertical):
    """Vertical stack of bordered panes.

    Children drop their top border edge so nested panes never double up.
    """

    DEFAULT_CSS = """
    PaneStack {
        height: 1fr;
        background: transparent;
    }
    PaneStack > Pane {
        margin: 0 0 1 0;
    }
    PaneStack > Pane:last-of-type {
        margin: 0;
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

    DEFAULT_CSS = """
    KeyHint {
        width: auto;
        height: 1;
        padding: 0 1;
        background: $panel;
        color: $primary;
        text-style: bold;
    }
    """

    def __init__(self, key: str, id: str | None = None) -> None:
        super().__init__(key_chip(key), id=id, markup=False)


class ActionChip(Static, can_focus=True):
    """A clickable one-row ``[key] label`` chip replacing every Button.

    Posts ``Selected`` on click or enter, the way ``NavKey.on_click`` does,
    so screens handle chips and keys through one path. Resting / active /
    disabled states come from ``-active`` / ``-disabled`` classes; a disabled
    chip keeps its reason in the label (``[n] generate — needs evidence``).
    """

    class Selected(Message):
        """Posted when the chip is activated by click or key."""

        def __init__(self, action: str) -> None:
            super().__init__()
            self.action = action

    DEFAULT_CSS = """
    ActionChip {
        width: auto;
        height: 1;
        padding: 0 1;
        margin: 0 1 0 0;
        background: $panel;
        color: $foreground;
    }
    ActionChip > .chip-inner {
        width: auto;
        height: 1;
    }
    ActionChip .chip-key {
        color: $primary;
        text-style: bold;
    }
    ActionChip.-active {
        background: $primary;
        color: $background;
    }
    ActionChip.-active .chip-key {
        color: $background;
    }
    ActionChip.-disabled {
        color: $text-muted;
    }
    ActionChip.-disabled .chip-key {
        color: $text-muted;
    }
    ActionChip:focus {
        text-style: underline;
    }
    """

    BINDINGS = [("enter", "select", "Select")]

    def __init__(
        self,
        key: str,
        label: str,
        action: str,
        *,
        active: bool = False,
        disabled: bool = False,
        reason: str = "",
        id: str | None = None,
    ) -> None:
        self.chip_key = key
        self.chip_label = label
        self.action = action
        self._active = active
        self._disabled = disabled
        self._reason = reason
        super().__init__(self._text(), markup=False, id=id)

    def _text(self) -> Any:
        from rich.text import Text

        text = Text()
        text.append(key_chip(self.chip_key), style="bold")
        text.append(f" {self.chip_label}")
        if self._disabled and self._reason:
            text.append(f" — {self._reason}")
        return text

    def _paint(self) -> None:
        self.update(self._text())
        self.set_class(self._active, "-active")
        self.set_class(self._disabled, "-disabled")

    @property
    def is_disabled(self) -> bool:
        return self._disabled

    def set_active(self, active: bool) -> None:
        self._active = active
        self._paint()

    def set_disabled(self, disabled: bool, reason: str = "") -> None:
        self._disabled = disabled
        if reason:
            self._reason = reason
        self._paint()

    def on_click(self) -> None:
        if not self._disabled:
            self.post_message(ActionChip.Selected(self.action))

    def action_select(self) -> None:
        self.on_click()


class ChipRow(Horizontal):
    """One row of screen-wide action chips."""

    DEFAULT_CSS = """
    ChipRow {
        height: 1;
        width: 1fr;
        layout: horizontal;
        overflow: hidden hidden;
    }
    ChipRow > * {
        width: auto;
        height: 1;
    }
    """


class TabStrip(Horizontal):
    """Two-tab switcher for Evidence / Report.

    Active tab is filled ``$primary``; inactive is muted. Replaces the
    hand-toggled ``-tab-active`` Button hack, which collided with the
    permanently-primary generate button so two controls read as primary.
    """

    class Selected(Message):
        """Posted when a tab is picked."""

        def __init__(self, tab: str) -> None:
            super().__init__()
            self.tab = tab

    DEFAULT_CSS = """
    TabStrip {
        height: 1;
        width: auto;
    }
    TabStrip > ActionChip {
        width: auto;
        height: 1;
    }
    """

    def __init__(self, tabs: Sequence[tuple[str, str, str]], active: str = "") -> None:
        """``tabs`` is ``(tab_id, key, label)`` triples."""
        super().__init__()
        self._tabs = list(tabs)
        self._active = active

    def compose(self) -> ComposeResult:
        for tab_id, key, label in self._tabs:
            yield ActionChip(key, label, f"tab-{tab_id}", active=tab_id == self._active)

    def set_active(self, tab: str) -> None:
        self._active = tab
        for chip, (tab_id, _key, _label) in zip(
            self.query(ActionChip), self._tabs, strict=True
        ):
            chip.set_active(tab_id == tab)

    def on_action_chip_selected(self, event: ActionChip.Selected) -> None:
        tab = event.action.removeprefix("tab-")
        if tab != event.action:
            self.set_active(tab)
            self.post_message(TabStrip.Selected(tab))


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
        color: $primary;
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
            (binding_key(binding), binding.description)
            for binding in shown_bindings(bindings)
        ]

    def compose(self) -> ComposeResult:
        for key, description in self._pairs:
            yield Static(key, classes="ks-key", markup=False)
            yield Static(description, markup=False)

    def on_resize(self, event: Any) -> None:
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


_D = TypeVar("_D")


class Dialog(ModalScreen[_D]):
    """Centred floating dialog over a dimmed backdrop.

    Subclasses implement ``compose_dialog`` and set ``dialog_title`` /
    ``dialog_hint``. The frame is always ``MODAL_WIDTH`` — one source of
    truth for every modal.
    """

    BINDINGS = [("escape", "dismiss_dialog", "Close")]

    dialog_title: str = ""
    dialog_hint: str = "[ esc ] close"
    dialog_width: int = MODAL_WIDTH

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
        border: solid $panel;
    }
    Dialog #dialog-title {
        height: 1;
        margin: 0 0 1 0;
        color: $primary;
        text-style: bold;
        content-align-horizontal: center;
    }
    Dialog #dialog-hint {
        height: 1;
        margin: 1 0 0 0;
        color: $text-muted;
        content-align-horizontal: center;
    }
    Dialog Input, Dialog Select {
        height: 1;
        border: none;
        border-left: thick $panel;
        background: $panel;
        padding: 0 1;
        margin: 0 0 1 0;
    }
    Dialog Input:focus, Dialog Select:focus {
        border-left: thick $primary;
    }
    Dialog SelectCurrent {
        border: none;
        padding: 0;
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


class RiggerTable(DataTable[Any]):
    """DataTable with the desk defaults: zebra stripes and a row cursor."""

    def on_mount(self) -> None:
        self.zebra_stripes = True
        self.cursor_type = "row"
