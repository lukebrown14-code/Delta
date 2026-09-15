"""Watch target management panel."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.screen import Screen
from textual.widgets import Button, DataTable, Input, Static

from rigger import services


class Targets(Screen):
    name = "targets"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield VerticalScroll(
            Static("[bold]Targets[/bold]", classes="title"),
            DataTable(id="target-table"),
            Horizontal(
                Input(placeholder="name", id="tg-name"),
                Input(placeholder="kind (company|sector|industry|market|theme)", id="tg-kind"),
                Input(placeholder="market (us|asx)", id="tg-market"),
                Input(placeholder="tickers (BHP,RIO)", id="tg-tickers"),
                Input(placeholder="tags (a,b)", id="tg-tags"),
                Button("Add", id="tg-add"),
                Button("Remove", id="tg-remove"),
            ),
        )

    def on_mount(self) -> None:
        self.query_one("#target-table", DataTable).add_columns(
            "Name", "Kind", "Market", "Tickers", "Tags"
        )
        self.refresh_view()

    def refresh_view(self) -> None:
        table = self.query_one("#target-table", DataTable)
        table.clear()
        for target in sorted(services.target_specs().values(), key=lambda t: t.id):
            table.add_row(
                target.id,
                target.kind,
                ",".join(target.markets),
                ",".join(target.tickers),
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
            row = self.query_one("#target-table", DataTable).cursor_row
            if row < 0:
                self.notify("Select a target first", severity="error")
                return
            name = self.query_one("#target-table", DataTable).get_row_at(row)[0]
            services.remove_target(str(name))
            self.refresh_view()
            self.notify(f"Removed target {name}")
