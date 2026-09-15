"""Signals screen."""

from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.screen import Screen
from textual.widgets import DataTable, Static

from rigger import services


class Signals(Screen):
    name = "signals"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig
        self.rows = []

    def compose(self) -> ComposeResult:
        yield Horizontal(
            DataTable(id="signals-table"),
            VerticalScroll(Static("Select a signal", id="signal-detail")),
        )

    def on_mount(self) -> None:
        table = self.query_one("#signals-table", DataTable)
        table.add_columns(
            "Date", "Instrument", "Strategy", "Direction", "Conviction", "Model", "Cost"
        )
        self.refresh_view()

    def refresh_view(self) -> None:
        self.rows = services.signals(self.rig.engine)
        table = self.query_one("#signals-table", DataTable)
        table.clear()
        for signal in self.rows:
            table.add_row(
                signal.ts.strftime("%Y-%m-%d"),
                signal.instrument_id,
                signal.strategy,
                signal.direction,
                f"{signal.conviction:.2f}",
                signal.model or "n/a",
                f"{signal.cost_usd or 0:.4f}",
                key=signal.id,
            )

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        signal = services.signal_by_id(self.rig.engine, str(event.row_key.value))
        if signal is None:
            return
        evidence = services.resolve_evidence(self.rig.engine, signal.evidence_ids)
        details = (
            f"[bold]{signal.instrument_id} — {signal.direction.upper()}[/bold]\n\n"
            f"[bold]Thesis[/bold]\n{signal.thesis}\n\n"
            f"[bold]Invalidation[/bold]\n{signal.invalidation}\n\n"
            f"Horizon: {signal.horizon_days} days\n"
            f"Evidence: bars {evidence.bars.count}, news {len(evidence.news)}, "
            f"events {len(evidence.events)}, fundamentals {len(evidence.fundamentals)}"
        )
        critic = signal.metadata.get("critic")
        if critic:
            details += f"\n\n[bold]Critic[/bold]\nVerdict: {critic.get('verdict')}\n{critic.get('counter_argument', '')}"
        ensemble = signal.metadata.get("ensemble")
        if ensemble:
            details += f"\n\n[bold]Ensemble[/bold]\nDispersion: {ensemble.get('dispersion', 0):.3f}"
        self.query_one("#signal-detail", Static).update(details)

    def key_b(self) -> None:
        row = self.query_one("#signals-table", DataTable).cursor_row
        if 0 <= row < len(self.rows):
            brief = services.brief_for(self.rig, self.rows[row].instrument_id)
            self.app.push_screen(BriefModal(brief or "No brief available"))


class BriefModal(Screen):
    def __init__(self, content: str) -> None:
        super().__init__()
        self.content = content

    def compose(self) -> ComposeResult:
        yield VerticalScroll(Static(self.content), classes="modal")
