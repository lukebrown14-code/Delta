"""Data and LLM cost screen."""

from textual.app import ComposeResult
from textual.containers import VerticalScroll
from textual.screen import Screen
from textual.widgets import Static

from rigger import services


class Data(Screen):
    name = "data"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield VerticalScroll(Static(id="health"), Static(id="costs"))

    def on_mount(self) -> None:
        self.refresh_view()

    def refresh_view(self) -> None:
        health = services.data_health(self.rig)
        counts = "\n".join(f"{name}: {count}" for name, count in sorted(health.counts.items()))
        latest = (
            "\n".join(f"{name}: {value}" for name, value in sorted(health.latest_bar.items()))
            or "none"
        )
        self.query_one("#health", Static).update(
            f"[bold]Data health[/bold]\n{counts}\n\nLatest bars\n{latest}\n\nFX: {health.fx_rates}"
        )
        costs = services.llm_costs(self.rig.engine)
        lines = [
            f"{row.task} / {row.model}: {row.calls} calls, ${row.cost_usd:.6f}" for row in costs
        ]
        self.query_one("#costs", Static).update(
            "[bold]LLM costs[/bold]\n" + ("\n".join(lines) or "none")
        )
