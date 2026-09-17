"""Configuration screen: provider, model routing, plugins and targets in one column."""

from __future__ import annotations

from typing import Any

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Collapsible, Static

from rigger import services
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Pane, PaneStack, Pill, RiggerTable, StatusDot


class Config(RiggerScreen):
    name = "config"
    BINDINGS = [
        Binding("d", "configure_source", "Source"),
        Binding("a", "add_market", "Market"),
        Binding("e", "edit_market", "Edit market"),
        Binding("x", "remove_market", "Remove market"),
    ]

    CSS = """
    #cfg-scroll {
        height: 1fr;
    }
    #cfg-sections > Pane {
        height: auto;
    }
    #cfg-routing, #cfg-targets {
        height: auto;
        max-height: 12;
    }
    #health-table, #costs-table { height: auto; max-height: 12; }
    #cfg-plugins {
        height: auto;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with VerticalScroll(id="cfg-scroll"):
            with PaneStack(id="cfg-sections"):
                with Pane(title="provider", icon="", classes="-auto"):
                    with Horizontal(classes="check-row"):
                        yield Static("llm provider:", markup=False, classes="muted")
                        yield Pill("none", id="cfg-provider")
                    yield Static(
                        "press p to connect or switch providers",
                        markup=False,
                        classes="empty-hint",
                    )
                with Pane(title="model routing", icon="", classes="-auto"):
                    yield RiggerTable(id="cfg-routing")
                    yield Static(
                        "no routes set — press m to pick models",
                        id="cfg-routing-empty",
                        markup=False,
                        classes="empty-hint",
                    )
                with Pane(title="plugins", icon="", classes="-auto"):
                    yield Vertical(id="cfg-plugins")
                with Pane(title="data sources", icon="", classes="-auto"):
                    yield RiggerTable(id="cfg-sources")
                    yield Static(
                        "press d to configure the selected source",
                        id="cfg-sources-hint",
                        markup=False,
                        classes="empty-hint",
                    )
                with Pane(title="markets", icon="", classes="-auto"):
                    yield RiggerTable(id="cfg-markets")
                    yield Static(
                        "a add · e edit · x remove (user-defined markets only)",
                        markup=False,
                        classes="empty-hint",
                    )
                with Pane(title="targets", icon="", classes="-auto"):
                    yield RiggerTable(id="cfg-targets")
                    yield Static(
                        "no targets configured — press w to add one",
                        id="cfg-targets-empty",
                        markup=False,
                        classes="empty-hint",
                    )

                with Collapsible(title="Diagnostics", collapsed=True):
                    with Pane(title="stored evidence", classes="-auto"):
                        yield RiggerTable(id="health-table")
                        yield Static(id="health-latest", markup=False)
                    with Pane(title="model spend · cumulative", classes="-auto"):
                        yield RiggerTable(id="costs-table")
                        yield Static(id="costs-total", markup=False)

    async def on_mount(self) -> None:
        self.query_one("#health-table", RiggerTable).add_columns("Table", "Rows")
        self.query_one("#costs-table", RiggerTable).add_columns("Task", "Model", "Calls", "USD")
        self.query_one("#cfg-routing", RiggerTable).add_columns("Task", "Model")
        self.query_one("#cfg-targets", RiggerTable).add_columns("Name", "Kind", "Market", "Tickers")
        self.query_one("#cfg-sources", RiggerTable).add_columns("Source", "Quality", "Status")
        self.query_one("#cfg-markets", RiggerTable).add_columns("ID", "Market", "Currency", "Yahoo")
        await self.refresh_view()

    async def refresh_view(self) -> None:
        health = services.data_health(self.rig)
        table = self.query_one("#health-table", RiggerTable)
        table.clear()
        for name, count in sorted(health.counts.items()):
            table.add_row(name, str(count))
        self.query_one("#health-latest", Static).update(
            "\n".join(
                f"{name} · latest price {stamp.isoformat()}"
                for name, stamp in sorted(health.latest_bar.items())
            )
            or "No prices gathered yet"
        )
        costs = self.query_one("#costs-table", RiggerTable)
        costs.clear()
        cost_rows = services.llm_costs(self.rig.engine)
        self.query_one("#costs-total", Static).update(
            f"Total: ${sum(row.cost_usd for row in cost_rows):.4f}"
        )
        for row in cost_rows:
            costs.add_row(row.task, row.model, str(row.calls), f"${row.cost_usd:.4f}")
        await self._refresh_plugins()
        self._refresh_sources()
        self._refresh_markets()
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

    def _refresh_sources(self) -> None:
        table = self.query_one("#cfg-sources", RiggerTable)
        table.clear()
        sources = services.data_provider_status(self.rig)
        for source in sources:
            table.add_row(
                source.label,
                "primary" if source.primary_disclosure else "secondary",
                "ready" if source.configured and source.enabled else "needs setup",
                key=source.name,
            )
        self.query_one("#cfg-sources-hint", Static).display = bool(sources)

    async def action_configure_source(self) -> None:
        from rigger.tui.screens.source_setup import configure_source

        table = self.query_one("#cfg-sources", RiggerTable)
        if table.cursor_row is None or table.row_count == 0:
            self.app.notify("select a data source first", severity="warning")
            return
        row_key = table.coordinate_to_cell_key((table.cursor_row, 0)).row_key
        if row_key is None:
            return
        await configure_source(self.app, self.rig, str(row_key.value), self.refresh_view)

    def _refresh_markets(self) -> None:
        table = self.query_one("#cfg-markets", RiggerTable)
        table.clear()
        for name, profile in sorted(getattr(self.rig.cfg, "markets", {}).items()):
            table.add_row(name, profile.label, profile.currency, profile.yahoo_suffix or "—", key=name)

    def _selected_market(self) -> str | None:
        table = self.query_one("#cfg-markets", RiggerTable)
        if table.cursor_row is None or table.row_count == 0:
            return None
        return str(table.coordinate_to_cell_key((table.cursor_row, 0)).row_key.value)

    async def action_add_market(self) -> None:
        from rigger.tui.screens.market_setup import MarketSetupModal

        values = await self.app.push_screen_wait(MarketSetupModal())
        if values is None:
            return
        await self._save_market(values)

    async def action_edit_market(self) -> None:
        from rigger.tui.screens.market_setup import MarketSetupModal

        name = self._selected_market()
        if name is None:
            self.app.notify("select a market first", severity="warning")
            return
        profile = self.rig.cfg.markets[name]
        values = await self.app.push_screen_wait(
            MarketSetupModal(
                {"id": name, "label": profile.label, "currency": profile.currency, "yahoo_suffix": profile.yahoo_suffix},
                editable_id=False,
            )
        )
        if values is not None:
            await self._save_market(values)

    async def _save_market(self, values: dict[str, str]) -> None:
        try:
            services.save_market(**values)
        except ValueError as exc:
            self.app.notify(str(exc), severity="error")
            return
        reload_markets = getattr(self.rig, "reload_markets", None)
        if callable(reload_markets):
            reload_markets()
        await self.refresh_view()
        self.app.notify(f"{values['id'].lower()} market saved")

    async def action_remove_market(self) -> None:
        name = self._selected_market()
        if name is None:
            self.app.notify("select a market first", severity="warning")
            return
        try:
            services.remove_market(name)
        except ValueError as exc:
            self.app.notify(str(exc), severity="error")
            return
        reload_markets = getattr(self.rig, "reload_markets", None)
        if callable(reload_markets):
            reload_markets()
        await self.refresh_view()
        self.app.notify(f"{name} market removed")
