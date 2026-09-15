"""Home screen."""

from textual.app import ComposeResult
from textual.widgets import Static

from rigger import services
from rigger.tui.shell import RiggerScreen


class Home(RiggerScreen):
    name = "home"

    def __init__(self, rig) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        yield Static(
            "Watch what you care about → gather evidence → read sourced reports",
            classes="diagram",
        )
        yield Static(id="checks")
        yield Static(id="targets")

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
