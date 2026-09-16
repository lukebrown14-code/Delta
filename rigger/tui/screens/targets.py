"""Watchlist with an always-open, asset-class-aware metrics inspector."""

from __future__ import annotations

import asyncio
from typing import Any

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Button, Input, Sparkline, Static

from rigger import services
from rigger.asset_metrics import AssetMetrics, chart_window, fetch_asset_metrics
from rigger.core.models import Instrument
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Pane, PaneRow, RiggerTable


class Targets(RiggerScreen):
    name = "targets"
    BINDINGS = [("space", "inspect", "Inspect metrics")]
    CSS = """
    #target-split { height: 1fr; }
    #target-list-pane { width: 2fr; }
    #target-form-pane { width: 1fr; }
    #target-inspector-pane { width: 2fr; }
    #target-table { height: 1fr; margin: 0; }
    .tg-form { height: auto; }
    .tg-form Input { width: 1fr; margin: 0; }
    .tg-buttons { height: auto; }
    #target-inspector { height: 1fr; overflow-y: auto; }
    #target-inspector-empty, #target-inspector-status { height: auto; color: $text-muted; }
    #target-inspector-title { height: auto; color: $primary; text-style: bold; }
    #target-chart { height: 5; width: 1fr; }
    #target-inspector-values { height: auto; }
    .metric-row { height: 1; }
    .metric-label { width: 1fr; color: $text-muted; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self._metrics: dict[str, AssetMetrics] = {}
        self._selected_instrument: Instrument | None = None
        self.specs: dict[str, Any] = {}

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="target-split"):
            with Pane(title="watchlist", icon="", id="target-list-pane"):
                yield RiggerTable(id="target-table")
            with Pane(title="metrics", icon="", id="target-inspector-pane"):
                with Vertical(id="target-inspector"):
                    yield Static(
                        "Select a Watchlist item", id="target-inspector-empty", markup=False
                    )
                    yield Static("", id="target-inspector-title", markup=False)
                    yield Static("", id="target-inspector-status", markup=False)
                    yield Sparkline([], id="target-chart")
                    yield Vertical(id="target-inspector-values")
                    yield Button("Refresh metrics", id="target-refresh")
        yield Input(placeholder="name", id="tg-name", classes="tg-form")
        yield Input(placeholder="kind (company|sector|…)", id="tg-kind", classes="tg-form")
        yield Input(placeholder="market (us|asx)", id="tg-market", classes="tg-form")
        yield Input(placeholder="tickers (BHP,RIO)", id="tg-tickers", classes="tg-form")
        yield Input(
            placeholder="asset class (equity|bond|commodity|…)",
            id="tg-asset-class",
            classes="tg-form",
        )
        yield Input(placeholder="tags (a,b)", id="tg-tags", classes="tg-form")
        yield Horizontal(
            Button("Add", id="tg-add", variant="primary"),
            Button("Remove", id="tg-remove", variant="error"),
            classes="tg-buttons",
        )

    def on_mount(self) -> None:
        self.query_one("#target-table", RiggerTable).add_columns(
            "Name", "Kind", "Market", "Tickers", "Tags"
        )
        self.refresh_view()

    def refresh_view(self) -> None:
        table = self.query_one("#target-table", RiggerTable)
        selected = str(table.get_row_at(table.cursor_row)[0]) if table.row_count else None
        table.clear()
        self.specs = services.target_specs()
        specs = sorted(self.specs.values(), key=lambda t: t.id)
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
        if table.row_count:
            target_id = selected if selected in self.specs else str(table.get_row_at(0)[0])
            table.move_cursor(row=[target.id for target in specs].index(target_id))
            self._select_instrument(target_id)
        else:
            self._selected_instrument = None
            self._render_inspector()

    def _select_instrument(self, target_id: str, force: bool = False) -> None:
        target = self.specs.get(target_id)
        if target is None:
            return
        instrument = next((i for i in self.rig.universe() if target_id in i.watchlists), None)
        if instrument is None and target.tickers and target.markets:
            symbol = target.tickers[0]
            instrument = Instrument(
                id=f"{target.markets[0].upper()}:{symbol}",
                market=target.markets[0],
                symbol=symbol,
                currency="",
                asset_class=target.asset_class,
            )
        self._selected_instrument = instrument
        if instrument:
            self._load_metrics(instrument, force)

    def _load_metrics(self, instrument: Instrument, force: bool = False) -> None:
        if not force and instrument.id in self._metrics:
            self._render_inspector()
            return
        self.query_one("#target-inspector-status", Static).update("Loading live metrics…")
        self.fetch_metrics(instrument)

    @work(exclusive=True, thread=False)
    async def fetch_metrics(self, instrument: Instrument) -> None:
        metrics = await asyncio.to_thread(fetch_asset_metrics, instrument)
        self._metrics[instrument.id] = metrics
        if self._selected_instrument and self._selected_instrument.id == instrument.id:
            self._render_inspector()

    def _render_inspector(self) -> None:
        instrument = self._selected_instrument
        metric = self._metrics.get(instrument.id) if instrument else None
        empty = self.query_one("#target-inspector-empty", Static)
        title = self.query_one("#target-inspector-title", Static)
        status = self.query_one("#target-inspector-status", Static)
        values = self.query_one("#target-inspector-values", Vertical)
        if not instrument:
            empty.update("Select a Watchlist item")
            title.update("")
            status.update("")
            self.query_one("#target-chart", Sparkline).data = []
            values.remove_children()
            return
        empty.update("")
        title.update(f"{instrument.symbol} · {instrument.id} · {instrument.asset_class}")
        if metric is None:
            status.update("Loading live metrics…")
            return
        status.update(metric.error or f"1 month: {metric.change_label or 'insufficient history'}")
        self.query_one("#target-chart", Sparkline).data = chart_window(metric.series)
        values.remove_children()
        values.mount(
            *(
                Horizontal(
                    Static(label, classes="metric-label"), Static(value), classes="metric-row"
                )
                for label, value in metric.values.items()
            )
        )

    def on_data_table_row_highlighted(self, event: RiggerTable.RowHighlighted) -> None:
        if event.data_table.id == "target-table":
            self._select_instrument(str(event.row_key.value))

    def action_inspect(self) -> None:
        if self._selected_instrument:
            self._load_metrics(self._selected_instrument, force=True)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "target-refresh":
            self.action_inspect()
            return
        if event.button.id == "tg-add":
            name = self.query_one("#tg-name", Input).value.strip()
            kind = self.query_one("#tg-kind", Input).value.strip() or "company"
            market = self.query_one("#tg-market", Input).value.strip()
            tickers = self.query_one("#tg-tickers", Input).value.strip()
            asset_class = self.query_one("#tg-asset-class", Input).value.strip() or "equity"
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
                    asset_class=asset_class,
                )
            except (ValueError, KeyError) as exc:
                self.notify(exc.args[0], severity="error")
                return
            self.query_one("#tg-name", Input).value = ""
            self.query_one("#tg-tickers", Input).value = ""
            self.refresh_view()
            self.notify(f"Added {name} to the watchlist")
            return
        if event.button.id == "tg-remove":
            table = self.query_one("#target-table", RiggerTable)
            if not table.row_count:
                self.notify("Select a watchlist entry first", severity="error")
                return
            name = table.get_row_at(table.cursor_row)[0]
            try:
                services.remove_target(str(name))
            except (ValueError, KeyError) as exc:
                self.notify(exc.args[0], severity="error")
                return
            self.refresh_view()
            self.notify(f"Removed {name} from the watchlist")
