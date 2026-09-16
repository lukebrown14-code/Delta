"""Evidence screen: stored-evidence health beside cumulative model spend."""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from textual.app import ComposeResult
from textual.widgets import Static

from rigger import services
from rigger.tui.shell import RiggerScreen, age_text
from rigger.tui.widgets import Pane, PaneRow, RiggerTable


class Data(RiggerScreen):
    name = "data"

    CSS = """
    #data-split {
        height: 1fr;
    }
    #data-split > Pane {
        width: 1fr;
    }
    #health-table, #costs-table {
        height: auto;
    }
    #health-latest {
        height: auto;
        margin-top: 1;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="data-split"):
            with Pane(title="stored evidence", icon="", id="data-health-pane"):
                yield RiggerTable(id="health-table")
                yield Static(id="health-latest", markup=False, classes="muted")
            with Pane(title="model spend", icon="", id="data-costs-pane"):
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
        self.query_one("#data-health-pane", Pane).set_badge(
            f"{sum(health.counts.values())} rows"
        )
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
        cost_rows = services.llm_costs(self.rig.engine)
        self.query_one("#data-costs-pane", Pane).set_badge(
            f"${sum(row.cost_usd for row in cost_rows):.2f}"
        )
        for row in cost_rows:
            costs_table.add_row(
                row.task, row.model, str(row.calls), f"{row.cost_usd:.4f}",
                key=f"{row.task}/{row.model}",
            )
