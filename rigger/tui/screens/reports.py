"""Reports screen: pick a watch target, generate its sourced report, read it."""

from __future__ import annotations

from pathlib import Path
from typing import Any

from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.widgets import Button, MarkdownViewer

from rigger import services
from rigger.reports import build_report, write_report
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Card, RiggerTable


class Reports(RiggerScreen):
    """List watch targets, generate reports, and read the latest markdown.

    The orchestrator registers this screen in the app; it also mounts
    standalone against any duck-typed ``rig`` (engine, cfg, llm, universe()).
    """

    name = "reports"
    CSS = """
    #report-split {
        height: 1fr;
    }
    #report-split Card {
        height: 1fr;
    }
    #report-list {
        width: 44;
    }
    #report-doc {
        width: 1fr;
    }
    #report-targets {
        height: 1fr;
    }
    #report-actions {
        height: auto;
        margin-top: 1;
    }
    #report-view {
        height: 1fr;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with Horizontal(id="report-split"):
            with Card(title="Targets", id="report-list"):
                yield RiggerTable(id="report-targets")
                with Horizontal(id="report-actions"):
                    yield Button("Generate", id="report-generate", variant="primary")
            with Card(title="Report", id="report-doc", highlight=True):
                yield MarkdownViewer(id="report-view", show_table_of_contents=True)

    def on_mount(self) -> None:
        table = self.query_one("#report-targets", RiggerTable)
        table.add_columns("Name", "Kind", "Market", "Tickers")
        self.refresh_view()

    def refresh_view(self) -> None:
        table = self.query_one("#report-targets", RiggerTable)
        table.clear()
        for target in sorted(services.target_specs().values(), key=lambda t: t.id):
            table.add_row(
                target.id,
                target.kind,
                ",".join(target.markets),
                ",".join(target.tickers),
                key=target.id,
            )

    async def on_data_table_row_highlighted(self, event: RiggerTable.RowHighlighted) -> None:
        await self.show_latest(str(event.row_key.value))

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id != "report-generate":
            return
        table = self.query_one("#report-targets", RiggerTable)
        # An empty DataTable still reports cursor_row == 0, so row_count is
        # the only reliable "nothing to select" test.
        if table.row_count == 0:
            self.notify("Select a target first", severity="error")
            return
        self.generate(str(table.get_row_at(table.cursor_row)[0]))

    def _instruments(self, target_id: str) -> list[str]:
        return [inst.id for inst in self.rig.universe() if target_id in inst.watchlists]

    async def show_latest(self, target_id: str) -> None:
        """Render the newest persisted report for the target, if any."""
        viewer = self.query_one("#report-view", MarkdownViewer)
        base = Path(getattr(self.rig.cfg, "reports_dir", "reports"))
        # Reports live at base/<instrument>/<date>.md, so sort by filename: a
        # plain path sort would order by instrument first and show the last
        # ticker's report rather than the newest one.
        paths = sorted(
            (path for inst in self._instruments(target_id) for path in (base / inst).glob("*.md")),
            key=lambda path: path.name,
        )
        if not paths:
            await viewer.document.update(
                f"# {target_id}\n\n*No report yet — select the target and press Generate.*"
            )
            return
        await viewer.document.update(paths[-1].read_text(encoding="utf-8"))

    @work
    async def generate(self, target_id: str) -> None:
        """Build and persist a report for each instrument in the target."""
        viewer = self.query_one("#report-view", MarkdownViewer)
        viewer.loading = True
        try:
            instruments = self._instruments(target_id)
            if not instruments:
                self.notify(f"target {target_id} has no instruments to report on", severity="error")
                return
            base = Path(getattr(self.rig.cfg, "reports_dir", "reports"))
            for instrument_id in instruments:
                try:
                    report = await build_report(self.rig, instrument_id)
                except ValueError as exc:
                    self.notify(exc.args[0], severity="error")
                    continue
                write_report(report, base)
                self.notify(f"Report written for {instrument_id}")
            await self.show_latest(target_id)
        finally:
            viewer.loading = False
