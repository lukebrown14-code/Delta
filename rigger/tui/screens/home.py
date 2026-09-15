"""Home screen: the dashboard — stat tiles, setup checks, watch list, latest report."""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Button, Sparkline, Static

from rigger import services
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Card, RiggerTable, StatTile, StatusDot


class Home(RiggerScreen):
    name = "home"

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)

    def compose_content(self) -> ComposeResult:
        with Horizontal(classes="stat-row"):
            yield StatTile("targets", "0", id="tile-targets")
            yield StatTile("evidence rows", "0", id="tile-evidence")
            yield StatTile("latest bar (ymd)", "0", id="tile-bar")
            yield StatTile("spend (usd)", "0", id="tile-spend")
        with Card(title="Setup"):
            yield Vertical(id="checks")
        with Card(title="What you watch"):
            yield RiggerTable(id="home-targets")
            yield Vertical(id="home-sparks")
        with Card(title="Latest report", highlight=True):
            with Horizontal(classes="report-row"):
                yield Static("none yet", id="home-report", markup=False)
                yield Button("Open", id="home-report-open", variant="primary")

    async def on_mount(self) -> None:
        self.query_one("#home-targets", RiggerTable).add_columns(
            "Name", "Kind", "Markets", "Tickers"
        )
        await self.refresh_view()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "home-report-open":
            switch = getattr(self.app, "action_switch_screen", None)
            if callable(switch):
                switch("reports")

    async def refresh_view(self) -> None:
        self._refresh_tiles()
        await self._refresh_checks()
        self._refresh_watch()
        await self._refresh_watch_sparks(
            sorted(services.target_specs().values(), key=lambda t: t.id)
        )

    def _refresh_tiles(self) -> None:
        specs = services.target_specs()
        health = services.data_health(self.rig)
        self.query_one("#tile-targets", StatTile).update(str(len(specs)))
        evidence_rows = sum(count for name, count in health.counts.items() if name != "llmcall")
        self.query_one("#tile-evidence", StatTile).update(str(evidence_rows))
        if health.latest_bar:
            newest = max(health.latest_bar.values())
            self.query_one("#tile-bar", StatTile).update(newest.strftime("%Y.%m.%d"))
        else:
            self.query_one("#tile-bar", StatTile).update("0")
        spend = sum(row.cost_usd for row in services.llm_costs(self.rig.engine))
        self.query_one("#tile-spend", StatTile).update(f"{spend:.2f}")

    async def _refresh_checks(self) -> None:
        checks = services.setup_checks(self.rig)
        holder = self.query_one("#checks", Vertical)
        await holder.remove_children()
        for check in checks:
            await holder.mount(
                Horizontal(
                    StatusDot("ok" if check.ok else "error"),
                    Static(check.name + (f" — {check.fix}" if not check.ok else ""), markup=False),
                    classes="check-row",
                )
            )

    def _refresh_watch(self) -> None:
        table = self.query_one("#home-targets", RiggerTable)
        table.clear()
        specs = sorted(services.target_specs().values(), key=lambda t: t.id)
        for target in specs:
            table.add_row(
                target.id,
                target.kind,
                ",".join(target.markets),
                ",".join(target.tickers) or "—",
                key=target.id,
            )
        self._refresh_report()

    async def _refresh_watch_sparks(self, specs: list) -> None:
        holder = self.query_one("#home-sparks", Vertical)
        await holder.remove_children()
        for target in specs:
            instrument = next(
                (inst for inst in self.rig.universe() if target.id in inst.watchlists), None
            )
            if instrument is None:
                continue
            closes = services.recent_closes(self.rig.engine, instrument.id)
            if not closes:
                continue
            await holder.mount(
                Horizontal(
                    Static(instrument.symbol, classes="spark-label", markup=False),
                    Sparkline(closes, min_color="success", max_color="primary"),
                    classes="spark-row",
                )
            )

    def _refresh_report(self) -> None:
        base = Path(getattr(self.rig.cfg, "reports_dir", "reports"))
        # Reports live at base/<instrument>/<date>.md, so sort by filename to
        # get the newest date, not the last ticker alphabetically.
        paths = sorted(base.glob("*/*.md"), key=lambda path: path.name)
        label = self.query_one("#home-report", Static)
        if not paths:
            label.update("none yet — generate one from Reports")
            return
        newest = paths[-1]
        text = newest.read_text(encoding="utf-8")
        citations = text.count("<http")
        mtime = datetime.fromtimestamp(newest.stat().st_mtime, tz=UTC)
        age = datetime.now(UTC) - mtime
        hours = int(age.total_seconds() // 3600)
        when = "just now" if hours < 1 else f"{hours}h ago" if hours < 24 else f"{hours // 24}d ago"
        label.update(f"{newest.stem} · {when} · {citations} citations", markup=False)
