"""Persistent shell: top status bar, left nav rail, shared screen base.

All shell styling lives in ``DEFAULT_CSS`` on these classes so the screens
that double as standalone panels (reports, theses, chat) keep the shell when
mounted under any App, and inherit theme tokens when a Rigger theme is active.
"""

from __future__ import annotations

import inspect
from datetime import UTC, datetime, timedelta
from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.message import Message
from textual.screen import Screen
from textual.widgets import Footer, Static

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


class TopBar(Horizontal):
    """Brand plus live status cells: data freshness, model, spend."""

    DEFAULT_CSS = """
    TopBar {
        height: 3;
        background: $panel;
        border-top: outer $primary;
        align: center middle;
    }
    TopBar #tb-brand {
        width: auto;
        height: 1;
        padding: 0 2;
        color: $primary;
        text-style: bold;
    }
    TopBar #tb-stats {
        align-horizontal: right;
        height: 1;
    }
    TopBar .tb-cell {
        height: 1;
        padding: 0 2;
        border-left: inner $panel;
        color: $text-muted;
    }
    TopBar .tb-cell .tb-value { color: $foreground; width: auto; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__()
        self.rig = rig
        self._dot = StatusDot("warn", id="tb-dot")
        self._freshness = Static("data —", markup=False, classes="tb-value")
        self._model = Static("", markup=False, classes="tb-value")
        self._spend = Static("", markup=False, classes="tb-value")

    def compose(self) -> ComposeResult:
        yield Static("RIGGER", id="tb-brand", markup=False)
        with Horizontal(id="tb-stats"):
            with Horizontal(classes="tb-cell"):
                yield self._dot
                yield self._freshness
            with Horizontal(classes="tb-cell"):
                yield Static("model", markup=False)
                yield self._model
            with Horizontal(classes="tb-cell"):
                yield Static("spend", markup=False)
                yield self._spend

    def on_mount(self) -> None:
        self._refresh()
        self.set_interval(30, self._refresh)

    def _refresh(self) -> None:
        """Update status cells; the bar must never take a screen down."""
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


class NavItem(Horizontal):
    """One clickable rail entry: key hint plus label."""

    class Selected(Message):
        """Posted when a nav item is clicked."""

        def __init__(self, screen_name: str) -> None:
            super().__init__()
            self.screen_name = screen_name

    DEFAULT_CSS = """
    NavItem {
        height: 3;
        padding: 0 1;
        align-vertical: middle;
        background: transparent;
    }
    NavItem .rail-key {
        width: 3;
        height: 1;
        color: $text-muted;
        text-style: bold;
    }
    NavItem .rail-label {
        height: 1;
        padding-left: 1;
        color: $foreground;
    }
    NavItem.-active {
        background: $surface;
        border-left: thick $primary;
    }
    NavItem.-active .rail-key { color: $primary; }
    NavItem:hover { background: $surface; }
    """

    def __init__(self, key: str, screen_name: str, label: str) -> None:
        super().__init__(id=f"nav-{screen_name}")
        self.screen_name = screen_name
        self._key = Static(key, classes="rail-key", markup=False)
        self._label = Static(label.upper(), classes="rail-label", markup=False)

    def compose(self) -> ComposeResult:
        yield self._key
        yield self._label

    def on_click(self) -> None:
        self.post_message(NavItem.Selected(self.screen_name))


class NavRail(Vertical):
    """Persistent left navigation column, one NavItem per screen."""

    DEFAULT_CSS = """
    NavRail {
        width: 18;
        background: $panel;
        padding: 1 0;
    }
    NavRail.-collapsed {
        width: 6;
    }
    NavRail.-collapsed .rail-label {
        display: none;
    }
    NavRail.-collapsed .rail-key {
        width: 4;
        color: $primary;
        text-style: bold;
    }
    """

    def __init__(self) -> None:
        super().__init__()
        self._items = {name: NavItem(key, name, label) for key, name, label in NAV_ITEMS}

    def compose(self) -> ComposeResult:
        yield from self._items.values()

    def on_mount(self) -> None:
        self.set_active(self.screen.name or "")
        self.set_class(bool(getattr(self.app, "narrow", False)), "-collapsed")

    def set_active(self, screen_name: str) -> None:
        for name, item in self._items.items():
            item.set_class(name == screen_name, "-active")


class RiggerScreen(Screen):
    """Base screen: TopBar, NavRail + content, Footer.

    Screens implement ``compose_content`` and keep their ``name`` class
    attribute. ``on_screen_resume`` re-runs ``refresh_view`` when defined,
    fixing data frozen at launch (screens are constructed eagerly).
    """

    DEFAULT_CSS = """
    RiggerScreen {
        layout: vertical;
    }
    RiggerScreen #screen-body {
        height: 1fr;
    }
    RiggerScreen #screen-content {
        padding: 0 1;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield TopBar(self.rig)
        with Horizontal(id="screen-body"):
            yield NavRail()
            with VerticalScroll(id="screen-content"):
                yield from self.compose_content()
        yield Footer()

    def compose_content(self) -> ComposeResult:
        raise NotImplementedError
        yield  # pragma: no cover

    async def on_screen_resume(self) -> None:
        self.query_one(NavRail).set_active(self.name or "")
        refresh = getattr(self, "refresh_view", None)
        if callable(refresh):
            result = refresh()
            if inspect.isawaitable(result):
                await result

    def on_nav_item_selected(self, event: NavItem.Selected) -> None:
        switch = getattr(self.app, "action_switch_screen", None)
        if callable(switch):
            switch(event.screen_name)
