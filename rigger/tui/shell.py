"""Persistent shell: the footer nav strip, statusline and shared screen base.

There is no nav rail and no top bar. Navigation is keyboard-driven (number
keys, ``g``, the command palette) and the chrome is two docked rows: a
footer listing every panel with its hotkey, the current one highlighted,
and a statusline. Shell styling lives in ``DEFAULT_CSS`` so the screens
that double as standalone panels (reports, theses, chat) keep the shell
when mounted under any App, and inherit theme tokens when a Rigger theme
is active.
"""

from __future__ import annotations

import inspect
from datetime import UTC, datetime, timedelta
from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.message import Message
from textual.screen import Screen
from textual.widgets import Static

from rigger import services
from rigger.tui.widgets import StatusDot

NAV_ITEMS: list[tuple[str, str, str]] = [
    ("1", "home", "Watch"),
    ("2", "data", "Evidence"),
    ("3", "config", "Config"),
    ("4", "reports", "Reports"),
    ("5", "theses", "Theses"),
    ("6", "chat", "Ask"),
    ("w", "targets", "Targets"),
    ("c", "console", "Console"),
]


def age_text(age: timedelta) -> tuple[str, str]:
    """Humanise a data age into (label, dot-state)."""
    seconds = age.total_seconds()
    if seconds < 300:
        return "live", "ok"
    if seconds < 3600:
        return f"{int(seconds // 60)}m", "ok"
    if seconds < 86400:
        return f"{int(seconds // 3600)}h", "warn" if seconds > 86400 / 2 else "ok"
    if seconds < 7 * 86400:
        return f"{int(seconds // 86400)}d", "warn"
    return f"{int(seconds // 86400)}d", "error"


class NavKey(Static):
    """One footer entry: hotkey plus panel name, clickable."""

    class Selected(Message):
        """Posted when a footer entry is clicked."""

        def __init__(self, screen_name: str) -> None:
            super().__init__()
            self.screen_name = screen_name

    DEFAULT_CSS = """
    NavKey {
        width: auto;
        height: 1;
        padding: 0 1;
        color: $text-muted;
    }
    NavKey.-active {
        background: $primary;
        color: $background;
        text-style: bold;
    }
    NavKey:hover {
        background: $surface;
    }
    """

    def __init__(self, key: str, screen_name: str, label: str) -> None:
        super().__init__(id=f"nav-{screen_name}")
        self.screen_name = screen_name
        self._key = key
        self._label = label
        self.render_label(compact=False, active=False)

    def render_label(self, *, compact: bool, active: bool) -> None:
        """Show the label unless the strip is too narrow to fit every name.

        The active entry always keeps its label: it is the only thing naming
        the panel you are on.
        """
        show_label = active or not compact
        self.update(f"[b]{self._key}[/b] {self._label}" if show_label else f"[b]{self._key}[/b]")

    def on_click(self) -> None:
        self.post_message(NavKey.Selected(self.screen_name))


class NavStrip(Horizontal):
    """Footer listing every panel and its hotkey, current one highlighted."""

    #: The eight labelled entries need 81 columns. Below that every entry
    #: but the active one drops to its hotkey alone.
    COMPACT_WIDTH = 81

    DEFAULT_CSS = """
    NavStrip {
        height: 1;
        background: $panel;
    }
    """

    def __init__(self, active: str = "") -> None:
        super().__init__()
        self._items = {name: NavKey(key, name, label) for key, name, label in NAV_ITEMS}
        self._active = active
        self._compact = False

    def compose(self) -> ComposeResult:
        yield from self._items.values()

    def on_mount(self) -> None:
        self.set_active(self._active)

    def on_resize(self, event: Any) -> None:
        self._compact = event.size.width < self.COMPACT_WIDTH
        self._render_items()

    def set_active(self, screen_name: str) -> None:
        self._active = screen_name
        self._render_items()

    def _render_items(self) -> None:
        for name, item in self._items.items():
            active = name == self._active
            item.set_class(active, "-active")
            item.render_label(compact=self._compact, active=active)


class StatusLine(Horizontal):
    """One-row bottom chrome: screen context and live status cells.

    The panel name is not repeated here — the nav strip above highlights it.
    """

    DEFAULT_CSS = """
    StatusLine {
        height: 1;
        background: $panel;
    }
    StatusLine #sl-context {
        width: 1fr;
        padding: 0 1;
        color: $text-muted;
    }
    StatusLine .sl-value {
        width: auto;
        padding: 0 1;
        color: $foreground;
    }
    StatusLine #sl-dot {
        width: 2;
        padding: 0 0 0 1;
    }
    StatusLine #sl-keys {
        width: auto;
        padding: 0 1;
        color: $text-muted;
    }
    """

    def __init__(self, rig: Any, context: str = "") -> None:
        super().__init__()
        self.rig = rig
        # NB: not ``self._context`` — that name shadows a Textual MessagePump
        # attribute and silently deadlocks the widget's message loop.
        self._ctx = Static(context, id="sl-context", markup=False)
        self._dot = StatusDot("warn", id="sl-dot")
        self._freshness = Static("data —", markup=False, classes="sl-value")
        self._model = Static("", markup=False, classes="sl-value")
        self._spend = Static("", markup=False, classes="sl-value")

    def compose(self) -> ComposeResult:
        # Flat children only: auto-width Horizontals nested inside a docked
        # auto row deadlock Textual's layout pass.
        yield self._ctx
        yield self._dot
        yield self._freshness
        yield self._model
        yield self._spend
        yield Static("? help · ^p go", id="sl-keys", markup=False)

    def on_mount(self) -> None:
        self._refresh()
        self.set_interval(30, self._refresh)

    def set_context(self, text: str) -> None:
        self._ctx.update(text)

    def _refresh(self) -> None:
        """Update status cells; the statusline must never take a screen down."""
        try:
            health = services.data_health(self.rig)
            if health.latest_bar:
                newest = max(health.latest_bar.values())
                if newest.tzinfo is None:
                    newest = newest.replace(tzinfo=UTC)
                label, state = age_text(datetime.now(UTC) - newest)
                self._freshness.update(f"data {label}")
                self._dot.set_state(state)
            else:
                self._freshness.update("data none")
                self._dot.set_state("error")
            self._model.update(str(getattr(self.rig.cfg, "llm_provider", "") or "—"))
            total = sum(row.cost_usd for row in services.llm_costs(self.rig.engine))
            self._spend.update(f"${total:.2f}")
        except Exception:
            self._freshness.update("data ?")
            self._dot.set_state("warn")


class ScreenFooter(Vertical):
    """The two docked chrome rows: nav strip above, statusline below.

    They share one docked container because two widgets docked to the same
    edge independently resolve to the same row rather than stacking.
    """

    DEFAULT_CSS = """
    ScreenFooter {
        dock: bottom;
        height: 2;
    }
    """

    def __init__(self, rig: Any, *, active: str = "") -> None:
        super().__init__()
        self.rig = rig
        self._active = active

    def compose(self) -> ComposeResult:
        yield NavStrip(active=self._active)
        yield StatusLine(self.rig)


class RiggerScreen(Screen):
    """Base screen: pane content above a docked nav footer and statusline.

    Screens implement ``compose_content`` and keep their ``name`` class
    attribute. Content is *not* wrapped in a scroller — a screen that needs
    scrolling puts a ``VerticalScroll`` inside its own ``Pane``, so there is
    never more than one scroll region. ``on_screen_resume`` re-runs
    ``refresh_view`` when defined, fixing data frozen at launch (screens are
    constructed eagerly).
    """

    DEFAULT_CSS = """
    RiggerScreen {
        layout: vertical;
        padding: 0 1;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield from self.compose_content()
        yield ScreenFooter(self.rig, active=self.name or "")

    def compose_content(self) -> ComposeResult:
        raise NotImplementedError
        yield  # pragma: no cover

    def set_context(self, text: str) -> None:
        """Update the statusline's middle cell, if it is mounted."""
        try:
            self.query_one(StatusLine).set_context(text)
        except Exception:
            pass

    def on_nav_key_selected(self, event: NavKey.Selected) -> None:
        switch = getattr(self.app, "action_switch_screen", None)
        if callable(switch):
            switch(event.screen_name)

    async def on_screen_resume(self) -> None:
        self.query_one(NavStrip).set_active(self.name or "")
        refresh = getattr(self, "refresh_view", None)
        if callable(refresh):
            result = refresh()
            if inspect.isawaitable(result):
                await result
