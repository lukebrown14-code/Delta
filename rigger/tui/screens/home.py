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
            Static(id="watchlists"),
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
        specs = services.watchlist_specs()
        if specs:
            lines = "\n".join(
                f"{name}: {spec.get('market', '')} ({len(spec.get('tickers', []))} holdings)"
                for name, spec in sorted(specs.items())
            )
            self.query_one("#watchlists", Static).update("[bold]Watchlists[/bold]\n" + lines)
        else:
            self.query_one("#watchlists", Static).update(
                "[bold]Watchlists[/bold]\nNone configured — press w to add one."
            )
