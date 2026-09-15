"""In-app command console: run the CLI's target commands without leaving the TUI."""

from __future__ import annotations

import shlex

from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.widgets import Input, RichLog, Static

from rigger import services
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Card


class Console(RiggerScreen):
    name = "console"
    AUTO_FOCUS = "#console-input"

    def __init__(self, rig) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with Card(title="Command console", classes="console-card"):
            yield Static(
                "type help for commands — target add | remove | list | show, config show",
                markup=False,
                classes="muted",
            )
            yield Horizontal(
                Static("❯", markup=False, classes="console-prompt"),
                Input(
                    placeholder="target add mining --kind industry --market asx --tickers BHP,RIO",
                    id="console-input",
                ),
                classes="console-row",
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
                "  target add NAME --kind K --market M --tickers A,B [--tags x,y]"
                " [--notes text] [--label text]\n"
                "  target remove NAME\n"
                "  target list\n"
                "  target show NAME\n"
                "  config show"
            )

        if cmd == "target":
            return self._target(args)

        if cmd == "config" and args and args[0] == "show":
            from rigger.core import config as config_mod

            return str(config_mod.load_toml())

        raise ValueError(f"unknown command: {cmd!r}; try 'help'")

    def _target(self, argv: list[str]) -> str:
        if not argv:
            raise ValueError("target requires a subcommand: add | remove | list | show")
        sub = argv[0]
        rest = argv[1:]

        if sub == "list":
            specs = services.target_specs()
            if not specs:
                return "No targets."
            lines = ["[bold]Targets[/bold]"]
            for target in sorted(specs.values(), key=lambda t: t.id):
                lines.append(
                    f"  {target.id}  {target.kind}  {','.join(target.markets)}"
                    + (f"  {', '.join(target.tickers)}" if target.tickers else "")
                )
            return "\n".join(lines)

        if sub == "show":
            if not rest:
                raise ValueError("target show requires a name")
            target = services.target_specs().get(rest[0])
            if target is None:
                raise ValueError(f"unknown target: {rest[0]}")
            bits = [f"{target.id}: {target.kind} ({','.join(target.markets)})"]
            if target.tickers:
                bits.append(f"tickers {', '.join(target.tickers)}")
            if target.tags:
                bits.append(f"tags {', '.join(sorted(target.tags))}")
            if target.notes:
                bits.append(f"notes {target.notes}")
            return "  ".join(bits)

        if sub == "remove":
            if not rest:
                raise ValueError("target remove requires a name")
            services.remove_target(rest[0])
            return f"Removed target {rest[0]}."

        if sub == "add":
            return self._target_add(rest)

        raise ValueError(f"unknown target subcommand: {sub!r}")

    def _target_add(self, argv: list[str]) -> str:
        name = None
        kind = "company"
        market = None
        tickers: list[str] = []
        tags: list[str] = []
        notes = ""
        label: str | None = None
        i = 0
        while i < len(argv):
            token = argv[i]
            if token.startswith("--"):
                key = token[2:]
                if i + 1 >= len(argv):
                    raise ValueError(f"missing value for {token}")
                value = argv[i + 1]
                if key == "kind":
                    kind = value
                elif key == "market":
                    market = value
                elif key == "tickers":
                    tickers = [t.strip().upper() for t in value.split(",") if t.strip()]
                elif key == "tags":
                    tags = [t.strip() for t in value.split(",") if t.strip()]
                elif key == "notes":
                    notes = value
                elif key == "label":
                    label = value
                else:
                    raise ValueError(f"unknown option: {token}")
                i += 2
            else:
                name = token
                i += 1
        if not name:
            raise ValueError("target add requires a name")
        if not market:
            raise ValueError("target add requires --market")
        services.add_target(
            name, kind=kind, market=market, tickers=tickers, tags=tags, notes=notes, label=label
        )
        return f"Added target {name} ({kind})."
