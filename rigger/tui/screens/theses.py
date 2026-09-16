"""Keyboard-driven research desk, with a compact evidence review ledger."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime
from typing import Any

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, VerticalScroll
from textual.widget import Widget
from textual.widgets import Button, Input, Select, Static

from rigger import theses, thesis_summary
from rigger.evidence import EvidenceItem, cite, evidence_by_ids
from rigger.thesis_health import badge_text, compute_health
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Dialog, Pane, PaneRow, Pill, RiggerTable, health_variant


class NewThesis(Dialog):
    dialog_title = "new thesis"
    dialog_hint = "tab next field · enter create · esc cancel"
    BINDINGS = [("escape", "dismiss_dialog", "Cancel")]

    def compose_dialog(self) -> ComposeResult:
        yield Input(placeholder="Claim", id="th-claim")
        yield Input(placeholder="Targets (US:AAPL, US:MSFT)", id="th-targets")
        yield Input(placeholder="Time horizon (e.g. 5y)", id="th-horizon")
        yield Button("Create thesis", id="th-add", variant="primary")

    def on_mount(self) -> None:
        self.query_one("#th-claim", Input).focus()

    def on_input_submitted(self) -> None:
        self.create()

    def on_button_pressed(self) -> None:
        self.create()

    def create(self) -> None:
        claim = self.query_one("#th-claim", Input).value.strip()
        if not claim:
            self.notify("claim is required", severity="error")
            return
        targets = tuple(
            t.strip() for t in self.query_one("#th-targets", Input).value.split(",") if t.strip()
        )
        self.dismiss((claim, targets, self.query_one("#th-horizon", Input).value.strip()))


class EditThesis(Dialog):
    dialog_title = "edit thesis"
    dialog_hint = "tab next field · enter save · esc cancel"
    BINDINGS = [("escape", "dismiss_dialog", "Cancel")]

    def __init__(self, thesis: theses.Thesis) -> None:
        super().__init__()
        self.thesis = thesis

    def compose_dialog(self) -> ComposeResult:
        yield Input(value=self.thesis.claim, placeholder="Claim", id="th-edit-claim")
        yield Input(
            value=", ".join(self.thesis.targets),
            placeholder="Targets (US:AAPL, US:MSFT)",
            id="th-edit-targets",
        )
        yield Input(
            value=self.thesis.time_horizon,
            placeholder="Time horizon (e.g. 5y)",
            id="th-edit-horizon",
        )
        yield Select(
            [(status.title(), status) for status in theses.STATUSES],
            value=self.thesis.status,
            allow_blank=False,
            id="th-edit-status",
        )
        yield Button("Save changes", id="th-save", variant="primary")

    def on_mount(self) -> None:
        self.query_one("#th-edit-claim", Input).focus()

    def on_input_submitted(self) -> None:
        self.save()

    def on_button_pressed(self) -> None:
        self.save()

    def save(self) -> None:
        claim = self.query_one("#th-edit-claim", Input).value.strip()
        if not claim:
            self.notify("claim is required", severity="error")
            return
        targets = tuple(
            target.strip()
            for target in self.query_one("#th-edit-targets", Input).value.split(",")
            if target.strip()
        )
        status = str(self.query_one("#th-edit-status", Select).value)
        self.dismiss(
            (claim, targets, self.query_one("#th-edit-horizon", Input).value.strip(), status)
        )


class Theses(RiggerScreen):
    name = "theses"
    BINDINGS = [
        ("n", "new_thesis", "New thesis"),
        ("s", "summarise", "Summarise"),
        ("a", "accept", "Accept evidence"),
        ("x", "reject", "Reject evidence"),
        ("d", "edit_thesis", "Edit thesis"),
        ("e", "toggle_evidence", "Detail / evidence"),
    ]
    WIDE_WIDTH = 110
    DEFAULT_CSS = """
    #thesis-split { height: 1fr; }
    #thesis-claims { width: 30; }
    #thesis-table { height: 1fr; }
    #thesis-detail-pane { width: 1fr; }
    #thesis-evidence-pane { width: 44; }
    #thesis-detail { height: 1fr; }
    #thesis-ledger { height: 1fr; min-height: 4; }
    #thesis-preview { height: 8; border-top: solid $panel; padding-top: 1; }
    #thesis-preview-text { height: auto; }
    #thesis-counts { height: auto; color: $text-muted; margin-bottom: 1; }
    .thesis-claim { text-style: bold; margin-bottom: 1; height: auto; }
    .thesis-meta { height: auto; margin-bottom: 1; }
    .thesis-meta Pill { width: auto; height: 1; margin-right: 1; }
    #thesis-health { width: 1fr; height: auto; margin-bottom: 1; background: transparent; padding: 0; }
    .thesis-section { color: $primary; margin-top: 1; height: 1; }
    #thesis-actions { height: auto; }
    #thesis-actions Button { height: 1; min-width: 0; border: none; padding: 0 1; background: transparent; color: $primary; }
    #thesis-actions Button:focus { background: $primary; color: $background; }
    #thesis-actions Button:disabled { color: $text-muted; }
    #thesis-keys { height: 1; color: $text-muted; }
    Theses.-compact #thesis-evidence-pane { width: 1fr; }
    .side-support { color: $success; }
    .side-against { color: $error; }
    .side-neutral { color: $warning; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self.selected: str | None = None
        self.candidates: list[theses.ThesisEvidence] = []
        self._summary: thesis_summary.ThesisSummary | None = None
        self._wide = True
        self._show_evidence = False
        self._rows: dict[str, theses.ThesisEvidence] = {}
        self._cites: dict[str, str] = {}
        self._summarising = False
        self._render_lock = asyncio.Lock()

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="thesis-split"):
            with Pane(title="theses", id="thesis-claims"):
                yield RiggerTable(id="thesis-table")
            with Pane(title="thesis / detail", id="thesis-detail-pane"):
                yield VerticalScroll(id="thesis-detail")
            with Pane(title="evidence", id="thesis-evidence-pane"):
                yield Static("", id="thesis-counts", markup=False)
                yield RiggerTable(id="thesis-ledger")
                with VerticalScroll(id="thesis-preview"):
                    yield Static(
                        "Select evidence to read its note and citation.",
                        id="thesis-preview-text",
                        markup=False,
                    )
                with Horizontal(id="thesis-actions"):
                    yield Button("a accept", id="th-accept")
                    yield Button("x reject", id="th-reject")
        yield Static(
            "↑↓ move  enter select  tab pane  n new  d edit  s summary  e evidence",
            id="thesis-keys",
        )

    async def on_mount(self) -> None:
        self.query_one("#thesis-table", RiggerTable).add_column("Claim", width=15)
        self.query_one("#thesis-table", RiggerTable).add_column("Status", width=9)
        self.query_one("#thesis-ledger", RiggerTable).add_column("State", width=8)
        self.query_one("#thesis-ledger", RiggerTable).add_column("±", width=1)
        self.query_one("#thesis-ledger", RiggerTable).add_column("Note", width=24)
        self._layout()
        await self.refresh_view()
        self.query_one("#thesis-table").focus()

    def on_resize(self) -> None:
        if self.is_mounted:
            self._layout()

    def _layout(self) -> None:
        self._wide = self.size.width >= self.WIDE_WIDTH
        self.set_class(not self._wide, "-compact")
        self.query_one("#thesis-detail-pane").display = self._wide or not self._show_evidence
        self.query_one("#thesis-evidence-pane").display = self._wide or self._show_evidence

    def action_toggle_evidence(self) -> None:
        self._show_evidence = not self._show_evidence
        self._layout()
        self.query_one(
            "#thesis-ledger" if self._wide or self._show_evidence else "#thesis-detail"
        ).focus()

    async def refresh_view(self) -> None:
        self.refresh_list()
        await self.render_detail()

    def refresh_list(self) -> None:
        table = self.query_one("#thesis-table", RiggerTable)
        table.clear()
        rows = theses.list_theses(self.rig.engine)
        if self.selected not in {row.id for row in rows}:
            self.selected = rows[0].id if rows else None
            self._summary = None
        for row in rows:
            table.add_row(row.claim, row.status, key=row.id)
        if self.selected:
            table.move_cursor(row=next(i for i, row in enumerate(rows) if row.id == self.selected))
        self.query_one("#thesis-claims", Pane).set_badge(str(len(rows)))

    async def on_data_table_row_selected(self, event: RiggerTable.RowSelected) -> None:
        if event.data_table.id == "thesis-table" and event.row_key.value is not None:
            self.selected = str(event.row_key.value)
            self._summary = None
            await self.render_detail()

    def on_data_table_row_highlighted(self, event: RiggerTable.RowHighlighted) -> None:
        if event.data_table.id == "thesis-ledger":
            self._preview()

    def action_new_thesis(self) -> None:
        self.app.push_screen(NewThesis(), self._create_thesis)

    def action_edit_thesis(self) -> None:
        if self.selected is None:
            self.notify("select a thesis first", severity="error")
            return
        self.app.push_screen(
            EditThesis(theses.get_thesis(self.rig.engine, self.selected)), self._update_thesis
        )

    async def _update_thesis(self, result) -> None:
        if result is None or self.selected is None:
            return
        claim, targets, horizon, status = result
        try:
            thesis = theses.update_thesis(
                self.rig.engine,
                self.selected,
                claim=claim,
                targets=targets,
                time_horizon=horizon,
                status=status,
            )
        except (KeyError, ValueError) as exc:
            self.notify(str(exc), severity="error")
            return
        self.selected = thesis.id
        self._summary = None
        await self.refresh_view()

    async def _create_thesis(self, result) -> None:
        if result is None:
            return
        claim, targets, horizon = result
        try:
            thesis = theses.create_thesis(
                self.rig.engine, claim, targets=targets, time_horizon=horizon
            )
        except ValueError as exc:
            self.notify(str(exc), severity="error")
            return
        self.selected = thesis.id
        self._summary = None
        await self.refresh_view()

    @work(exclusive=True, group="thesis-summary")
    async def action_summarise(self) -> None:
        await self._summarise()

    async def _summarise(self) -> None:
        if self.selected is None:
            self.notify("select a thesis first", severity="error")
            return
        if self._summarising:
            return
        selected = self.selected
        self._summarising = True
        self.query_one("#thesis-detail-pane", Pane).set_badge("summarising…")
        try:
            summary = await thesis_summary.summarize_thesis(self.rig, selected)
            if self.selected == selected:
                self._summary = summary
                await self.render_detail()
        except Exception as exc:
            self.notify(f"summary failed: {exc}", severity="error")
        finally:
            self._summarising = False
            self.query_one("#thesis-detail-pane", Pane).set_badge("")

    def _current_row(self) -> theses.ThesisEvidence | None:
        table = self.query_one("#thesis-ledger", RiggerTable)
        if not table.row_count:
            return None
        return self._rows.get(
            str(table.coordinate_to_cell_key(table.cursor_coordinate).row_key.value)
        )

    def _preview(self) -> None:
        row = self._current_row()
        text = "No evidence yet. Gather and classify evidence to begin review."
        if row:
            side = {"support": "Supporting", "against": "Against", "neutral": "Neutral"}[row.side]
            text = f"{side} · {'accepted' if row.accepted else 'pending review'}\n\n{row.note}\n\n{self._cites.get(row.evidence_id, row.evidence_id)}"
        self.query_one("#thesis-preview-text", Static).update(text)
        for button in self.query("#thesis-actions Button"):
            button.disabled = row is None or row.accepted

    async def action_accept(self) -> None:
        await self._resolve(True)

    async def action_reject(self) -> None:
        await self._resolve(False)

    async def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id in ("th-accept", "th-reject"):
            await self._resolve(event.button.id == "th-accept")

    async def _resolve(self, accept: bool) -> None:
        # Hidden evidence must not be modified by a stray shortcut.
        if not self._wide and not self._show_evidence:
            return
        row = self._current_row()
        if row is None or row.accepted:
            return
        if accept:
            theses.set_accepted(self.rig.engine, row.thesis_id, row.evidence_id, True)
        else:
            theses.remove_evidence(self.rig.engine, row.thesis_id, row.evidence_id)
        self._summary = None
        await self.render_detail()
        self.query_one("#thesis-ledger").focus()

    async def render_detail(self) -> None:
        async with self._render_lock:
            await self._render_detail()

    async def _render_detail(self) -> None:
        detail = self.query_one("#thesis-detail", VerticalScroll)
        await detail.remove_children()
        table = self.query_one("#thesis-ledger", RiggerTable)
        cursor = table.cursor_row
        table.clear()
        self._rows = {}
        self.candidates = []
        if self.selected is None:
            await detail.mount(
                Static("No theses yet.\n\nPress n to create a claim to research.", markup=False)
            )
            self.query_one("#thesis-counts", Static).update("No evidence")
            self.query_one("#thesis-evidence-pane", Pane).set_badge("0")
            self._preview()
            return
        thesis = theses.get_thesis(self.rig.engine, self.selected)
        links = theses.evidence_for(self.rig.engine, thesis.id, accepted_only=False)
        groups = {
            side: [r for r in links if r.accepted and r.side == side]
            for side in ("support", "against", "neutral")
        }
        self.candidates = [r for r in links if not r.accepted]
        items = evidence_by_ids(self.rig.engine, [r.evidence_id for r in links])
        self._cites = {item.id: cite(item) for item in items}
        widgets = [
            Static(thesis.claim, classes="thesis-claim", markup=False),
            Horizontal(
                Pill(thesis.status),
                Pill(thesis.time_horizon or "no horizon", variant="dim"),
                classes="thesis-meta",
            ),
            self._health_pill(thesis, groups, {i.id: i for i in items}),
            Static("targets  " + (", ".join(thesis.targets) or "all evidence"), markup=False),
        ]
        if thesis.scope:
            widgets.append(Static(f"scope    {thesis.scope}", markup=False))
        widgets.append(Static("SUMMARY", classes="thesis-section"))
        widgets.extend(self._summary_widgets())
        widgets.append(Static(f"REVIEW QUEUE  {len(self.candidates):02}", classes="thesis-section"))
        widgets.append(
            Static(
                "Press e to review pending evidence."
                if self.candidates
                else "All caught up. No pending evidence.",
                markup=False,
            )
        )
        await detail.mount(*widgets)
        for row in sorted(links, key=lambda r: r.accepted):
            self._rows[row.evidence_id] = row
            symbol, token = {
                "support": ("+", "success"),
                "against": ("−", "error"),
                "neutral": ("?", "warning"),
            }[row.side]
            color = self.app.current_theme.to_color_system().generate().get(token, "white")
            table.add_row(
                "accepted" if row.accepted else "pending",
                Text(symbol, style=color),
                row.note or row.evidence_id,
                key=row.evidence_id,
            )
        if table.row_count:
            table.move_cursor(row=min(cursor, table.row_count - 1))
        self.query_one("#thesis-counts", Static).update(
            f"+ {len(groups['support'])} supporting  − {len(groups['against'])} against  ? {len(groups['neutral'])} neutral"
        )
        self.query_one("#thesis-evidence-pane", Pane).set_badge(
            f"{len(self.candidates)} pending / {len(links)} total"
        )
        self._preview()

    def _summary_widgets(self) -> list[Widget]:
        if self._summary is None:
            return [
                Static("No summary yet. Press s to summarise accepted evidence.", classes="muted")
            ]
        widgets = [
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
