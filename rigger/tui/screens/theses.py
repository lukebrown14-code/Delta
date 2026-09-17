"""Keyboard-driven research desk, with a compact evidence review ledger."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime
from typing import Any

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, VerticalScroll
from textual.widget import Widget
from textual.widgets import Button, Input, Select, Static

from rigger import theses, thesis_summary
from rigger.core.time import to_utc
from rigger.evidence import EvidenceItem, cite, evidence_by_ids
from rigger.thesis_health import HealthResult, badge_text, compute_health
from rigger.tui.shell import RiggerScreen, age_text
from rigger.tui.widgets import (
    Dialog,
    KeyStrip,
    Pane,
    PaneRow,
    Pill,
    RiggerTable,
    health_variant,
    shown_bindings,
)

#: Per side: ledger glyph, the theme token that colours it, and the word the
#: preview uses. One table so the ledger, the legend and the preview cannot
#: disagree about what a side is called.
SIDE_MARKS: dict[str, tuple[str, str, str]] = {
    "support": ("+", "text-success", "Supporting"),
    "against": ("−", "text-error", "Against"),
    "neutral": ("?", "text-warning", "Neutral"),
}

#: Claims-list status marker: glyph plus the theme token that colours it.
#: Shape carries the meaning as well as colour, so the three states stay
#: distinguishable without relying on hue. Every glyph is verified single-cell
#: so the claim text stays aligned down the column.
STATUS_MARKS: dict[str, tuple[str, str]] = {
    "active": ("●", "text-success"),
    "paused": ("◐", "text-warning"),
    "concluded": ("○", "foreground"),
}

#: Pane widths. The claims and detail panes share the space left over in 2:3,
#: so a wide terminal lengthens the claim column instead of padding the prose;
#: the ledger is a fixed-shape table, so it keeps a fixed width. ``Theses.CSS``
#: repeats these numbers because Textual CSS cannot read them —
#: ``test_pane_width_constants_match_the_stylesheet`` holds the two in step.
CLAIMS_MIN_WIDTH = 24
EVIDENCE_WIDTH = 42
DETAIL_MIN_WIDTH = 34


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

    dialog_hint = "tab next field · enter save · esc cancel"

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


class Theses(RiggerScreen):
    name = "theses"
    BINDINGS = [
        Binding("n", "new_thesis", "new", tooltip="Create a thesis"),
        Binding("d", "edit_thesis", "edit", tooltip="Edit the claim and its framing"),
        Binding("f", "find_evidence", "find", tooltip="Ask the model for candidate evidence"),
        Binding("s", "summarise", "summary", tooltip="Summarise the accepted evidence"),
        Binding("e", "toggle_evidence", "evidence", tooltip="Move between detail and evidence"),
        Binding("a", "accept", "accept", tooltip="Accept the highlighted candidate"),
        Binding("x", "reject", "reject", tooltip="Drop the highlighted candidate"),
        Binding("u", "unaccept", "un-accept", tooltip="Return accepted evidence to pending"),
        Binding("slash", "filter_claims", "filter", tooltip="Filter the claims list"),
        Binding("escape", "back", "back", show=False),
        # Offered in the evidence pane only when a note is taller than its box,
        # so they stay out of the key strip.
        Binding("shift+down", "scroll_note_down", "scroll note", show=False),
        Binding("shift+up", "scroll_note_up", "scroll note", show=False),
    ]

    #: The width at which all three panes fit side by side: both side panes,
    #: the detail pane's minimum, plus the screen padding and pane gutters.
    #: Derived rather than guessed so it cannot drift from the CSS, and it
    #: stays well above ``PaneRow.NARROW_WIDTH`` — the screen therefore never
    #: claims three columns while the row beneath it is stacking them.
    WIDE_WIDTH = CLAIMS_MIN_WIDTH + EVIDENCE_WIDTH + DETAIL_MIN_WIDTH + 4

    CSS = """
    #thesis-split { height: 1fr; }
    #thesis-claims { width: 2fr; min-width: 24; }
    #thesis-filter { height: 3; }
    #thesis-table { height: 1fr; }
    #thesis-detail-pane { width: 3fr; min-width: 34; }
    #thesis-evidence-pane { width: 42; }
    #thesis-detail { height: 1fr; }
    #thesis-ledger { height: 1fr; min-height: 4; }
    /* The same separator PaneStack draws between stacked panes; the children
       here are a table and a scroller rather than Panes, so the rule is
       applied directly instead. */
    #thesis-preview { height: 8; border-top: solid $panel; padding-top: 1; }
    #thesis-preview-text { height: auto; }
    #thesis-counts { height: auto; color: $text-muted; margin-bottom: 1; }
    .thesis-claim { text-style: bold; margin-bottom: 1; height: auto; }
    .thesis-meta { height: auto; margin-bottom: 1; }
    .thesis-meta Pill { width: auto; height: 1; margin-right: 1; }
    #thesis-health { width: auto; height: 1; }
    .thesis-health-row { height: auto; margin-bottom: 1; }
    .thesis-drivers { height: auto; color: $text-muted; margin-bottom: 1; }
    .thesis-field { height: auto; }
    /* Section rules are structure, not action: $primary stays reserved for
       keys and the primary action, as on the research screen. */
    .thesis-section { color: $text-muted; text-style: bold; margin-top: 1; height: 1; }
    #thesis-actions { height: 1; color: $text-muted; }
    /* Compact shows the claims list beside ONE content pane, so the list
       takes a fixed slice and whichever pane is showing takes the rest —
       sharing by fr would leave the content pane narrower than the list. */
    Theses.-compact #thesis-claims { width: 26; }
    Theses.-compact #thesis-detail-pane { width: 1fr; }
    Theses.-compact #thesis-evidence-pane { width: 1fr; }
    /* Last word on width: when the row stacks there are no columns to share,
       so every pane takes the full width. rigger.tcss carries this rule too,
       with !important, and that copy wins wherever the app stylesheet is
       loaded. This one is for the standalone case — the panel screens are
       built to mount under any App, and under a bare one the widths above
       would otherwise multiply out past the row. It sits after the compact
       rules because it is of equal specificity and has to be the last word. */
    #thesis-split.-narrow > Pane { width: 1fr; min-width: 0; }
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
        self._ages: dict[str, str] = {}
        self._links: list[theses.ThesisEvidence] = []
        self._theses: list[theses.Thesis] = []
        self._overflowing = False
        self._summarising = False
        self._finding = False
        self._widths: tuple[int, int] | None = None
        self._render_lock = asyncio.Lock()

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="thesis-split"):
            with Pane(title="theses", id="thesis-claims"):
                yield Input(placeholder="filter claims", id="thesis-filter")
                yield RiggerTable(id="thesis-table")
            with Pane(title="thesis / detail", id="thesis-detail-pane"):
                yield VerticalScroll(id="thesis-detail")
            with Pane(title="evidence", id="thesis-evidence-pane"):
                yield Static("", id="thesis-counts", markup=False)
                yield RiggerTable(id="thesis-ledger")
                # Not focusable: tab means "next pane", and a read-only note
                # viewer is not a pane. It scrolls from the ledger instead.
                with VerticalScroll(id="thesis-preview", can_focus=False):
                    yield Static(
                        "Select evidence to read its note and citation.",
                        id="thesis-preview-text",
                        markup=False,
                    )
                yield Static("", id="thesis-actions", markup=False)
        yield KeyStrip(self.BINDINGS, id="thesis-keys")

    async def on_mount(self) -> None:
        self.query_one("#thesis-filter").display = False
        self._layout()
        self._sync_columns()
        await self.refresh_view()
        self.query_one("#thesis-table").focus()

    def on_resize(self) -> None:
        if self.is_mounted:
            self._layout()

    # ---------- layout ----------

    def _layout(self) -> None:
        self._wide = self.size.width >= self.WIDE_WIDTH
        self.set_class(not self._wide, "-compact")
        self.query_one("#thesis-detail-pane").display = self._wide or not self._show_evidence
        self.query_one("#thesis-evidence-pane").display = self._wide or self._show_evidence
        # Showing a pane does not resize the screen, so the column widths are
        # re-measured once the new layout has been applied rather than now,
        # when the pane that was just revealed still reports its old size.
        self.call_after_refresh(self._resync_columns)

    def _resync_columns(self) -> None:
        if self._sync_columns():
            self.refresh_list()
            self._fill_ledger()

    def _sync_columns(self) -> bool:
        """Size the flexible columns to the panes; rebuild them only on change.

        A ``DataTable`` column cannot be ``1fr``, so the widths are recomputed
        and the columns re-added when the terminal is resized. Returns whether
        anything changed, so the caller knows to repopulate the rows.
        """
        claim_width = max(12, self.query_one("#thesis-claims").size.width - 6)
        evidence_pane = self.query_one("#thesis-evidence-pane")
        evidence_width = evidence_pane.size.width or EVIDENCE_WIDTH
        note_width = max(12, evidence_width - 18)
        if self._widths == (claim_width, note_width):
            return False
        self._widths = (claim_width, note_width)
        table = self.query_one("#thesis-table", RiggerTable)
        table.clear(columns=True)
        table.add_column("Claim", width=claim_width)
        ledger = self.query_one("#thesis-ledger", RiggerTable)
        ledger.clear(columns=True)
        ledger.add_column("", width=1)
        ledger.add_column("±", width=1)
        ledger.add_column("age", width=5)
        ledger.add_column("note", width=note_width)
        return True

    def _colours(self) -> dict[str, str]:
        """The app's cached theme tokens.

        ``theme_variables`` is the same mapping Textual builds for CSS and
        refreshes on a theme change, so this costs a dict lookup; generating
        the colour system here instead rebuilt 168 tokens on every call.
        """
        return self.app.theme_variables

    # ---------- navigation ----------

    def action_toggle_evidence(self) -> None:
        """Compact: swap the visible pane. Wide: alternate focus between them."""
        if self._wide:
            on_ledger = self.query_one("#thesis-ledger").has_focus
            self.query_one("#thesis-detail" if on_ledger else "#thesis-ledger").focus()
            return
        self._show_evidence = not self._show_evidence
        self._layout()
        self.query_one("#thesis-ledger" if self._show_evidence else "#thesis-detail").focus()

    def action_filter_claims(self) -> None:
        self.query_one("#thesis-filter").display = True
        self.query_one("#thesis-filter").focus()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "thesis-filter":
            self.refresh_list()

    def action_back(self) -> None:
        """Back out one step: a running job, then the filter, then the pane, then focus."""
        if self._summarising or self._finding:
            self.workers.cancel_group(self, "thesis-summary")
            self.workers.cancel_group(self, "thesis-discover")
            self._summarising = False
            self._finding = False
            self.query_one("#thesis-detail-pane", Pane).set_badge("")
            self.notify("cancelled")
            return
        filter_input = self.query_one("#thesis-filter", Input)
        if filter_input.display:
            filter_input.value = ""
            filter_input.display = False
            self.query_one("#thesis-table").focus()
            return
        if not self._wide and self._show_evidence:
            self.action_toggle_evidence()
            return
        self.query_one("#thesis-table").focus()

    # ---------- claims list ----------

    async def refresh_view(self) -> None:
        self.reload_theses()
        await self.render_detail()

    def reload_theses(self) -> None:
        """Re-read the claims from the database, then redraw the list.

        The only place that queries: filtering and resizing redraw from this
        cache, so neither a keystroke in the filter box nor a column of a drag
        resize costs a query.
        """
        self._theses = theses.list_theses(self.rig.engine)
        self.refresh_list()

    def refresh_list(self) -> None:
        table = self.query_one("#thesis-table", RiggerTable)
        query = self.query_one("#thesis-filter", Input).value.casefold().strip()
        rows = (
            [row for row in self._theses if query in row.claim.casefold()]
            if query
            else self._theses
        )
        index = {row.id: position for position, row in enumerate(rows)}
        colours = self._colours()
        with self.prevent(RiggerTable.RowHighlighted, RiggerTable.RowSelected):
            table.clear()
            if self.selected not in index:
                self.selected = rows[0].id if rows else None
                self._summary = None
            for row in rows:
                glyph, token = STATUS_MARKS.get(row.status, ("○", "foreground"))
                cell = Text(glyph, style=colours.get(token, "white"))
                cell.append(" " + row.claim, style="")
                table.add_row(cell, key=row.id)
            if self.selected is not None:
                table.move_cursor(row=index[self.selected])
        self.query_one("#thesis-claims", Pane).set_badge(str(len(rows)))

    async def on_data_table_row_selected(self, event: RiggerTable.RowSelected) -> None:
        if event.data_table.id == "thesis-table" and event.row_key.value is not None:
            self.selected = str(event.row_key.value)
            self._summary = None
            await self.render_detail()

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
            await self.render_detail()
            self.notify(
                f"{len(found)} candidate{'' if len(found) == 1 else 's'} to review"
                if found
                else "no new candidates found"
            )
            if found and (self._wide or self._show_evidence):
                self.query_one("#thesis-ledger").focus()
        except Exception as exc:
            self.notify(f"discovery failed: {exc}", severity="error")
        finally:
            self._finding = False

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
        text = "No evidence yet. Press f to find candidate evidence."
        if row:
            side = SIDE_MARKS[row.side][2]
            text = f"{side} · {'accepted' if row.accepted else 'pending review'}\n\n{row.note}\n\n{self._cites.get(row.evidence_id, row.evidence_id)}"
        self.query_one("#thesis-preview-text", Static).update(text)
        # A new note starts at its top, not wherever the last one was left.
        self.query_one("#thesis-preview", VerticalScroll).scroll_home(animate=False)
        self._render_actions(row)
        # Whether the note overflows is only known once the new text has been
        # laid out, so the line is checked again then — and re-rendered only if
        # that answer changed, rather than unconditionally drawing it twice.
        self.call_after_refresh(self._rerender_actions_if_overflow_changed, row)

    def _rerender_actions_if_overflow_changed(self, row: theses.ThesisEvidence | None) -> None:
        if self._overflowing != self._preview_overflows():
            self._render_actions(row)

    def _render_actions(self, row: theses.ThesisEvidence | None) -> None:
        """Say what applies to the highlighted row, rather than greying buttons.

        Buttons here were key hints in costume: they duplicated the keys and
        the screen's key strip, and each one was a tab stop, so tab stopped
        meaning "next pane". A line of text carries the same information and
        changes with the row, which disabled buttons only hinted at by fading.
        """
        if row is None:
            wanted: tuple[str, ...] = ()
        elif row.accepted:
            # Accepted evidence is removed in two steps — un-accept, then
            # reject — so one stray keystroke cannot delete curated evidence.
            wanted = ("u",)
        else:
            wanted = ("a", "x")
        # Labels come from BINDINGS rather than a second list, for the same
        # reason the key strip is generated: two spellings of one key drift.
        by_key = {b.key: b for b in shown_bindings(self.BINDINGS)}
        keys = [(key, by_key[key].description) for key in wanted if key in by_key]
        # Only the keys are styled here; the muted body colour comes from the
        # widget's own CSS, because $text-muted resolves to a blend token that
        # is not a colour Rich can parse.
        key_style = f"bold {self._colours().get('text-primary', 'white')}"
        line = Text(no_wrap=True, overflow="ellipsis")
        if not keys:
            line.append("nothing to review")
        for index, (key, label) in enumerate(keys):
            if index:
                line.append("   ")
            line.append(key, style=key_style)
            line.append(f" {label}")
        self._overflowing = self._preview_overflows()
        if self._overflowing:
            line.append("   ")
            line.append("⇧↑↓", style=key_style)
            line.append(" scroll note")
        self.query_one("#thesis-actions", Static).update(line)

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
        if not self._wide and not self._show_evidence:
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
        if self.selected is None:
            await detail.mount(
                Static("No theses yet.\n\nPress n to create a claim to research.", markup=False)
            )
            self._fill_ledger()
            self.query_one("#thesis-counts", Static).update("No evidence")
            self.query_one("#thesis-evidence-pane", Pane).set_badge("0")
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
        await detail.mount(*self._detail_widgets(thesis, result, by_id, now))
        self._fill_ledger()
        # The per-side counts are read off the health result rather than
        # regrouped here, so the legend and the pill cannot disagree.
        counts = (result.support, result.against, result.neutral) if result else (0, 0, 0)
        self.query_one("#thesis-counts", Static).update(
            f"+ {counts[0]} supporting  − {counts[1]} against  ? {counts[2]} neutral"
        )
        self.query_one("#thesis-evidence-pane", Pane).set_badge(
            f"{len(self.candidates)} pending / {len(links)} total"
        )
        detail.scroll_to(y=offset, animate=False)

    def _detail_widgets(
        self,
        thesis: theses.Thesis,
        result: HealthResult | None,
        by_id: dict[str, EvidenceItem],
        now: datetime,
    ) -> list[Widget]:
        pill = (
            Pill(badge_text(result), variant=health_variant(result.state), id="thesis-health")
            if result is not None
            else Pill("emerging · no accepted evidence", variant="dim", id="thesis-health")
        )
        widgets: list[Widget] = [
            Static(thesis.claim, classes="thesis-claim", markup=False),
            Horizontal(
                Pill(thesis.status),
                Pill(thesis.time_horizon or "no horizon", variant="dim"),
                Pill(f"opened {thesis.created_at:%d %b %Y}", variant="dim"),
                classes="thesis-meta",
            ),
            Horizontal(pill, classes="thesis-health-row"),
        ]
        if result is not None and result.drivers:
            widgets.append(
                Static("moved by  " + ", ".join(result.drivers), classes="thesis-drivers")
            )
        widgets.append(
            Static(
                f"{'targets':10}" + (", ".join(thesis.targets) or "all evidence"),
                classes="thesis-field",
                markup=False,
            )
        )
        for label, value in (
            ("scope", thesis.scope),
            ("holds if", "; ".join(thesis.assumptions)),
            ("breaks if", "; ".join(thesis.falsifiers)),
        ):
            if value:
                widgets.append(Static(f"{label:10}{value}", classes="thesis-field", markup=False))
        if not thesis.falsifiers:
            widgets.append(
                Static(
                    "breaks if  not set — press d to say what would disprove this",
                    classes="muted",
                    markup=False,
                )
            )
        widgets.append(Static("SUMMARY", classes="thesis-section"))
        widgets.extend(self._summary_widgets(by_id, now))
        widgets.append(Static(f"REVIEW QUEUE  {len(self.candidates):02}", classes="thesis-section"))
        widgets.append(
            Static(
                "Press e to review pending evidence."
                if self.candidates
                else "All caught up. Press f to find more.",
                markup=False,
            )
        )
        return widgets

    def _fill_ledger(self) -> None:
        """Repopulate the ledger from the current links, keeping the cursor row."""
        table = self.query_one("#thesis-ledger", RiggerTable)
        cursor = table.cursor_row
        colours = self._colours()
        with self.prevent(RiggerTable.RowHighlighted, RiggerTable.RowSelected):
            table.clear()
            self._rows = {}
            for row in sorted(self._links, key=lambda r: r.accepted):
                self._rows[row.evidence_id] = row
                symbol, token, _label = SIDE_MARKS[row.side]
                table.add_row(
                    "✓" if row.accepted else "",
                    Text(symbol, style=colours.get(token, "white")),
                    self._ages.get(row.evidence_id, "—"),
                    row.note or row.evidence_id,
                    key=row.evidence_id,
                )
            if table.row_count:
                table.move_cursor(row=min(cursor, table.row_count - 1))
        self._preview()

    def _summary_widgets(self, by_id: dict[str, EvidenceItem], now: datetime) -> list[Widget]:
        if self._summary is None:
            return [
                Static("No summary yet. Press s to summarise accepted evidence.", classes="muted")
            ]
        as_of = to_utc(self._summary.as_of)
        newest = max(
            (to_utc(item.ts) for item in by_id.values()),
            default=None,
        )
        stale = newest is not None and newest > as_of
        age = age_text(now - as_of)[0]
        widgets: list[Widget] = [
            Static(
                f"{age} old · {len(self._summary.citations)} citations"
                + (" · evidence has moved since" if stale else ""),
                classes="muted",
                markup=False,
            ),
            Static(self._summary.summary, markup=False, classes="muted" if stale else ""),
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
