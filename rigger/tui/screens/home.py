"""Home screen."""

from textual.app import ComposeResult
from textual.containers import VerticalScroll
from textual.screen import Screen
from textual.widgets import Static

from rigger import services


class Home(Screen):
    name = "home"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield VerticalScroll(
            Static(
                "[bold cyan]RIGGER[/bold cyan] — investment research assistant", classes="title"
            ),
            Static(
                "Watch what you care about → gather evidence → read sourced reports",
                classes="diagram",
            ),
            Static(id="checks"),
            Static(id="targets"),
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
        specs = services.target_specs()
        if specs:
            lines = "\n".join(
                f"{target.id}: {target.kind} {','.join(target.markets)} "
                f"({len(target.tickers)} tickers)"
                for target in sorted(specs.values(), key=lambda t: t.id)
            )
            self.query_one("#targets", Static).update("[bold]Targets[/bold]\n" + lines)
        else:
            self.query_one("#targets", Static).update(
                "[bold]Targets[/bold]\nNone configured — press w to add one."
            )
