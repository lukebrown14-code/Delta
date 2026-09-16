"""Data and LLM cost screen: evidence tables in cards."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from textual.app import ComposeResult
from textual.widgets import Static

from rigger import services
from rigger.tui.shell import RiggerScreen, age_text
from rigger.tui.widgets import Card, RiggerTable


class Data(RiggerScreen):
    name = "data"

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with Card(title="Stored evidence"):
            yield RiggerTable(id="health-table")
            yield Static(id="health-latest", markup=False, classes="muted")
        with Card(title="Model spend"):
            yield RiggerTable(id="costs-table")

    def on_mount(self) -> None:
        self.query_one("#health-table", RiggerTable).add_columns("Table", "Rows")
        self.query_one("#costs-table", RiggerTable).add_columns("Task", "Model", "Calls", "$")
        self.refresh_view()

    def refresh_view(self) -> None:
        health = services.data_health(self.rig)
        health_table = self.query_one("#health-table", RiggerTable)
        health_table.clear()
        for name, count in sorted(health.counts.items()):
            health_table.add_row(name, str(count), key=name)
        latest = self.query_one("#health-latest", Static)
        if health.latest_bar:
            cells = []
            for instrument_id, ts in sorted(health.latest_bar.items()):
                if ts.tzinfo is None:
                    ts = ts.replace(tzinfo=UTC)
                label, state = age_text(datetime.now(UTC) - ts)
                cells.append(f"{instrument_id} {label} ({state})")
            latest.update("  ".join(cells))
        else:
            latest.update("no bars yet — run gather")
        costs_table = self.query_one("#costs-table", RiggerTable)
        costs_table.clear()
        for row in services.llm_costs(self.rig.engine):
            costs_table.add_row(
                row.task, row.model, str(row.calls), f"{row.cost_usd:.4f}",
                key=f"{row.task}/{row.model}",
            )
