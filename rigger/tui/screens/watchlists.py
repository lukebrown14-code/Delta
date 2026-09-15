"""Watchlist management panel."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.screen import Screen
from textual.widgets import Button, DataTable, Input, Static

from rigger import services


class Watchlists(Screen):
    name = "watchlists"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield VerticalScroll(
            Static("[bold]Watchlists[/bold]", classes="title"),
            DataTable(id="watchlist-table"),
            Horizontal(
                Input(placeholder="name", id="wl-name"),
                Input(placeholder="market (us|asx)", id="wl-market"),
                Input(placeholder="tickers (BHP,RIO)", id="wl-tickers"),
                Button("Add", id="wl-add"),
                Button("Remove", id="wl-remove"),
            ),
        )

    def on_mount(self) -> None:
        self.query_one("#watchlist-table", DataTable).add_columns(
            "Name", "Market", "Holdings", "Max"
        )
        self.refresh_view()

    def refresh_view(self) -> None:
        table = self.query_one("#watchlist-table", DataTable)
        table.clear()
        for name, spec in sorted(services.watchlist_specs().items()):
            if spec.get("kind", "tickers") != "tickers":
                continue
            max_pct = spec.get("max_pct")
            table.add_row(
                name,
                spec.get("market", ""),
                str(len(spec.get("tickers", []))),
                f"{max_pct:.0f}%" if max_pct is not None else "",
                key=name,
            )

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "wl-add":
            name = self.query_one("#wl-name", Input).value.strip()
            market = self.query_one("#wl-market", Input).value.strip()
            tickers = self.query_one("#wl-tickers", Input).value.strip()
            if not name or not market or not tickers:
                self.notify("name, market and tickers are all required", severity="error")
                return
            try:
                services.add_watchlist(
                    name,
                    market=market,
                    tickers=[t.strip() for t in tickers.split(",") if t.strip()],
                )
            except (ValueError, KeyError) as exc:
                self.notify(exc.args[0], severity="error")
                return
            self.query_one("#wl-name", Input).value = ""
            self.query_one("#wl-tickers", Input).value = ""
            self.refresh_view()
            self.notify(f"Added watchlist {name}")
        elif event.button.id == "wl-remove":
            row = self.query_one("#watchlist-table", DataTable).cursor_row
            if row < 0:
                self.notify("Select a watchlist first", severity="error")
                return
            name = self.query_one("#watchlist-table", DataTable).get_row(row)[0]
            services.remove_watchlist(str(name))
            self.refresh_view()
            self.notify(f"Removed watchlist {name}")
