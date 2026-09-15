"""Pipeline screen."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.screen import Screen
from textual.widgets import Button, RichLog, Static

from rigger import services


class Pipeline(Screen):
    name = "pipeline"

    def __init__(self, rig) -> None:
        super().__init__()
        self.rig = rig

    def compose(self) -> ComposeResult:
        yield Horizontal(
            VerticalScroll(
                Static("[bold]Steps[/bold]"),
                Button("Ingest", id="ingest"),
                Button("Extract", id="extract"),
                Button("Analyse", id="analyse"),
                Button("Execute", id="execute"),
                Button("Report", id="report"),
                Button("Run all", id="run-all", variant="primary"),
            ),
            RichLog(id="pipeline-log"),
        )

    def on_mount(self) -> None:
        self.query_one("#pipeline-log", RichLog).write("Ready. Click a step or Run all.")

    def _log(self, message: str) -> None:
        try:
            self.query_one("#pipeline-log", RichLog).write(message)
        except Exception:
            pass

    def on_button_pressed(self, event: Button.Pressed) -> None:
        step = event.button.id
        if step not in ("ingest", "extract", "analyse", "execute", "report", "run-all"):
            return
        self._run_pipeline(step)

    def _run_pipeline(self, step: str) -> None:
        self.run_worker(self._execute(step), thread=True)

    async def _execute(self, step: str) -> None:
        log = self._log
        try:
            if step in ("ingest", "run-all"):
                await services.ingest(self.rig, log=log)
            if step in ("extract", "run-all"):
                await services.extract(self.rig, log=log)
            if step in ("analyse", "run-all"):
                await services.analyse(self.rig, log=log)
            if step in ("execute", "run-all"):
                await services.execute(self.rig, log=log)
            if step in ("report", "run-all"):
                path = services.report(self.rig)
                log(f"[green]Report written to {path}[/green]")
        except Exception as exc:
            log(f"[red]{exc}[/red]")
