"""Reports screen: pick a watch target, generate its sourced report, read it."""

from __future__ import annotations

from pathlib import Path

from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.widgets import Button, DataTable, RichLog

from rigger import services
from rigger.reports import build_report, write_report
from rigger.tui.shell import RiggerScreen


class Reports(RiggerScreen):
    """List watch targets, generate reports, and read the latest markdown.

    The orchestrator registers this screen in the app; it also mounts
    standalone against any duck-typed ``rig`` (engine, cfg, llm, universe()).
    """

    name = "reports"
    CSS = """
    #report-targets {
        height: 10;
        margin: 1 2;
    }
    #report-actions {
        margin: 0 2;
    }
    #report-view {
        height: 1fr;
        margin: 1 2;
    }
    """

    def __init__(self, rig) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        yield DataTable(id="report-targets")
        yield Horizontal(Button("Generate", id="report-generate"), id="report-actions")
        yield RichLog(id="report-view", markup=False, highlight=False)

    def on_mount(self) -> None:
        table = self.query_one("#report-targets", DataTable)
        table.add_columns("Name", "Kind", "Market", "Tickers")
        for target in sorted(services.target_specs().values(), key=lambda t: t.id):
            table.add_row(
                target.id,
                target.kind,
                ",".join(target.markets),
                ",".join(target.tickers),
                key=target.id,
            )

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        self.show_latest(str(event.row_key.value))

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id != "report-generate":
            return
        table = self.query_one("#report-targets", DataTable)
        # An empty DataTable still reports cursor_row == 0, so row_count is the
        # only reliable "nothing to select" test.
        if table.row_count == 0:
            self.notify("Select a target first", severity="error")
            return
        self.generate(str(table.get_row_at(table.cursor_row)[0]))

    def _instruments(self, target_id: str) -> list[str]:
        return [inst.id for inst in self.rig.universe() if target_id in inst.watchlists]

    def show_latest(self, target_id: str) -> None:
        """Render the newest persisted report for the target, if any."""
        log = self.query_one("#report-view", RichLog)
        log.clear()
        base = Path(getattr(self.rig.cfg, "reports_dir", "reports"))
        # Reports live at base/<instrument>/<date>.md, so sort by filename: a
        # plain path sort would order by instrument first and show the last
        # ticker's report rather than the newest one.
        paths = sorted(
            (path for inst in self._instruments(target_id) for path in (base / inst).glob("*.md")),
            key=lambda path: path.name,
        )
        if not paths:
            log.write(f"No report yet for {target_id} — select it and press Generate.")
            return
        log.write(paths[-1].read_text(encoding="utf-8"))

    @work
    async def generate(self, target_id: str) -> None:
        """Build and persist a report for each instrument in the target."""
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
        self.show_latest(target_id)
