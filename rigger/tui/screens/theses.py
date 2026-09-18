"""Keyboard-driven research desk, with a compact evidence review ledger."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime
from typing import Any

from rich.errors import StyleSyntaxError
from rich.style import Style
from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, VerticalScroll
from textual.markup import escape
from textual.widget import Widget
from textual.widgets import Button, Input, Select, Static

from rigger import theses, thesis_summary
from rigger.core.time import to_utc
from rigger.evidence import EvidenceItem, cite, evidence_by_ids
from rigger.thesis_health import HealthResult, compute_health
from rigger.tui.shell import RiggerScreen, age_text
from rigger.tui.widgets import Dialog, Pane, PaneRow, RiggerTable, hint_markup

#: Per side: ledger glyph, the theme token that colours it, and the word the
#: preview uses. One table so the ledger, the legend and the preview cannot
#: disagree about what a side is called.
SIDE_MARKS: dict[str, tuple[str, str, str]] = {
    "support": ("+", "text-success", "Supporting"),
    "against": ("−", "text-error", "Against"),
    "neutral": ("?", "text-warning", "Neutral"),
}

#: Claims-list health marker: glyph plus the theme token that colours it.
#: Shape carries the state as well as hue, so the two reds (weakening,
#: challenged) and the two hollow rings (emerging, idle) still read apart in
#: a monochrome terminal. Keys are ``thesis_health.HealthState`` values plus
#: ``concluded``, which is a thesis *status*: a concluded claim shows dimmed
#: whatever its evidence says. Every glyph is single-cell so the claim text
#: stays aligned down the column.
HEALTH_MARKS: dict[str, tuple[str, str]] = {
    "building": ("▲", "text-success"),
    "weakening": ("▼", "text-error"),
    "mixed": ("◆", "foreground"),
    "challenged": ("✕", "text-error"),
    "emerging": ("○", "text-muted"),
    "idle": ("◌", "text-warning"),
    "concluded": ("○", "text-muted"),
}

#: Pane widths at the wide layout: the claims list and the ledger are
#: fixed-shape tables, so they keep fixed widths and the thesis pane takes
#: the rest (42 columns at 120). ``Theses.CSS`` repeats these numbers because
#: Textual CSS cannot read them — ``test_pane_width_constants_match_the_stylesheet``
#: holds the two in step.
CLAIMS_WIDTH = 36
EVIDENCE_WIDTH = 40

#: Narrow-list columns (health word, tilt, queue).
LIST_HEALTH_WIDTH = 10
LIST_TILT_WIDTH = 5
LIST_QUEUE_WIDTH = 9


def _csv(value: str) -> tuple[str, ...]:
    """Split a comma-separated input into stripped, non-empty parts."""
    return tuple(part.strip() for part in value.split(",") if part.strip())


class ThesisForm(Dialog):
    """Create or edit a thesis: one form, seeded when editing.

    New and edit differ only by seeded values, the status field and the button
    label, so they are one class. Six framing fields at the app's default
    three-row Input would overflow a 24-row terminal and clip the submit
    button, so fields here are one row with a left marker for focus instead of
    a full border.
    """

    dialog_hint = hint_markup(("tab", "next field"), ("enter", "save"), ("esc", "cancel"))

    DEFAULT_CSS = """
    ThesisForm Input, ThesisForm Select {
        height: 1;
        border: none;
        border-left: thick $panel;
        background: $panel;
        padding: 0 1;
        margin: 0 0 1 0;
    }
    ThesisForm Input:focus, ThesisForm Select:focus {
        border-left: thick $primary;
    }
    ThesisForm SelectCurrent {
        border: none;
        padding: 0;
    }
    ThesisForm Button {
        height: 1;
        min-width: 0;
        border: none;
        padding: 0 1;
    }
    ThesisForm .form-row {
        height: auto;
    }
    ThesisForm .form-row Input, ThesisForm .form-row Select {
        width: 1fr;
    }
    """

    def __init__(
        self,
        thesis: theses.Thesis | None = None,
        *,
        claim: str = "",
        targets: str = "",
    ) -> None:
        super().__init__()
        self.thesis = thesis
        self.dialog_title = "edit thesis" if thesis else "new thesis"
        self._seed = {
            "claim": thesis.claim if thesis else claim,
            "targets": ", ".join(thesis.targets) if thesis else targets,
            "horizon": thesis.time_horizon if thesis else "",
            "scope": thesis.scope if thesis else "",
            "assumptions": ", ".join(thesis.assumptions) if thesis else "",
            "falsifiers": ", ".join(thesis.falsifiers) if thesis else "",
        }

    def compose_dialog(self) -> ComposeResult:
        yield Input(value=self._seed["claim"], placeholder="Claim", id="th-claim")
        with Horizontal(classes="form-row"):
            yield Input(
                value=self._seed["targets"],
                placeholder="Targets (US:AAPL)",
                id="th-targets",
            )
            yield Input(value=self._seed["horizon"], placeholder="Horizon (5y)", id="th-horizon")
        with Horizontal(classes="form-row"):
            yield Input(
                value=self._seed["scope"],
                placeholder="Scope (what the claim is about)",
                id="th-scope",
            )
            if self.thesis is not None:
                yield Select(
                    [(status.title(), status) for status in theses.STATUSES],
                    value=self.thesis.status,
                    allow_blank=False,
                    id="th-status",
                )
        yield Input(
            value=self._seed["assumptions"],
            placeholder="Holds if — assumptions, comma separated",
            id="th-assumptions",
        )
        yield Input(
            value=self._seed["falsifiers"],
            placeholder="Breaks if — what would disprove it",
            id="th-falsifiers",
        )
        yield Button(
            "Save changes" if self.thesis else "Create thesis",
            id="th-save",
            variant="primary",
        )

    def on_mount(self) -> None:
        self.query_one("#th-claim", Input).focus()

    def on_input_submitted(self) -> None:
        self.save()

    def on_button_pressed(self) -> None:
        self.save()

    def _value(self, field: str) -> str:
        return self.query_one(f"#th-{field}", Input).value.strip()

    def save(self) -> None:
        """Dismiss with the ``create_thesis``/``update_thesis`` keywords.

        A dict rather than a tuple: the two calls take different field sets, and
        a positional contract silently mis-binds when one of them gains a field.
        """
        claim = self._value("claim")
        if not claim:
            self.notify("claim is required", severity="error")
            return
        fields: dict[str, Any] = {
            "claim": claim,
            "targets": _csv(self.query_one("#th-targets", Input).value),
            "time_horizon": self._value("horizon"),
            "scope": self._value("scope"),
            "assumptions": _csv(self.query_one("#th-assumptions", Input).value),
            "falsifiers": _csv(self.query_one("#th-falsifiers", Input).value),
        }
        if self.thesis is not None:
            fields["status"] = str(self.query_one("#th-status", Select).value)
        self.dismiss(fields)


class HealthPane(Pane):
    """A ``Pane`` whose badge can carry a theme colour.

    ``Pane`` paints every badge in ``$text-muted``; the thesis pane's badge
    is the health glyph and state, which reads in its own colour everywhere
    else on the screen. Local until ``Pane.set_badge`` grows a token argument.
    """

    def __init__(self, *children, **kwargs) -> None:
        self._badge_token = "text-muted"
        super().__init__(*children, **kwargs)

    def set_badge(self, text: str, token: str = "text-muted") -> None:  # type: ignore[override]
        self._badge_token = token
        super().set_badge(text)

    def _paint(self) -> None:
        super()._paint()
        if self._badge:
            parts = []
            if self._key:
                parts.append(f"[bold]{escape(self._key)}[/bold]")
            if self._title:
                parts.append(escape(self._title))
            parts.append(f"[${self._badge_token}]· {escape(self._badge)}[/]")
            self.border_title = " ".join(parts)


class Theses(RiggerScreen):
    name = "theses"
    BINDINGS = [
        Binding("n", "new_thesis", "new", tooltip="Create a thesis"),
        Binding("d", "edit_thesis", "edit", tooltip="Edit the claim and its framing"),
        Binding("f", "find_evidence", "find", tooltip="Ask the model for candidate evidence"),
        Binding("s", "summarise", "summarise", tooltip="Summarise the accepted evidence"),
        Binding("t", "focus_thesis", "thesis", tooltip="Open the thesis"),
        Binding("e", "focus_evidence", "evidence", tooltip="Open the evidence ledger"),
        Binding("a", "accept", "accept", tooltip="Accept the highlighted candidate"),
        Binding("x", "reject", "reject", tooltip="Drop the highlighted candidate"),
        Binding("u", "unaccept", "un-accept", tooltip="Return accepted evidence to pending"),
        Binding("slash", "filter_claims", "filter", tooltip="Filter the claims list"),
        Binding("escape", "back", "back", show=False),
        # Offered in the ledger's hints only when a note is taller than its box.
        Binding("shift+down", "scroll_note_down", "scroll note", show=False),
        Binding("shift+up", "scroll_note_up", "scroll note", show=False),
    ]

    #: Below this terminal width the three panes no longer fit side by side:
    #: the claims list takes the whole width and the thesis and the ledger
    #: open full-width on demand (enter / e), esc stepping back.
    NARROW_WIDTH = 100

    CSS = """
    #thesis-split { height: 1fr; }
    #thesis-claims { width: 36; }
    #thesis-detail-pane { width: 1fr; min-width: 0; }
    #thesis-evidence-pane { width: 40; }
    Theses.-narrow #thesis-claims,
    Theses.-narrow #thesis-detail-pane,
    Theses.-narrow #thesis-evidence-pane { width: 1fr; }
    #thesis-filter { height: 1; margin: 0; }
    #thesis-table { height: 1fr; }
    #thesis-list-foot { height: 1; padding: 0 1; color: $text-muted; }
    #thesis-detail { height: 1fr; padding: 0 1; }
    #thesis-detail > Static { height: auto; }
    #thesis-counts { height: 1; padding: 0 1; }
    #thesis-ledger { height: 1fr; min-height: 4; }
    /* The read-out sits under a rule in the same pane: the ledger's note
       column is a title, this is where the note and citation are read. */
    #thesis-preview { height: 8; border-top: solid $border-blurred; padding: 0 1; }
    #thesis-preview-text { height: auto; }
    .thesis-claim { text-style: bold; }
    .thesis-gap { height: 1; }
    .thesis-line { height: 1; text-wrap: nowrap; text-overflow: ellipsis; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self.selected: str | None = None
        self.candidates: list[theses.ThesisEvidence] = []
        self._summary: thesis_summary.ThesisSummary | None = None
        self._result: HealthResult | None = None
        self._narrow = False
        #: Narrow only: which pane fills the screen. Wide shows all three.
        self._view = "claims"
        self._rows: dict[str, theses.ThesisEvidence] = {}
        self._cites: dict[str, str] = {}
        self._ages: dict[str, str] = {}
        self._links: list[theses.ThesisEvidence] = []
        self._theses: list[theses.Thesis] = []
        self._health: dict[str, HealthResult | None] = {}
        self._pending: dict[str, int] = {}
        self._overflowing = False
        self._summarising = False
        self._finding = False
        self._widths: tuple[bool, int, int] | None = None
        self._render_lock = asyncio.Lock()

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="thesis-split"):
            with Pane(title="theses", id="thesis-claims"):
                yield Input(placeholder="/ filter claims", id="thesis-filter")
                yield RiggerTable(id="thesis-table")
                yield Static("", id="thesis-list-foot", markup=False)
            with HealthPane(title="thesis", key="t", id="thesis-detail-pane"):
                yield VerticalScroll(id="thesis-detail")
            with Pane(title="evidence", key="e", id="thesis-evidence-pane"):
                yield Static("", id="thesis-counts", markup=False)
                yield RiggerTable(id="thesis-ledger")
                # Not focusable: tab means "next pane", and a read-only note
                # viewer is not a pane. It scrolls from the ledger instead.
                with VerticalScroll(id="thesis-preview", can_focus=False):
                    yield Static(
                        "select evidence to read its note and citation",
                        id="thesis-preview-text",
                        markup=False,
                    )

    async def on_mount(self) -> None:
        self.query_one("#thesis-filter").display = False
        self.layout_views()
        self._sync_columns()
        await self.refresh_view()
        self.query_one("#thesis-table").focus()

    def on_resize(self) -> None:
        if self.is_mounted:
            self.layout_views()

    # ---------- layout ----------

    def layout_views(self) -> None:
        """Wide: three panes. Narrow: one pane at a time, ``self._view``."""
        self._narrow = self.size.width < self.NARROW_WIDTH
        self.set_class(self._narrow, "-narrow")
        for pane, view in (
            ("#thesis-claims", "claims"),
            ("#thesis-detail-pane", "detail"),
            ("#thesis-evidence-pane", "evidence"),
        ):
            self.query_one(pane).display = not self._narrow or self._view == view
        self._paint_hints()
        # Showing a pane does not resize the screen, so the column widths are
        # re-measured once the new layout has been applied rather than now,
        # when the pane that was just revealed still reports its old size.
        self.call_after_refresh(self._resync_columns)

    def _paint_hints(self) -> None:
        """Every pane's bottom border: fixed keys plus what narrow adds."""
        claims = [("n", "new"), ("d", "edit"), ("/", "filter")]
        detail = [("s", "summarise"), ("d", "edit"), ("↑↓", "scroll")]
        if self._narrow:
            claims += [("enter", "thesis"), ("e", "evidence")]
            detail.append(("esc", "back"))
        self.query_one("#thesis-claims", Pane).set_hints(hint_markup(*claims))
        self.query_one("#thesis-detail-pane", Pane).set_hints(hint_markup(*detail))
        self._render_actions(self._current_row())

    def _resync_columns(self) -> None:
        if self._sync_columns():
            self.refresh_list()
            self._fill_ledger()

    def _sync_columns(self) -> bool:
        """Size the flexible columns to the panes; rebuild them only on change.

        A ``DataTable`` column cannot be ``1fr``, so the widths are recomputed
        and the columns re-added when the terminal is resized. The narrow
        list adds health, tilt and queue columns the 36-column list has no
        room for. Returns whether anything changed, so the caller knows to
        repopulate the rows.
        """
        # A DataTable pads every cell by one column each side.
        claims_inner = self.query_one("#thesis-claims").size.width or CLAIMS_WIDTH - 2
        claim_width = claims_inner - 2
        if self._narrow:
            claim_width -= (LIST_HEALTH_WIDTH + 2) + (LIST_TILT_WIDTH + 2) + (LIST_QUEUE_WIDTH + 2)
        claim_width = max(12, claim_width)
        evidence_inner = self.query_one("#thesis-evidence-pane").size.width or EVIDENCE_WIDTH - 2
        note_width = max(12, evidence_inner - (1 + 2) - (1 + 2) - (4 + 2) - 2)
        if self._widths == (self._narrow, claim_width, note_width):
            return False
        self._widths = (self._narrow, claim_width, note_width)
        table = self.query_one("#thesis-table", RiggerTable)
        table.clear(columns=True)
        table.show_header = self._narrow
        table.add_column("  Claim", width=claim_width)
        if self._narrow:
            table.add_column("Health", width=LIST_HEALTH_WIDTH)
            table.add_column(Text("Tilt", justify="right"), width=LIST_TILT_WIDTH)
            table.add_column(Text("Queue", justify="right"), width=LIST_QUEUE_WIDTH)
        ledger = self.query_one("#thesis-ledger", RiggerTable)
        ledger.clear(columns=True)
        ledger.add_column("", width=1)
        ledger.add_column("±", width=1)
        ledger.add_column("age", width=4)
        ledger.add_column("note", width=note_width)
        return True

    def _colours(self) -> dict[str, str]:
        """The app's cached theme tokens.

        ``theme_variables`` is the same mapping Textual builds for CSS and
        refreshes on a theme change, so this costs a dict lookup; generating
        the colour system here instead rebuilt 168 tokens on every call.
        """
        return self.app.theme_variables

    def _style(self, token: str) -> str:
        """A theme token as a Rich style, for ``Text`` spans.

        Under the Rigger theme every ``text-*`` token is a hex colour. Under a
        bare App (the tests) ``text-muted`` is Textual's ``auto 60%`` blend,
        which Rich cannot parse, so it degrades to ``dim`` rather than crash.
        """
        value = self._colours().get(token, "")
        try:
            Style.parse(value)
        except StyleSyntaxError:
            return "dim" if token == "text-muted" else ""
        return value

    # ---------- navigation ----------

    def _open(self, view: str) -> None:
        """Narrow: fill the screen with ``view``. Wide: just focus its pane."""
        if self._narrow and self._view != view:
            self._view = view
            self.layout_views()
        target = {
            "claims": "#thesis-table",
            "detail": "#thesis-detail",
            "evidence": "#thesis-ledger",
        }
        self.query_one(target[view]).focus()

    def action_focus_thesis(self) -> None:
        self._open("detail")

    def action_focus_evidence(self) -> None:
        self._open("evidence")

    def action_filter_claims(self) -> None:
        if self._narrow and self._view != "claims":
            self._open("claims")
        self.query_one("#thesis-filter").display = True
        self.query_one("#thesis-filter").focus()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "thesis-filter":
            self.refresh_list()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "thesis-filter":
            self.query_one("#thesis-table").focus()

    def action_back(self) -> None:
        """Back out one step: a running job, then the filter, then the pane, then focus."""
        if self._summarising or self._finding:
            self.workers.cancel_group(self, "thesis-summary")
            self.workers.cancel_group(self, "thesis-discover")
            self._summarising = False
            self._finding = False
            self._paint_health_badge()
            self._paint_ledger_badge()
            self.notify("cancelled")
            return
        filter_input = self.query_one("#thesis-filter", Input)
        if filter_input.display:
            filter_input.value = ""
            filter_input.display = False
            self.query_one("#thesis-table").focus()
            return
        if self._narrow and self._view != "claims":
            # Whether the ledger was reached from the thesis or from the
            # list, back is the list: it is where the next thesis is.
            self._open("claims")
            return
        self.query_one("#thesis-table").focus()

    # ---------- claims list ----------

    async def refresh_view(self) -> None:
        self.reload_theses()
        await self.render_detail()

    def reload_theses(self) -> None:
        """Re-read the claims and their health, then redraw the list.

        The only place that queries: filtering and resizing redraw from this
        cache, so neither a keystroke in the filter box nor a column of a drag
        resize costs a query.
        """
        self._theses = theses.list_theses(self.rig.engine)
        now = datetime.now(UTC)
        self._health = {}
        self._pending = {}
        for thesis in self._theses:
            links = theses.evidence_for(self.rig.engine, thesis.id, accepted_only=False)
            self._pending[thesis.id] = sum(1 for link in links if not link.accepted)
            accepted_links = [link for link in links if link.accepted]
            if not accepted_links:
                self._health[thesis.id] = None
                continue
            by_id = {
                item.id: item
                for item in evidence_by_ids(
                    self.rig.engine, [link.evidence_id for link in accepted_links]
                )
            }
            accepted = [
                (by_id[link.evidence_id], link.side)
                for link in accepted_links
                if link.evidence_id in by_id
            ]
            self._health[thesis.id] = (
                compute_health(thesis, accepted, now=now) if accepted else None
            )
        self.refresh_list()

    def _health_state(self, thesis: theses.Thesis) -> str:
        """The list glyph's key: a status when it overrides, else the health state."""
        if thesis.status == "concluded":
            return "concluded"
        result = self._health.get(thesis.id)
        return result.state if result else "emerging"

    def refresh_list(self) -> None:
        table = self.query_one("#thesis-table", RiggerTable)
        query = self.query_one("#thesis-filter", Input).value.casefold().strip()
        rows = (
            [row for row in self._theses if query in row.claim.casefold()]
            if query
            else self._theses
        )
        index = {row.id: position for position, row in enumerate(rows)}
        muted = self._style("text-muted")
        with self.prevent(RiggerTable.RowHighlighted, RiggerTable.RowSelected):
            table.clear()
            if self.selected not in index:
                self.selected = rows[0].id if rows else None
                self._summary = None
            for row in rows:
                state = self._health_state(row)
                glyph, token = HEALTH_MARKS[state]
                dim = state == "concluded"
                # Built span by span: a style on the constructor would be the
                # base style and colour the claim text too.
                cell = Text(no_wrap=True, overflow="ellipsis")
                cell.append(glyph, style=self._style(token))
                cell.append(" " + row.claim, style=muted if dim else "")
                if not self._narrow:
                    table.add_row(cell, key=row.id)
                    continue
                result = self._health.get(row.id)
                tilt = f"{result.tilt:+.2f}" if result and state != "emerging" else ""
                tilt_token = "text-muted"
                if not dim and state not in ("idle", "emerging"):
                    tilt_token = "text-success" if result and result.tilt >= 0 else "text-error"
                pending = self._pending.get(row.id, 0)
                table.add_row(
                    cell,
                    Text(state, style=muted),
                    Text(tilt, style=self._style(tilt_token), justify="right"),
                    Text(
                        f"{pending} pending" if pending else "",
                        style=self._style("text-warning"),
                        justify="right",
                    ),
                    key=row.id,
                )
            if self.selected is not None:
                table.move_cursor(row=index[self.selected])
        pending_total = sum(self._pending.get(row.id, 0) for row in rows)
        badge = str(len(rows))
        if pending_total:
            badge += f" · {pending_total} pending"
        self.query_one("#thesis-claims", Pane).set_badge(badge)
        concluded = sum(1 for row in rows if row.status == "concluded")
        foot = f"{len(rows)} thes{'is' if len(rows) == 1 else 'es'}"
        if concluded:
            foot += f" · {concluded} concluded"
        if query:
            foot += f" · of {len(self._theses)}"
        # Narrow shows the list alone, so the thesis pane's empty line is not
        # on screen: the foot says it instead, rather than nothing at all.
        empty = "no theses yet — press n to create one" if self._narrow else ""
        self.query_one("#thesis-list-foot", Static).update(foot if rows else empty)

    async def on_data_table_row_selected(self, event: RiggerTable.RowSelected) -> None:
        if event.data_table.id == "thesis-table" and event.row_key.value is not None:
            self.selected = str(event.row_key.value)
            self._summary = None
            await self.render_detail()
            if self._narrow:
                self._open("detail")

    def on_data_table_row_highlighted(self, event: RiggerTable.RowHighlighted) -> None:
        if event.data_table.id == "thesis-ledger":
            self._preview()

    # ---------- create / edit ----------

    def action_new_thesis(self) -> None:
        self.app.push_screen(ThesisForm(), self._save_thesis)

    def action_edit_thesis(self) -> None:
        if self.selected is None:
            self.notify("select a thesis first", severity="error")
            return
        self.app.push_screen(
            ThesisForm(theses.get_thesis(self.rig.engine, self.selected)), self._save_thesis
        )

    async def _save_thesis(self, fields: dict[str, Any] | None) -> None:
        """Create or update, depending on whether a thesis was being edited.

        The dialog hands back the keywords both service calls already take, so
        neither the order nor the arity has to be restated here.
        """
        if fields is None:
            return
        editing = "status" in fields
        try:
            if editing:
                if self.selected is None:
                    return
                thesis = theses.update_thesis(self.rig.engine, self.selected, **fields)
            else:
                claim = fields.pop("claim")
                thesis = theses.create_thesis(self.rig.engine, claim, **fields)
        except (KeyError, ValueError) as exc:
            self.notify(str(exc), severity="error")
            return
        self.selected = thesis.id
        self._summary = None
        await self.refresh_view()

    # ---------- long-running jobs ----------

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
        self.query_one("#thesis-detail-pane", HealthPane).set_badge("summarising…")
        try:
            summary = await thesis_summary.summarize_thesis(self.rig, selected)
            if self.selected == selected:
                self._summary = summary
                await self.render_detail()
        except Exception as exc:
            self.notify(f"summary failed: {exc} — press s to try again", severity="error")
        finally:
            self._summarising = False
            self._paint_health_badge()

    @work(exclusive=True, group="thesis-discover")
    async def action_find_evidence(self) -> None:
        await self._find_evidence()

    async def _find_evidence(self) -> None:
        """Ask the model for candidate evidence; it is stored unaccepted."""
        if self.selected is None:
            self.notify("select a thesis first", severity="error")
            return
        if self._finding:
            return
        selected = self.selected
        self._finding = True
        self.query_one("#thesis-evidence-pane", Pane).set_badge("finding…")
        try:
            found = await theses.propose_evidence(self.rig, selected)
            if self.selected != selected:
                return
            self.reload_theses()
            await self.render_detail()
            self.notify(
                f"{len(found)} candidate{'' if len(found) == 1 else 's'} to review"
                if found
                else "no new evidence found"
            )
            if found and (not self._narrow or self._view == "evidence"):
                self.query_one("#thesis-ledger").focus()
        except Exception as exc:
            self.notify(f"could not find evidence: {exc} — press f to try again", severity="error")
        finally:
            self._finding = False
            self._paint_ledger_badge()

    # ---------- evidence review ----------

    def _current_row(self) -> theses.ThesisEvidence | None:
        table = self.query_one("#thesis-ledger", RiggerTable)
        if not table.row_count:
            return None
        return self._rows.get(
            str(table.coordinate_to_cell_key(table.cursor_coordinate).row_key.value)
        )

    def _preview(self) -> None:
        row = self._current_row()
        text: Text | str = "no evidence yet — press f to find candidates"
        if row:
            _glyph, token, side = SIDE_MARKS[row.side]
            text = Text()
            if row.accepted:
                text.append("accepted", style=f"bold {self._style('text-success')}")
            else:
                text.append("pending review", style=f"bold {self._style('text-warning')}")
            text.append(" · ", style=self._style("text-muted"))
            text.append(side, style=self._style(token))
            text.append(f"\n{row.note}\n\n")
            text.append(
                self._cites.get(row.evidence_id, row.evidence_id), style=self._style("text-muted")
            )
        self.query_one("#thesis-preview-text", Static).update(text)
        # A new note starts at its top, not wherever the last one was left.
        self.query_one("#thesis-preview", VerticalScroll).scroll_home(animate=False)
        self._render_actions(row)
        # Whether the note overflows is only known once the new text has been
        # laid out, so the hints are checked again then — and re-painted only
        # if that answer changed, rather than unconditionally drawing twice.
        self.call_after_refresh(self._rerender_actions_if_overflow_changed, row)

    def _rerender_actions_if_overflow_changed(self, row: theses.ThesisEvidence | None) -> None:
        if self._overflowing != self._preview_overflows():
            self._render_actions(row)

    def _render_actions(self, row: theses.ThesisEvidence | None) -> None:
        """The ledger's hints say what applies to the highlighted row.

        Contextual rather than greyed: ``a``/``x`` on a pending row, ``u`` on
        an accepted one (accepted evidence is removed in two steps — un-accept,
        then reject — so one stray keystroke cannot delete curated evidence),
        ``f`` always, and the note-scroll keys only when the note overflows.
        """
        if row is None:
            wanted: tuple[str, ...] = ()
        elif row.accepted:
            wanted = ("u",)
        else:
            wanted = ("a", "x")
        # Labels come from BINDINGS rather than a second list: two spellings
        # of one key drift.
        by_key = {b.key: b for b in self.BINDINGS if isinstance(b, Binding)}
        pairs = [(key, by_key[key].description) for key in wanted if key in by_key]
        pairs.append(("f", by_key["f"].description))
        self._overflowing = self._preview_overflows()
        if self._overflowing:
            # One glyph for both arrows: four hints have to fit a 40-column border.
            pairs.append(("⇧↕", "note"))
        if self._narrow:
            pairs.append(("esc", "back"))
        self.query_one("#thesis-evidence-pane", Pane).set_hints(hint_markup(*pairs))

    def _preview_overflows(self) -> bool:
        """Whether the note is taller than the box, so scrolling is worth offering."""
        preview = self.query_one("#thesis-preview", VerticalScroll)
        return preview.max_scroll_y > 0

    def action_scroll_note_down(self) -> None:
        self._scroll_note(1)

    def action_scroll_note_up(self) -> None:
        self._scroll_note(-1)

    def _scroll_note(self, direction: int) -> None:
        """Scroll the note under the ledger cursor without leaving the ledger."""
        if not self.query_one("#thesis-ledger").has_focus:
            return
        self.query_one("#thesis-preview", VerticalScroll).scroll_relative(
            y=direction, animate=False
        )

    async def action_accept(self) -> None:
        await self._resolve("accept")

    async def action_reject(self) -> None:
        await self._resolve("reject")

    async def action_unaccept(self) -> None:
        await self._resolve("unaccept")

    async def _resolve(self, action: str) -> None:
        # Hidden evidence must not be modified by a stray shortcut.
        if self._narrow and self._view != "evidence":
            self.notify("press e to review evidence first")
            return
        row = self._current_row()
        if row is None:
            self.notify("no evidence to review")
            return
        if action == "unaccept":
            if not row.accepted:
                self.notify("that item is already pending")
                return
            theses.set_accepted(self.rig.engine, row.thesis_id, row.evidence_id, False)
        else:
            if row.accepted:
                self.notify("already accepted — press u to return it to pending")
                return
            if action == "accept":
                theses.set_accepted(self.rig.engine, row.thesis_id, row.evidence_id, True)
            else:
                theses.remove_evidence(self.rig.engine, row.thesis_id, row.evidence_id)
        self._summary = None
        # Health and the queue count changed for this claim, so the list
        # redraws too — its glyph and badge come from the same read.
        self.reload_theses()
        await self.render_detail()
        self.query_one("#thesis-ledger").focus()

    # ---------- detail ----------

    async def render_detail(self) -> None:
        async with self._render_lock:
            await self._render_detail()

    async def _render_detail(self) -> None:
        detail = self.query_one("#thesis-detail", VerticalScroll)
        offset = detail.scroll_offset.y
        await detail.remove_children()
        self._rows = {}
        self._cites = {}
        self._ages = {}
        self._links = []
        self.candidates = []
        self._result = None
        if self.selected is None:
            await detail.mount(Static("no theses yet — press n to create one", markup=False))
            self._fill_ledger()
            self.query_one("#thesis-counts", Static).update("no evidence yet")
            self._paint_health_badge()
            self._paint_ledger_badge()
            return
        thesis = theses.get_thesis(self.rig.engine, self.selected)
        self._links = theses.evidence_for(self.rig.engine, thesis.id, accepted_only=False)
        links = self._links
        self.candidates = [r for r in links if not r.accepted]
        items = evidence_by_ids(self.rig.engine, [r.evidence_id for r in links])
        by_id = {item.id: item for item in items}
        self._cites = {item.id: cite(item) for item in items}
        now = datetime.now(UTC)
        self._ages = {item.id: age_text(now - to_utc(item.ts))[0] for item in items}
        accepted = [
            (by_id[r.evidence_id], r.side) for r in links if r.accepted and r.evidence_id in by_id
        ]
        result = compute_health(thesis, accepted, now=now) if accepted else None
        self._result = result
        await detail.mount(*self._detail_widgets(thesis, result, by_id, now, len(accepted)))
        self._fill_ledger()
        # The per-side counts are read off the health result rather than
        # regrouped here, so the legend and the badge cannot disagree.
        counts = (result.support, result.against, result.neutral) if result else (0, 0, 0)
        legend = Text(no_wrap=True, overflow="ellipsis")
        for (glyph, token, _side), count, word in zip(
            SIDE_MARKS.values(), counts, ("support", "against", "neutral"), strict=True
        ):
            if legend:
                legend.append("  ")
            legend.append(f"{glyph}{count}", style=self._style(token))
            legend.append(f" {word}", style=self._style("text-muted"))
        self.query_one("#thesis-counts", Static).update(legend)
        self._paint_health_badge()
        self._paint_ledger_badge()
        detail.scroll_to(y=offset, animate=False)

    def _paint_health_badge(self) -> None:
        pane = self.query_one("#thesis-detail-pane", HealthPane)
        if self.selected is None:
            pane.set_badge("")
            return
        state = self._result.state if self._result else "emerging"
        glyph, token = HEALTH_MARKS[state]
        pane.set_badge(f"{glyph} {state}", token)

    def _paint_ledger_badge(self) -> None:
        pane = self.query_one("#thesis-evidence-pane", Pane)
        if self.selected is None:
            pane.set_badge("0")
            return
        pane.set_badge(f"{len(self.candidates)} pending / {len(self._links)}")

    def _field(self, label: str, value: str) -> Static:
        text = Text(f"{label:<10}", style=self._style("text-muted"))
        text.append(value)
        return Static(text)

    def _detail_widgets(
        self,
        thesis: theses.Thesis,
        result: HealthResult | None,
        by_id: dict[str, EvidenceItem],
        now: datetime,
        accepted_count: int,
    ) -> list[Widget]:
        muted = self._style("text-muted")
        state = result.state if result else "emerging"
        glyph, token = HEALTH_MARKS[state]
        health = Text(f" {glyph} {state}", style=f"bold {self._style(token)}")
        if result is not None:
            health.append(f" · tilt {result.tilt:+.2f}", style=self._style(token))
        health.append(" ")
        health.stylize(f"on {self._style('panel')}")
        health.append(
            f"  {accepted_count} accepted" if accepted_count else "  no accepted evidence",
            style=muted,
        )
        meta = f"{thesis.status}"
        if thesis.time_horizon:
            meta += f" · horizon {thesis.time_horizon}"
        meta += f" · opened {thesis.created_at:%d %b %Y}"
        widgets: list[Widget] = [
            Static(thesis.claim, classes="thesis-claim", markup=False),
            Static("", classes="thesis-gap"),
            Static(health, id="thesis-health"),
            Static(Text(meta, style=muted)),
        ]
        if result is not None and result.drivers:
            names = [
                getattr(by_id.get(driver), "title", None) or driver for driver in result.drivers
            ]
            shown = ", ".join(names[:2])
            if len(names) > 2:
                shown += f", +{len(names) - 2} more"
            # ``Static`` re-wraps a Rich ``Text`` regardless of ``no_wrap``,
            # so the one-line rule lives in CSS (``.thesis-line``).
            moved = Text("moved by  ", style=muted)
            moved.append(shown)
            widgets.append(Static(moved, classes="thesis-line"))
        widgets.append(Static("", classes="thesis-gap"))
        widgets.append(self._field("targets", ", ".join(thesis.targets) or "all evidence"))
        for label, value in (
            ("scope", thesis.scope),
            ("holds if", "; ".join(thesis.assumptions)),
            ("breaks if", "; ".join(thesis.falsifiers)),
        ):
            if value:
                widgets.append(self._field(label, value))
        if not thesis.falsifiers:
            widgets.append(
                Static(
                    Text(
                        "breaks if  not set — press d to say what would disprove it",
                        style=muted,
                    )
                )
            )
        widgets.append(Static("", classes="thesis-gap"))
        widgets.extend(self._summary_widgets(by_id, now))
        return widgets

    def _fill_ledger(self) -> None:
        """Repopulate the ledger from the current links, keeping the cursor row."""
        table = self.query_one("#thesis-ledger", RiggerTable)
        cursor = table.cursor_row
        muted = self._style("text-muted")
        with self.prevent(RiggerTable.RowHighlighted, RiggerTable.RowSelected):
            table.clear()
            self._rows = {}
            for row in sorted(self._links, key=lambda r: r.accepted):
                self._rows[row.evidence_id] = row
                symbol, token, _label = SIDE_MARKS[row.side]
                note = Text(row.note or row.evidence_id, no_wrap=True, overflow="ellipsis")
                if not row.accepted:
                    note.stylize("bold")
                table.add_row(
                    Text("✓", style=self._style("text-success"))
                    if row.accepted
                    else Text("•", style=self._style("text-warning")),
                    Text(symbol, style=self._style(token)),
                    Text(self._ages.get(row.evidence_id, "—"), style=muted),
                    note,
                    key=row.evidence_id,
                )
            if table.row_count:
                table.move_cursor(row=min(cursor, table.row_count - 1))
        self._preview()

    def _summary_widgets(self, by_id: dict[str, EvidenceItem], now: datetime) -> list[Widget]:
        muted = self._style("text-muted")
        heading = Text("summary", style=f"bold {muted}")
        if self._summary is None:
            return [
                Static(heading),
                Static(Text("no summary yet — press s to summarise", style=muted)),
            ]
        as_of = to_utc(self._summary.as_of)
        newest = max(
            (to_utc(item.ts) for item in by_id.values()),
            default=None,
        )
        stale = newest is not None and newest > as_of
        age = age_text(now - as_of)[0]
        heading.append(
            f"          {age} old · {len(self._summary.citations)} citations", style=muted
        )
        widgets: list[Widget] = [
            Static(heading),
            Static(Text(self._summary.summary, style=muted if stale else "")),
        ]
        strongest = [
            ("support", "text-success", self._summary.strongest_support),
            ("counter", "text-error", self._summary.strongest_counter),
        ] + [("unknown", "text-warning", unknown) for unknown in self._summary.unknowns]
        rows = [(label, token, value) for label, token, value in strongest if value]
        if rows:
            widgets.append(Static("", classes="thesis-gap"))
        for label, token, value in rows:
            line = Text(f"{label:<8} ", style=self._style(token))
            line.append(value, style=muted)
            widgets.append(Static(line))
        if stale:
            widgets.append(Static("", classes="thesis-gap"))
            note = Text("summary is older than the newest accepted evidence — press ", style=muted)
            note.append("s", style=f"bold {self._style('text-primary')}")
            note.append(" to refresh it", style=muted)
            widgets.append(Static(note))
        return widgets
