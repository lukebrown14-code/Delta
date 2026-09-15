"""Home screen."""

from textual.app import ComposeResult
from textual.containers import Container, VerticalScroll
from textual.screen import Screen
from textual.widgets import Button, Static

from rigger import services


class Home(Screen):
    name = "home"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield VerticalScroll(
            Static("[bold cyan]RIGGER[/bold cyan] — research and paper trading", classes="title"),
            Static("Ingest → Extract → Analyse → Execute → Report", classes="diagram"),
            Container(Button("Run the daily pipeline", id="run-pipeline")),
            Static(id="checks"),
            Static(id="summary"),
        )

    def on_mount(self) -> None:
        self.refresh_view()

    def refresh_view(self) -> None:
        checks = services.setup_checks(self.rig)
        markup = "\n".join(
            f"{'[green]✓[/green]' if check.ok else '[red]✗[/red]'} {check.name}"
            + (f" — {check.fix}" if not check.ok else "")
            for check in checks
        )
        self.query_one("#checks", Static).update("[bold]Setup checks[/bold]\n" + markup)
        health = services.data_health(self.rig)
        summary = "First run: add a key to .env, then use Pipeline → Ingest → Analyse."
        if health.latest_bar:
            summary = (
                f"Stored signals: {health.counts['signal']} | latest bars: {len(health.latest_bar)}"
            )
        self.query_one("#summary", Static).update(summary)

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "run-pipeline":
            self.app.action_switch_screen("pipeline")
