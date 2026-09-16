"""Reports screen: pick a watch target, generate its sourced report, read it."""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.widgets import Button, MarkdownViewer, Static

from rigger import services
from rigger.reports import build_report, write_report
from rigger.tui.shell import RiggerScreen, age_text
from rigger.tui.widgets import Pane, PaneRow, PaneStack, RiggerTable, StatusDot


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
    #report-doc {
        width: 1fr;
        margin-right: 1;
    }
    #report-stack {
        width: 38;
        border-left: solid $panel;
        padding-left: 1;
    }
    #report-targets-pane {
        height: auto;
    }
    #report-cites-pane, #report-fresh-pane {
        height: 1fr;
    }
    #report-targets {
        height: auto;
        max-height: 14;
    }
    #report-actions {
        height: auto;
        margin-top: 1;
    }
    #report-view {
        height: 1fr;
    }
    #report-cites, #report-fresh {
        height: 1fr;
    }
    .cite-line {
        height: auto;
        color: $text-muted;
    }
    .fresh-row {
        height: 1;
    }
    .fresh-row Static {
        width: 1fr;
        color: $foreground;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="report-split"):
            with Pane(title="report", icon="", id="report-doc"):
                yield MarkdownViewer(id="report-view", show_table_of_contents=True)
            with PaneStack(id="report-stack"):
                with Pane(title="targets", icon="", id="report-targets-pane"):
                    yield RiggerTable(id="report-targets")
                    with Horizontal(id="report-actions"):
                        yield Button("Generate", id="report-generate", variant="primary")
                with Pane(title="citations", icon="", id="report-cites-pane"):
                    yield VerticalScroll(id="report-cites")
                with Pane(title="freshness", icon="", id="report-fresh-pane"):
                    yield VerticalScroll(id="report-fresh")

    def on_mount(self) -> None:
        table = self.query_one("#report-targets", RiggerTable)
        table.add_columns("Name", "Kind", "Market", "Tickers")
        self.refresh_view()

    def refresh_view(self) -> None:
        table = self.query_one("#report-targets", RiggerTable)
        table.clear()
        specs = sorted(services.target_specs().values(), key=lambda t: t.id)
        self.query_one("#report-targets-pane", Pane).set_badge(str(len(specs)))
        for target in specs:
            table.add_row(
                target.id,
                target.kind,
                ",".join(target.markets),
                ",".join(target.tickers),
                key=target.id,
            )

    async def on_data_table_row_highlighted(self, event: RiggerTable.RowHighlighted) -> None:
        target_id = str(event.row_key.value)
        await self.show_latest(target_id)
        await self._show_freshness(target_id)

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
            await self._show_citations("")
            self.query_one("#report-doc", Pane).set_badge("no report")
            return
        newest = paths[-1]
        text = newest.read_text(encoding="utf-8")
        await viewer.document.update(text)
        self.query_one("#report-doc", Pane).set_badge(f"{target_id} · {newest.stem}")
        await self._show_citations(text)

    async def _show_citations(self, markdown: str) -> None:
        """List the report's cite lines: render_markdown indents them under a claim."""
        pane = self.query_one("#report-cites-pane", Pane)
        box = self.query_one("#report-cites", VerticalScroll)
        await box.remove_children()
        seen: list[str] = []
        for line in markdown.splitlines():
            if line.startswith("  - "):
                cite_text = line[4:].strip()
                if cite_text and cite_text not in seen:
                    seen.append(cite_text)
        pane.set_badge(str(len(seen)))
        if not seen:
            await box.mount(Static("no citations", markup=False, classes="cite-line"))
            return
        await box.mount(
            *(
                Static(f"[{index}] {text}", markup=False, classes="cite-line")
                for index, text in enumerate(seen, start=1)
            )
        )

    async def _show_freshness(self, target_id: str) -> None:
        """Per-instrument data age for the selected target."""
        box = self.query_one("#report-fresh", VerticalScroll)
        await box.remove_children()
        try:
            latest = services.data_health(self.rig).latest_bar
        except Exception:
            latest = {}
        rows = []
        for instrument_id in self._instruments(target_id):
            stamp = latest.get(instrument_id)
            if stamp is None:
                rows.append(Horizontal(StatusDot("error"), Static(f"{instrument_id}  none"),
                                       classes="fresh-row"))
                continue
            if stamp.tzinfo is None:
                stamp = stamp.replace(tzinfo=UTC)
            label, state = age_text(datetime.now(UTC) - stamp)
            rows.append(
                Horizontal(
                    StatusDot(state),
                    Static(f"{instrument_id}  {label}", markup=False),
                    classes="fresh-row",
                )
            )
        await box.mount(*(rows or [Static("no instruments", markup=False, classes="cite-line")]))

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
