"""Watch target management panel."""

from __future__ import annotations

from typing import Any

from rich.text import Text
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Button, Input

from rigger import services
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Pane, PaneRow, RiggerTable


class Targets(RiggerScreen):
    name = "targets"

    CSS = """
    #target-split {
        height: 1fr;
    }
    #target-list-pane {
        width: 1fr;
    }
    #target-form-pane {
        width: 32;
    }
    #target-table {
        height: 1fr;
        margin: 0;
    }
    .tg-form {
        height: auto;
    }
    .tg-form Input {
        width: 1fr;
        margin: 0;
    }
    .tg-buttons {
        height: auto;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="target-split"):
            with Pane(title="targets", icon="", id="target-list-pane"):
                yield RiggerTable(id="target-table")
            with Pane(title="add a target", icon="", id="target-form-pane"):
                yield Vertical(
                    Input(placeholder="name", id="tg-name"),
                    Input(placeholder="kind (company|sector|…)", id="tg-kind"),
                    Input(placeholder="market (us|asx)", id="tg-market"),
                    Input(placeholder="tickers (BHP,RIO)", id="tg-tickers"),
                    Input(placeholder="tags (a,b)", id="tg-tags"),
                    Horizontal(
                        Button("Add", id="tg-add", variant="primary"),
                        Button("Remove", id="tg-remove", variant="error"),
                        classes="tg-buttons",
                    ),
                    classes="tg-form",
                )

    def on_mount(self) -> None:
        self.query_one("#target-table", RiggerTable).add_columns(
            "Name", "Kind", "Market", "Tickers", "Tags"
        )
        self.refresh_view()

    def refresh_view(self) -> None:
        table = self.query_one("#target-table", RiggerTable)
        table.clear()
        specs = sorted(services.target_specs().values(), key=lambda t: t.id)
        self.query_one("#target-list-pane", Pane).set_badge(str(len(specs)))
        for target in specs:
            table.add_row(
                target.id,
                Text(target.kind, style="cyan"),
                ",".join(target.markets),
                ",".join(target.tickers) or "—",
                ",".join(sorted(target.tags)),
                key=target.id,
            )

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "tg-add":
            name = self.query_one("#tg-name", Input).value.strip()
            kind = self.query_one("#tg-kind", Input).value.strip() or "company"
            market = self.query_one("#tg-market", Input).value.strip()
            tickers = self.query_one("#tg-tickers", Input).value.strip()
            tags = self.query_one("#tg-tags", Input).value.strip()
            if not name or not market:
                self.notify("name and market are required", severity="error")
                return
            try:
                services.add_target(
                    name,
                    kind=kind,
                    market=market,
                    tickers=[t.strip() for t in tickers.split(",") if t.strip()],
                    tags=[t.strip() for t in tags.split(",") if t.strip()],
                )
            except (ValueError, KeyError) as exc:
                self.notify(exc.args[0], severity="error")
                return
            self.query_one("#tg-name", Input).value = ""
            self.query_one("#tg-tickers", Input).value = ""
            self.refresh_view()
            self.notify(f"Added target {name}")
        elif event.button.id == "tg-remove":
            table = self.query_one("#target-table", RiggerTable)
            # An empty DataTable still reports cursor_row == 0, so row_count is
            # the only reliable "nothing to select" test.
            if table.row_count == 0:
                self.notify("Select a target first", severity="error")
                return
            name = table.get_row_at(table.cursor_row)[0]
            try:
                services.remove_target(str(name))
            except (ValueError, KeyError) as exc:
                self.notify(exc.args[0], severity="error")
                return
            self.refresh_view()
            self.notify(f"Removed target {name}")
