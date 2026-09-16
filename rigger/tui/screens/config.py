"""Configuration screen: provider, model routing, plugins, targets — in cards."""

from __future__ import annotations

from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Static

from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Card, Pill, RiggerTable, StatusDot


class Config(RiggerScreen):
    name = "config"

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with Card(title="Provider"):
            with Horizontal(classes="check-row"):
                yield Static("llm provider:", markup=False, classes="muted")
                yield Pill("none", id="cfg-provider")
            yield Static(
                "press p to connect or switch providers",
                markup=False,
                classes="empty-hint",
            )
        with Card(title="Model routing"):
            yield RiggerTable(id="cfg-routing")
            yield Static("no routes set — press m to pick models", id="cfg-routing-empty", markup=False, classes="empty-hint")
        with Card(title="Plugins"):
            yield Vertical(id="cfg-plugins")
        with Card(title="Targets"):
            yield RiggerTable(id="cfg-targets")
            yield Static(
                "no targets configured — press w to add one",
                id="cfg-targets-empty",
                markup=False,
                classes="empty-hint",
            )

    async def on_mount(self) -> None:
        self.query_one("#cfg-routing", RiggerTable).add_columns("Task", "Model")
        self.query_one("#cfg-targets", RiggerTable).add_columns("Name", "Kind", "Market", "Tickers")
        await self.refresh_view()

    async def refresh_view(self) -> None:
        await self._refresh_plugins()
        cfg = self.rig.cfg
        self.query_one("#cfg-provider", Pill).update(str(getattr(cfg, "llm_provider", "") or "—"))
        routing = self.query_one("#cfg-routing", RiggerTable)
        routing.clear()
        routes = dict(getattr(cfg, "llm_routing", {}) or {})
        for task, model in sorted(routes.items()):
            routing.add_row(task, model, key=task)
        self.query_one("#cfg-routing-empty", Static).display = not routes
        targets_table = self.query_one("#cfg-targets", RiggerTable)
        targets_table.clear()
        configured = {
            name: spec
            for name, spec in getattr(cfg, "targets", {}).items()
            if not spec.get("legacy", False)
        }
        for name, spec in sorted(configured.items()):
            targets_table.add_row(
                name,
                str(spec.get("kind", "")),
                str(spec.get("market", "")),
                ",".join(spec.get("tickers", [])) or "—",
                key=name,
            )
        legacy = len(getattr(cfg, "targets", {})) - len(configured)
        empty = self.query_one("#cfg-targets-empty", Static)
        empty.display = not configured
        if legacy:
            empty.update(f"{legacy} legacy universe entries are active")
            empty.display = True

    async def _refresh_plugins(self) -> None:
        holder = self.query_one("#cfg-plugins", Vertical)
        await holder.remove_children()
        for name, plugin in sorted(self.rig.plugins.items()):
            enabled = bool(getattr(plugin, "enabled", False))
            await holder.mount(
                Horizontal(
                    StatusDot("ok" if enabled else "error"),
                    Static(name, markup=False),
                    classes="check-row",
                )
            )
