"""Portfolio screen."""

from textual.app import ComposeResult
from textual.containers import VerticalScroll
from textual.screen import ModalScreen, Screen
from textual.widgets import Button, Checkbox, DataTable, Static

from rigger import services


class Portfolio(Screen):
    name = "portfolio"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield VerticalScroll(
            Static(id="portfolio-summary"),
            DataTable(id="positions-table"),
            Static("[bold]Recent fills[/bold]", id="fills-title"),
            DataTable(id="fills-table"),
            Button("Reset paper book", id="reset", variant="warning"),
        )

    def on_mount(self) -> None:
        self.query_one("#positions-table", DataTable).add_columns(
            "Instrument", "Qty", "Avg Price", "Currency", "Value"
        )
        self.query_one("#fills-table", DataTable).add_columns(
            "Order", "Qty", "Price", "Fee", "Slippage"
        )
        self.refresh_view()

    def refresh_view(self) -> None:
        summary = services.portfolio_summary(self.rig)
        self.query_one("#portfolio-summary", Static).update(
            f"Cash: {summary.cash:,.2f} {summary.base_currency}   "
            f"Equity: {summary.equity:,.2f} {summary.base_currency}"
        )
        positions = self.query_one("#positions-table", DataTable)
        positions.clear()
        for position in summary.positions:
            positions.add_row(
                position.instrument_id,
                f"{position.qty:.4f}",
                f"{position.avg_price:.2f}",
                position.currency,
                f"{position.value:,.2f}",
            )
        fills = self.query_one("#fills-table", DataTable)
        fills.clear()
        for fill in summary.fills:
            fills.add_row(
                fill.order_id,
                f"{fill.qty:.4f}",
                f"{fill.price:.2f}",
                f"{fill.fee:.2f}",
                f"{fill.slippage:.4f}",
            )

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "reset":
            self.app.push_screen(ResetModal(self.rig))


class ResetModal(ModalScreen):
    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield Static("Reset the paper book?")
        yield Checkbox("Also delete signals", id="also-signals")
        yield Button("Confirm", id="confirm", variant="error")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "confirm":
            delete_signals = self.query_one("#also-signals", Checkbox).value
            services.reset_paper(self.rig, signals=delete_signals)
            self.app.pop_screen()
            self.app.query_one(Portfolio).refresh_view()
