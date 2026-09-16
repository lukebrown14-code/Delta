"""Theses panel: create a claim, review candidates, accept or reject them.

Standalone by design: layout comes from ``DEFAULT_CSS`` (RiggerScreen ships
its own shell styles, no ``app.py`` stylesheet needed) and only
``rig.engine`` is touched, so the screen works under any App with a
Rigger-like object.
"""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widget import Widget
from textual.widgets import Button, Collapsible, Input, Static

from rigger import theses, thesis_summary
from rigger.evidence import EvidenceItem, cite, evidence_by_ids
from rigger.thesis_health import badge_text, compute_health
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Pane, PaneRow, Pill, RiggerTable, health_variant


class Theses(RiggerScreen):
    name = "theses"

    DEFAULT_CSS = """
    #thesis-split {
        height: 1fr;
    }
    #thesis-claims {
        width: 34;
    }
    #thesis-table {
        height: 1fr;
    }
    #thesis-form {
        height: auto;
    }
    #thesis-form Input {
        width: 1fr;
        margin: 0;
    }
    .thesis-buttons {
        height: auto;
    }
    #thesis-detail-pane {
        width: 1fr;
    }
    #thesis-evidence-pane {
        width: 40;
    }
    #thesis-detail, #thesis-evidence {
        height: 1fr;
    }
    .thesis-claim {
        text-style: bold;
        margin: 0 0 1 0;
    }
    .thesis-meta {
        height: auto;
        margin: 0 0 1 0;
    }
    .thesis-line {
        height: auto;
        margin: 0 0 1 0;
    }
    .thesis-line Static {
        width: 1fr;
    }
    .thesis-line Button {
        margin: 0 0 0 1;
    }
    .side-support { color: $success; }
    .side-against { color: $error; }
    .side-neutral { color: $warning; }
    """

    #: Below this width the evidence pane folds into the detail column: three
    #: columns leave the detail pane a few characters wide on a small terminal.
    WIDE_WIDTH = 110

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self._wide = True
        self.selected: str | None = None
        self.candidates: list[theses.ThesisEvidence] = []
        self._summary: thesis_summary.ThesisSummary | None = None

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="thesis-split"):
            with Pane(title="claims", icon="", id="thesis-claims"):
                yield RiggerTable(id="thesis-table")
                with Vertical(id="thesis-form"):
                    yield Input(placeholder="claim", id="th-claim")
                    yield Input(placeholder="targets (US:AAPL,US:MSFT)", id="th-targets")
                    yield Input(placeholder="time horizon (e.g. 10y)", id="th-horizon")
                    with Horizontal(classes="thesis-buttons"):
                        yield Button("Add", id="th-add", variant="primary")
                        yield Button("Summarise", id="th-summarise")
            with Pane(title="detail", icon="", id="thesis-detail-pane"):
                with VerticalScroll(id="thesis-detail"):
                    yield Static("Select a thesis to see its evidence.", classes="muted")
            with Pane(title="evidence", icon="", id="thesis-evidence-pane"):
                with VerticalScroll(id="thesis-evidence"):
                    yield Static("", classes="muted")

    async def on_resize(self, event) -> None:
        wide = event.size.width >= self.WIDE_WIDTH
        if wide == self._wide:
            return
        self._wide = wide
        self.query_one("#thesis-evidence-pane").display = wide
        await self.render_detail()

    async def on_mount(self) -> None:
        table = self.query_one("#thesis-table", RiggerTable)
        table.add_columns("Claim", "Status")
        self.refresh_list()
        await self.render_detail()

    def refresh_list(self) -> None:
        table = self.query_one("#thesis-table", RiggerTable)
        table.clear()
        rows = theses.list_theses(self.rig.engine)
        for thesis in rows:
            table.add_row(thesis.claim, thesis.status, key=thesis.id)
        self.query_one("#thesis-claims", Pane).set_badge(str(len(rows)))

    async def on_data_table_row_selected(self, event: RiggerTable.RowSelected) -> None:
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
        detail = self.query_one("#thesis-detail", VerticalScroll)
        evidence = self.query_one("#thesis-evidence", VerticalScroll)
        await detail.remove_children()
        await evidence.remove_children()
        self.candidates = []
        if self.selected is None:
            await detail.mount(Static("Select a thesis to see its evidence.", classes="muted"))
            self.query_one("#thesis-evidence-pane", Pane).set_badge("")
            return
        try:
            thesis = theses.get_thesis(self.rig.engine, self.selected)
        except KeyError:
            self.selected = None
            return

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

        widgets: list[Widget] = [Static(thesis.claim, classes="thesis-claim", markup=False)]
        meta: list[Widget] = [Pill(thesis.status)]
        if thesis.time_horizon:
            meta.append(Pill(thesis.time_horizon, variant="dim"))
        meta.append(self._health_pill(thesis, groups, by_id))
        widgets.append(Horizontal(*meta, classes="thesis-meta"))
        if thesis.scope:
            widgets.append(Static(f"scope: {thesis.scope}", markup=False, classes="muted"))

        widgets.extend(self._summary_widgets())
        if self.candidates:
            widgets.append(Static("Candidates", classes="thesis-claim"))
            for index, row in enumerate(self.candidates):
                widgets.append(
                    Horizontal(
                        self._line_widget(row, cites, "neutral"),
                        Button("Accept", id=f"th-accept-{index}", variant="success"),
                        Button("Reject", id=f"th-reject-{index}", variant="error"),
                        classes="thesis-line",
                    )
                )
        # The accepted-evidence sides live in their own pane: the detail pane
        # stays a readable column, the evidence pane a scannable list.
        sides: list[Widget] = []
        for heading, side in (
            ("Supporting", "support"),
            ("Against", "against"),
            ("Unknown", "neutral"),
        ):
            accepted_rows = groups[side]
            sides.append(
                Collapsible(
                    *(self._line_widget(row, cites, side) for row in accepted_rows)
                    or (Static("none yet", markup=False, classes="muted"),),
                    title=f"{heading} ({len(accepted_rows)})",
                    collapsed=False,
                    classes=f"side-{side}",
                )
            )
        accepted = sum(len(rows) for rows in groups.values())
        self.query_one("#thesis-evidence-pane", Pane).set_badge(str(accepted))
        if self._wide:
            await detail.mount(*widgets)
            await evidence.mount(*sides)
        else:
            await detail.mount(*widgets, *sides)

    def _summary_widgets(self) -> list[Widget]:
        if self._summary is None:
            return [Static("press Summarise to generate", classes="muted")]
        widgets = [
            Static("Summary", classes="thesis-claim"),
            Static(self._summary.summary, markup=False),
        ]
        if self._summary.strongest_support:
            widgets.append(
                Static(f"support: {self._summary.strongest_support}", markup=False, classes="muted")
            )
        if self._summary.strongest_counter:
            widgets.append(
                Static(f"counter: {self._summary.strongest_counter}", markup=False, classes="muted")
            )
        for unknown in self._summary.unknowns:
            widgets.append(Static(f"unknown: {unknown}", markup=False, classes="muted"))
        return widgets

    @staticmethod
    def _line_widget(row: theses.ThesisEvidence, cites: dict[str, str], side: str) -> Static:
        citation = cites.get(row.evidence_id, row.evidence_id)
        return Static(f"{row.note} — {citation}", markup=False, classes=f"side-{side}")

    def _health_pill(
        self,
        thesis: theses.Thesis,
        groups: dict[str, list[theses.ThesisEvidence]],
        by_id: dict[str, EvidenceItem],
    ) -> Pill:
        linked = [
            (by_id[row.evidence_id], row.side)
            for rows in groups.values()
            for row in rows
            if row.evidence_id in by_id
        ]
        if not linked:
            return Pill("emerging · no accepted evidence", variant="dim", id="thesis-health")
        result = compute_health(thesis, linked, now=datetime.now(UTC))
        return Pill(badge_text(result), variant=health_variant(result.state), id="thesis-health")
