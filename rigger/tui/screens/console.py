"""In-app command console: run the CLI's watchlist/paper commands without leaving the TUI."""

from __future__ import annotations

import shlex

from textual.app import ComposeResult
from textual.screen import Screen
from textual.widgets import Input, RichLog, Static

from rigger import services


class Console(Screen):
    name = "console"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield Static("[bold]Command console[/bold] — type [bold]help[/bold] for commands")
        yield Input(
            placeholder="watchlist add mining --market asx --tickers BHP,RIO", id="console-input"
        )
        yield RichLog(id="console-log")

    def on_mount(self) -> None:
        self.query_one("#console-log", RichLog).write("Ready. Enter a command.")

    def on_input_submitted(self, event: Input.Submitted) -> None:
        text = event.value.strip()
        event.input.value = ""
        if not text:
            return
        self._run(text)

    def _run(self, text: str) -> None:
        log = self.query_one("#console-log", RichLog)
        try:
            argv = shlex.split(text)
        except ValueError as exc:
            log.write(f"[red]{exc}[/red]")
            return
        if not argv:
            return
        try:
            log.write(self._dispatch(argv))
        except (ValueError, KeyError) as exc:
            log.write(f"[red]{exc.args[0]}[/red]")

    def _dispatch(self, argv: list[str]) -> str:
        cmd = argv[0]
        args = argv[1:]

        if cmd in ("help", "?"):
            return (
                "[bold]Commands[/bold]\n"
                "  watchlist add NAME --market M --tickers A,B --max-pct N\n"
                "  watchlist remove NAME\n"
                "  watchlist list\n"
                "  watchlist show NAME\n"
                "  config show"
            )

        if cmd == "watchlist":
            return self._watchlist(args)

        if cmd == "config" and args and args[0] == "show":
            from rigger.core import config as config_mod

            return str(config_mod.load_toml())

        raise ValueError(f"unknown command: {cmd!r}; try 'help'")

    def _watchlist(self, argv: list[str]) -> str:
        if not argv:
            raise ValueError("watchlist requires a subcommand: add | remove | list | show")
        sub = argv[0]
        rest = argv[1:]

        if sub == "list":
            specs = services.watchlist_specs()
            if not specs:
                return "No watchlists."
            lines = ["[bold]Watchlists[/bold]"]
            for name, spec in sorted(specs.items()):
                if spec.get("kind", "tickers") != "tickers":
                    continue
                max_pct = spec.get("max_pct")
                lines.append(
                    f"  {name}  {spec.get('market', '')}  "
                    f"{len(spec.get('tickers', []))} holdings"
                    + (f"  max {max_pct:.0f}%" if max_pct is not None else "")
                )
            return ("\n".join(lines)) if len(lines) > 1 else "No ticker watchlists."

        if sub == "show":
            if not rest:
                raise ValueError("watchlist show requires a name")
            name = rest[0]
            specs = services.watchlist_specs()
            spec = specs.get(name)
            if spec is None or spec.get("kind", "tickers") != "tickers":
                raise ValueError(f"unknown watchlist: {name}")
            return f"{name}: {', '.join(spec['tickers'])} ({spec['market']})"

        if sub == "remove":
            if not rest:
                raise ValueError("watchlist remove requires a name")
            services.remove_watchlist(rest[0])
            return f"Removed watchlist {rest[0]}."

        if sub == "add":
            return self._watchlist_add(rest)

        raise ValueError(f"unknown watchlist subcommand: {sub!r}")

    def _watchlist_add(self, argv: list[str]) -> str:
        name = None
        market = None
        tickers: list[str] = []
        max_pct: float | None = None
        i = 0
        while i < len(argv):
            token = argv[i]
            if token.startswith("--"):
                key = token[2:]
                if i + 1 >= len(argv):
                    raise ValueError(f"missing value for {token}")
                value = argv[i + 1]
                if key == "market":
                    market = value
                elif key == "tickers":
                    tickers = [t.strip().upper() for t in value.split(",") if t.strip()]
                elif key == "max-pct":
                    try:
                        max_pct = float(value)
                    except ValueError:
                        raise ValueError(f"invalid max-pct: {value!r}") from None
                else:
                    raise ValueError(f"unknown option: {token}")
                i += 2
            else:
                name = token
                i += 1
        if not name:
            raise ValueError("watchlist add requires a name")
        if not market:
            raise ValueError("watchlist add requires --market")
        if not tickers:
            raise ValueError("watchlist add requires --tickers")
        services.add_watchlist(name, market=market, tickers=tickers, max_pct=max_pct)
        return f"Added watchlist {name} ({len(tickers)} holdings)."
