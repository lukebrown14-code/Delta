"""Design-system widgets: cards, stat tiles, status dots, pills, key hints.

These replace the old inline-markup ``Static`` dumps. Colour always comes
from theme-token CSS classes (see ``rigger.tcss``), never hex literals.
"""

from __future__ import annotations

from textual.app import ComposeResult
from textual.containers import Vertical
from textual.widgets import DataTable, Digits, Static

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


def _digitize(value: str) -> str:
    """Reduce a value to the charset Digits can render (digits and dots)."""
    text = "".join(ch for ch in str(value) if ch.isdigit() or ch == ".")
    return text or "0"


class Card(Vertical):
    """Bordered panel with a title. The workhorse container for screens."""

    def __init__(
        self,
        *children,
        title: str = "",
        id: str | None = None,
        classes: str = "",
        highlight: bool = False,
    ) -> None:
        super().__init__(*children, id=id, classes=classes)
        self.border_title = title
        if highlight:
            self.add_class("-highlight")


class StatTile(Vertical):
    """Big number plus muted label, for dashboard top rows."""

    def __init__(self, label: str, value: str = "0", id: str | None = None) -> None:
        super().__init__(id=id)
        self._digits = Digits(_digitize(value))
        self._label = Static(label, classes="stat-label")

    def compose(self) -> ComposeResult:
        yield self._digits
        yield self._label

    def update(self, value: str) -> None:
        self._digits.update(_digitize(value))


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


class RiggerTable(DataTable):
    """DataTable with the desk defaults: zebra stripes and a row cursor."""

    def on_mount(self) -> None:
        self.zebra_stripes = True
        self.cursor_type = "row"
