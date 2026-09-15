"""Optional theses panel: create a claim, review candidates, accept or reject them.

Standalone by design: layout comes from ``DEFAULT_CSS`` (RiggerScreen ships
its own shell styles, no ``app.py`` stylesheet needed) and only
``rig.engine`` is touched, so the screen works under any App with a
Rigger-like object.
"""

from __future__ import annotations

from datetime import UTC, datetime

from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.widget import Widget
from textual.widgets import Button, DataTable, Input, Static

from rigger import theses, thesis_summary
from rigger.evidence import EvidenceItem, cite, evidence_by_ids
from rigger.thesis_health import badge_text, compute_health, state_style
from rigger.tui.shell import RiggerScreen


class Theses(RiggerScreen):
    name = "theses"

    DEFAULT_CSS = """
    #thesis-table {
        height: 1fr;
        margin: 1 2;
    }
    #th-claim, #th-targets, #th-horizon {
        width: 1fr;
        margin: 1;
    }
    #thesis-detail-title {
        padding: 1 2 0 2;
    }
    #thesis-detail {
        height: auto;
        margin: 0 2 1 2;
    }
    #thesis-detail Horizontal {
        height: auto;
    }
    #thesis-detail Static {
        width: 1fr;
    }
    """

    def __init__(self, rig) -> None:
        super().__init__(rig)
        self.selected: str | None = None
        self.candidates: list[theses.ThesisEvidence] = []
        self._summary: thesis_summary.ThesisSummary | None = None

    def compose_content(self) -> ComposeResult:
        yield VerticalScroll(
            DataTable(id="thesis-table"),
            Horizontal(
                Input(placeholder="claim", id="th-claim"),
                Input(placeholder="targets (US:AAPL,US:MSFT)", id="th-targets"),
                Input(placeholder="time horizon (e.g. 10y)", id="th-horizon"),
                Button("Add", id="th-add"),
                Button("Summarise", id="th-summarise"),
            ),
            Static(id="thesis-detail-title"),
            VerticalScroll(id="thesis-detail"),
        )

    async def on_mount(self) -> None:
        table = self.query_one("#thesis-table", DataTable)
        table.cursor_type = "row"
        table.add_columns("Claim", "Status")
        self.refresh_list()
        await self.render_detail()

    def refresh_list(self) -> None:
        table = self.query_one("#thesis-table", DataTable)
        table.clear()
        for thesis in theses.list_theses(self.rig.engine):
            table.add_row(thesis.claim, thesis.status, key=thesis.id)

    async def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        if event.row_key.value is not None:
            self.selected = event.row_key.value
            self._summary = None
            await self.render_detail()

    async def on_button_pressed(self, event: Button.Pressed) -> None:
        button_id = event.button.id or ""
        if button_id == "th-add":
            await self._add_thesis()
        elif button_id == "th-summarise":
            await self._summarise()
        elif button_id.startswith(("th-accept-", "th-reject-")):
            await self._resolve_candidate(button_id)

    async def _add_thesis(self) -> None:
        claim = self.query_one("#th-claim", Input).value.strip()
        targets_text = self.query_one("#th-targets", Input).value.strip()
        time_horizon = self.query_one("#th-horizon", Input).value.strip()
        if not claim:
            self.notify("claim is required", severity="error")
            return
        targets = tuple(target.strip() for target in targets_text.split(",") if target.strip())
        try:
            thesis = theses.create_thesis(
                self.rig.engine, claim, targets=targets, time_horizon=time_horizon
            )
        except ValueError as exc:
            self.notify(exc.args[0], severity="error")
            return
        for input_id in ("#th-claim", "#th-targets", "#th-horizon"):
            self.query_one(input_id, Input).value = ""
        self.refresh_list()
        self.selected = thesis.id
        self._summary = None
        await self.render_detail()
        self.notify(f"Added thesis {thesis.id}")

    async def _summarise(self) -> None:
        if self.selected is None:
            self.notify("select a thesis first", severity="error")
            return
        try:
            self._summary = await thesis_summary.summarize_thesis(self.rig, self.selected)
        except Exception as exc:  # model key, network, validation — keep the panel usable
            self.notify(f"summary failed: {exc}", severity="error")
            return
        await self.render_detail()

    async def _resolve_candidate(self, button_id: str) -> None:
        index = int(button_id.rsplit("-", 1)[-1])
        if index >= len(self.candidates):
            return
        candidate = self.candidates[index]
        if button_id.startswith("th-accept-"):
            theses.set_accepted(self.rig.engine, candidate.thesis_id, candidate.evidence_id, True)
            self.notify(f"Accepted {candidate.evidence_id}")
        else:
            theses.remove_evidence(self.rig.engine, candidate.thesis_id, candidate.evidence_id)
            self.notify(f"Rejected {candidate.evidence_id}")
        await self.render_detail()

    async def render_detail(self) -> None:
        """Rebuild the detail pane for the selected thesis, or the empty prompt."""
        title = self.query_one("#thesis-detail-title", Static)
        detail = self.query_one("#thesis-detail", VerticalScroll)
        await detail.remove_children()
        self.candidates = []
        if self.selected is None:
            title.update("")
            await detail.mount(Static("Select a thesis to see its evidence.", classes="diagram"))
            return
        try:
            thesis = theses.get_thesis(self.rig.engine, self.selected)
        except KeyError:
            self.selected = None
            title.update("")
            return
        headline = f"[bold]{thesis.claim}[/bold] — {thesis.status}"
        if thesis.time_horizon:
            headline += f" ({thesis.time_horizon})"
        if thesis.scope:
            headline += f"\nscope: {thesis.scope}"
        title.update(headline)

        groups: dict[str, list[theses.ThesisEvidence]] = {
            "support": [],
            "against": [],
            "neutral": [],
        }
        links = theses.evidence_for(self.rig.engine, thesis.id, accepted_only=False)
        for row in links:
            if row.accepted:
                groups[row.side].append(row)
            else:
                self.candidates.append(row)

        # Fetch by linked id, not from the recent-evidence window: accepted
        # evidence that has aged out of the pool still belongs to the thesis.
        items = evidence_by_ids(self.rig.engine, [row.evidence_id for row in links])
        cites = {item.id: cite(item) for item in items}
        by_id = {item.id: item for item in items}

        widgets: list[Widget] = [self._health_badge(thesis, groups, by_id)]
        widgets.append(Static("[bold]Summary[/bold]"))
        if self._summary is None:
            widgets.append(Static("press Summarise to generate", classes="diagram"))
        else:
            widgets.append(Static(self._summary.summary, markup=False))
            if self._summary.strongest_support:
                widgets.append(Static(f"support: {self._summary.strongest_support}", markup=False))
            if self._summary.strongest_counter:
                widgets.append(Static(f"counter: {self._summary.strongest_counter}", markup=False))
            for unknown in self._summary.unknowns:
                widgets.append(Static(f"unknown: {unknown}", markup=False))
        for heading, side in (
            ("Supporting", "support"),
            ("Against", "against"),
            ("Unknown", "neutral"),
        ):
            widgets.append(Static(f"[bold]{heading}[/bold]"))
            accepted_rows = groups[side]
            if accepted_rows:
                widgets.extend(Static(f"  {self._line(row, cites)}") for row in accepted_rows)
            else:
                widgets.append(Static("  none", classes="diagram"))
        widgets.append(Static("[bold]Candidates[/bold]"))
        if self.candidates:
            for index, row in enumerate(self.candidates):
                widgets.append(
                    Horizontal(
                        Static(f"  {self._line(row, cites)}"),
                        Button("Accept", id=f"th-accept-{index}"),
                        Button("Reject", id=f"th-reject-{index}"),
                    )
                )
        else:
            widgets.append(Static("  none", classes="diagram"))
        await detail.mount(*widgets)

    @staticmethod
    def _line(row: theses.ThesisEvidence, cites: dict[str, str]) -> str:
        citation = cites.get(row.evidence_id, row.evidence_id)
        return f"[{row.side}] {row.note} — {citation}"

    def _health_badge(
        self,
        thesis: theses.Thesis,
        groups: dict[str, list[theses.ThesisEvidence]],
        by_id: dict[str, EvidenceItem],
    ) -> Static:
        linked = [
            (by_id[row.evidence_id], row.side)
            for rows in groups.values()
            for row in rows
            if row.evidence_id in by_id
        ]
        if not linked:
            return Static("[cyan]emerging · no accepted evidence[/cyan]", id="thesis-health")
        result = compute_health(thesis, linked, now=datetime.now(UTC))
        return Static(
            f"[{state_style(result.state)}]{badge_text(result)}[/]",
            id="thesis-health",
        )
