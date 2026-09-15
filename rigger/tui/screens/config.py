"""Configuration screen."""

from textual.app import ComposeResult
from textual.containers import VerticalScroll
from textual.screen import Screen
from textual.widgets import Static


class Config(Screen):
    name = "config"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield VerticalScroll(Static(id="config"))

    def on_mount(self) -> None:
        self.refresh_view()

    def refresh_view(self) -> None:
        cfg = self.rig.cfg
        plugins = "\n".join(
            f"{name}: {'enabled' if plugin.enabled else 'disabled'}"
            for name, plugin in sorted(self.rig.plugins.items())
        )
        universe = "\n".join(
            f"{market}: {', '.join(symbols)}" for market, symbols in cfg.universe.items()
        )
        routing = "\n".join(f"{task}: {model}" for task, model in cfg.llm_routing.items())
        self.query_one("#config", Static).update(
            f"[bold]Provider[/bold] {cfg.llm_provider}\n\n"
            f"[bold]Universe[/bold]\n{universe}\n\n"
            f"[bold]Model routing[/bold]\n{routing}\n\n"
            f"[bold]Risk limits[/bold]\n{cfg.risk}\n\n"
            f"[bold]Plugins[/bold]\n{plugins}"
        )
