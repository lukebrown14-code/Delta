"""User-owned decision journal.

The journal deliberately records the context of a decision; it does not score
or recommend securities.  Keeping this screen separate from Research makes it
easy to revisit the original rationale without changing the evidence view.
"""

from __future__ import annotations

import asyncio
from datetime import date, datetime, timedelta
from typing import Any

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, VerticalScroll
from textual.widgets import Input, Select, Static, TextArea
from textual.widgets.option_list import Option

from delta import decisions
from delta import theses as theses_mod
from delta.tui.components import EmptyState, SectionHeading, SuggestionList, goto, require_selection
from delta.tui.screens.research import ResearchState
from delta.tui.shell import DeltaScreen
from delta.tui.widgets import (
    MODAL_WIDTH_WIDE,
    DeltaTable,
    Dialog,
    Pane,
    PaneRow,
    hint_markup,
    token_color,
)


def _value(row: Any, name: str, default: Any = "") -> Any:
    """Read a model field while keeping the view tolerant of future additions."""
    return getattr(row, name, default)


def _date(value: Any) -> str:
    if isinstance(value, datetime):
        return value.date().isoformat()
    return str(value or "—")[:10]


def _thesis_options(engine: Any) -> list[tuple[str, str]]:
    """``(label, id)`` pairs for the thesis picker."""
    return [
        (f"{row.claim[:48]}{'…' if len(row.claim) > 48 else ''}", row.id)
        for row in theses_mod.list_theses(engine)
    ]


def _preset_date(preset: str) -> str:
    """Resolve a ``+3m``/``+6m``/``+1y`` preset to an ISO date."""
    months = {"+3m": 3, "+6m": 6, "+1y": 12}[preset]
    return (date.today() + timedelta(days=30 * months)).isoformat()


class DecisionForm(Dialog):
    """Modal for recording or editing the opening decision record.

    Driving a journal wants room to think, so the three prose fields are
    multiline ``TextArea``s and the modal is wider than the default. Enter steps
    to the next field rather than saving — a half-finished entry must not commit
    — and ``ctrl+s`` is the one save key.
    """

    dialog_title = "new decision"
    dialog_hint = hint_markup(("tab", "next field"), ("ctrl+s", "save"), ("esc", "cancel"))
    dialog_width = MODAL_WIDTH_WIDE

    BINDINGS = [Binding("ctrl+s", "save", "save", priority=True)]

    DEFAULT_CSS = """
    DecisionForm .form-row { height: auto; }
    DecisionForm Input, DecisionForm Select { height: 1; margin: 0 0 1 0; }
    DecisionForm TextArea { height: 3; margin: 0 0 1 0; }
    DecisionForm .form-row Input { width: 1fr; }
    DecisionForm #decision-instrument { width: 1fr; }
    DecisionForm #decision-suggestions { height: 4; border: none; background: $panel; margin: 0 0 1 0; }
    DecisionForm #decision-hint { height: 1; color: $text-muted; }
    """

    def __init__(self, decision: decisions.Decision | None = None) -> None:
        super().__init__()
        self.decision = decision
        self.dialog_title = "edit decision" if decision else "new decision"
        self._instruments: list[str] = []
        self._suppress = False
        self._seed = decision

    def _scope(self) -> Any:
        """The ``delta`` object the pushing screen carries, for engine/universe."""
        for screen in self.app.screen_stack:
            delta = getattr(screen, "delta", None)
            if delta is not None:
                return delta
        return None

    def compose_dialog(self) -> ComposeResult:
        seed = self._seed
        yield Static(
            "type an instrument to autocomplete · enter steps to the next field",
            id="decision-hint",
            markup=False,
        )
        yield Input(
            value=seed.instrument_id if seed else "",
            placeholder="Instrument (US:AAPL)",
            id="decision-instrument",
        )
        yield SuggestionList(id="decision-suggestions")
        yield TextArea(seed.rationale if seed else "", id="decision-rationale")
        yield TextArea(seed.valuation_context if seed else "", id="decision-valuation")
        with Horizontal(classes="form-row"):
            yield Input(
                value=seed.time_horizon if seed else "",
                placeholder="Time horizon (5y)",
                id="decision-horizon",
            )
            yield Select(
                [(label, label) for label in ("+3m", "+6m", "+1y")],
                prompt="review date…",
                value=None,
                allow_blank=True,
                id="decision-review-preset",
            )
        yield Input(
            value=seed.review_date.isoformat() if seed else "",
            placeholder="Review date (YYYY-MM-DD)",
            id="decision-review-at",
        )
        yield TextArea(seed.invalidation_criteria if seed else "", id="decision-invalidation")
        engine = getattr(self._scope(), "engine", None)
        yield Select(
            _thesis_options(engine) if engine else [],
            prompt="Linked thesis (optional)",
            value=seed.thesis_id if seed else None,
            allow_blank=True,
            id="decision-thesis",
        )

    def on_mount(self) -> None:
        self.query_one("#decision-instrument", Input).focus()

    def _move_focus(self) -> None:
        """Send focus to the next field in compose order; wraps to save."""
        order = [
            "#decision-instrument",
            "#decision-rationale",
            "#decision-valuation",
            "#decision-horizon",
            "#decision-review-preset",
            "#decision-review-at",
            "#decision-invalidation",
            "#decision-thesis",
        ]
        current = getattr(self.focused, "id", None)
        index = order.index(current) if current in order else -1
        if index + 1 < len(order):
            self.query_one(order[index + 1]).focus()
        else:
            self.save()

    # ----- instrument autocomplete -----

    def _universe_ids(self) -> list[str]:
        delta = self._scope()
        universe = getattr(delta, "universe", None)
        if not callable(universe):
            return []
        try:
            return [i.id for i in universe()]
        except Exception:
            return []

    def _matching(self, needle: str) -> list[str]:
        needle = needle.strip().lower()
        if not needle:
            return []
        instruments = self._universe_ids()
        starts = [i for i in instruments if i.lower().startswith(needle)]
        rest = [i for i in instruments if i not in starts and needle in i.lower()]
        return (starts + rest)[:6]

    def _update_suggestions(self, value: str) -> None:
        self._instruments = self._matching(value)
        self.query_one("#decision-suggestions", SuggestionList).show(
            Option(ident, id=ident) for ident in self._instruments
        )

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id != "decision-instrument":
            return
        if self._suppress:
            self._suppress = False
            return
        self._update_suggestions(event.value)

    def on_key(self, event: Any) -> None:
        """Arrows browse the suggestions while the instrument field keeps focus."""
        if getattr(self.focused, "id", None) != "decision-instrument":
            return
        self.query_one("#decision-suggestions", SuggestionList).browse(event)

    def on_option_list_option_selected(self, event: Any) -> None:
        if getattr(event.option_list, "id", None) != "decision-suggestions":
            return
        if not event.option.id:
            return
        event.stop()
        self._suppress = True
        self.query_one("#decision-instrument", Input).value = str(event.option.id)
        self._update_suggestions("")
        self.query_one("#decision-rationale", TextArea).focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        # Enter with a live dropdown adopts the highlighted suggestion, then
        # enter again steps onward; enter on a prose field never submits.
        if event.input.id == "decision-instrument" and self._instruments:
            index = self.query_one("#decision-suggestions", SuggestionList).highlighted_index
            self._suppress = True
            self.query_one("#decision-instrument", Input).value = self._instruments[index]
            self._update_suggestions("")
            self.query_one("#decision-rationale", TextArea).focus()
            return
        if event.input.id in ("decision-instrument", "decision-horizon", "decision-review-at"):
            self._move_focus()

    def action_save(self) -> None:
        self.save()

    def save(self) -> None:
        instrument = self.query_one("#decision-instrument", Input).value.strip()
        rationale = self.query_one("#decision-rationale", TextArea).text.strip()
        valuation = self.query_one("#decision-valuation", TextArea).text.strip()
        horizon = self.query_one("#decision-horizon", Input).value.strip()
        review_at = self.query_one("#decision-review-at", Input).value.strip()
        invalidation = self.query_one("#decision-invalidation", TextArea).text.strip()
        thesis_id = self.query_one("#decision-thesis", Select).value

        preset = self.query_one("#decision-review-preset", Select).value
        if preset and not review_at:
            review_at = _preset_date(str(preset))

        required = {
            "instrument": instrument,
            "rationale": rationale,
            "valuation context": valuation,
            "time horizon": horizon,
            "review date": review_at,
            "invalidation criteria": invalidation,
        }
        missing = [label for label, value in required.items() if not value]
        if missing:
            self.notify(f"{', '.join(missing)} required", severity="error")
            return
        try:
            review_date = date.fromisoformat(review_at)
        except ValueError:
            self.notify("review date must be YYYY-MM-DD", severity="error")
            return
        self.dismiss(
            {
                "instrument_id": instrument,
                "rationale": rationale,
                "valuation_context": valuation,
                "time_horizon": horizon,
                "review_date": review_date,
                "invalidation_criteria": invalidation,
                "thesis_id": str(thesis_id) if thesis_id else None,
            }
        )


class ReviewForm(Dialog):
    dialog_title = "review decision"
    dialog_hint = hint_markup(("ctrl+s", "save"), ("esc", "cancel"))

    BINDINGS = [Binding("ctrl+s", "save", "save", priority=True)]

    def compose_dialog(self) -> ComposeResult:
        yield TextArea("", id="decision-review-note")
        yield Input(
            placeholder="Status: open, reviewed, or retired",
            value="reviewed",
            id="decision-status",
        )

    def on_mount(self) -> None:
        self.query_one("#decision-review-note", TextArea).focus()

    def action_save(self) -> None:
        self.save()

    def save(self) -> None:
        note = self.query_one("#decision-review-note", TextArea).text.strip()
        status = self.query_one("#decision-status", Input).value.strip().lower()
        if not note:
            self.notify("review note is required", severity="error")
        elif status not in {"open", "reviewed", "retired"}:
            self.notify("status must be open, reviewed, or retired", severity="error")
        else:
            self.dismiss({"note": note, "status": status})


class Decisions(DeltaScreen):
    name = "decisions"
    BINDINGS = [
        Binding("n", "new_decision", "new", tooltip="Record decision context"),
        Binding("e", "edit_decision", "edit", tooltip="Edit the framing"),
        Binding("d", "delete_decision", "delete", tooltip="Remove the decision"),
        Binding("r", "review", "review", tooltip="Append a dated review"),
        Binding("o", "open_research", "research", tooltip="Open its evidence"),
        Binding("slash", "filter", "filter", tooltip="Filter by instrument or rationale"),
        Binding("y", "confirm_delete", "confirm", show=False),
        Binding("escape", "back", "back", show=False),
    ]
    CSS = """
    #decisions-split { height: 1fr; }
    #decisions-list { width: 2fr; min-width: 32; }
    #decisions-filter { height: 1; margin: 0; }
    #decisions-table { height: 1fr; }
    #decision-detail-pane { width: 3fr; min-width: 36; }
    #decision-detail { height: 1fr; }
    .decision-field { height: auto; padding: 0 1; }
    .decision-line { height: 1; text-wrap: nowrap; text-overflow: ellipsis; }
    #decisions-split.-narrow > Pane { width: 1fr; min-width: 0; }
    """

    def __init__(self, delta: Any, state: ResearchState | None = None) -> None:
        super().__init__(delta)
        self.state = state or ResearchState()
        self.selected: str | None = None
        self.rows: dict[str, Any] = {}
        self._confirm_delete = False

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="decisions-split"):
            with Pane(title="decisions", id="decisions-list"):
                yield Input(placeholder="/ filter", id="decisions-filter")
                yield DeltaTable(id="decisions-table")
            with Pane(title="timeline", id="decision-detail-pane"):
                yield VerticalScroll(id="decision-detail")

    async def on_mount(self) -> None:
        table = self.query_one("#decisions-table", DeltaTable)
        table.add_column("status", width=10)
        table.add_column("review", width=10)
        table.add_column("instrument", width=14)
        table.add_column("rationale")
        self.query_one("#decisions-filter").display = False
        await self.refresh_view()
        self._paint_hints()
        table.focus()

    def _paint_hints(self) -> None:
        self.query_one("#decisions-list", Pane).set_hints(
            hint_markup(("n", "new"), ("e", "edit"), ("d", "delete"), ("/", "filter"))
        )
        self.query_one("#decision-detail-pane", Pane).set_hints(
            hint_markup(("r", "review"), ("o", "research"), ("↑↓", "scroll"))
        )

    async def refresh_view(self) -> None:
        await self.reload()

    async def reload(self) -> None:
        """Read the journal off the event loop, then repaint."""
        self.rows = {
            str(_value(row, "id")): row
            for row in await asyncio.to_thread(decisions.list_decisions, self.delta.engine)
        }
        if self.selected not in self.rows:
            self.selected = next(iter(self.rows), None)
        self._fill_table()
        await self._render_detail()

    def _fill_table(self) -> None:
        table = self.query_one("#decisions-table", DeltaTable)
        query = self.query_one("#decisions-filter", Input).value.casefold().strip()
        rows = [row for row in self.rows.values() if self._matches(row, query)]
        with self.prevent(DeltaTable.RowHighlighted, DeltaTable.RowSelected):
            table.clear()
            for row in rows:
                table.add_row(
                    str(_value(row, "status", "open")),
                    _date(_value(row, "review_date")),
                    str(_value(row, "instrument_id")),
                    str(_value(row, "rationale")),
                    key=str(_value(row, "id")),
                )
            if self.selected and self.selected in self.rows:
                idx = [str(_value(r, "id")) for r in rows]
                if self.selected in idx:
                    table.move_cursor(row=idx.index(self.selected))
        self.query_one("#decisions-list", Pane).set_badge(str(len(rows)))

    @staticmethod
    def _matches(row: Any, query: str) -> bool:
        if not query:
            return True
        return query in str(_value(row, "instrument_id")).lower() or query in str(
            _value(row, "rationale")
        ).lower()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "decisions-filter":
            self._fill_table()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "decisions-filter":
            self.query_one("#decisions-table").focus()

    def action_filter(self) -> None:
        field = self.query_one("#decisions-filter", Input)
        field.display = True
        field.focus()

    def on_data_table_row_highlighted(self, event: DeltaTable.RowHighlighted) -> None:
        if event.data_table.id == "decisions-table" and event.row_key is not None:
            if event.row_key.value is not None:
                self.selected = str(event.row_key.value)
                self.run_worker(self._render_detail(), exclusive=True)

    async def _render_detail(self) -> None:
        pane = self.query_one("#decision-detail", VerticalScroll)
        await pane.remove_children()
        row = self.rows.get(self.selected or "")
        meta = self.query_one("#decision-detail-pane", Pane)
        if row is None:
            await pane.mount(
                EmptyState(
                    "no decisions yet", key="n", action="record the context you want to revisit"
                )
            )
            meta.set_badge("")
            return
        now_price = self._now_price(_value(row, "instrument_id"))
        created = _value(row, "created_at")
        widgets: list[Any] = [SectionHeading("decision", classes="-first")]
        widgets.extend(
            Static(f"{label:14}{value}", classes="decision-field", markup=False)
            for label, value in (
                ("instrument", _value(row, "instrument_id")),
                ("thesis", _value(row, "thesis_id") or "—"),
                ("horizon", _value(row, "time_horizon")),
                ("now", now_price or "—"),
            )
        )
        for heading, key in (
            ("rationale", "rationale"),
            ("valuation / price context", "valuation_context"),
            ("invalidation criteria", "invalidation_criteria"),
        ):
            widgets += [
                SectionHeading(heading),
                Static(str(_value(row, key)), classes="decision-field", markup=False),
            ]

        # The timeline: created, then each review, then next review due.
        widgets.append(SectionHeading("timeline"))
        history = decisions.review_history(self.delta.engine, str(_value(row, "id")))
        widgets.append(
            Static(
                Text(
                    f"{_date(created)}  open",
                    style=token_color(self.app, "text-success", ""),
                ),
                classes="decision-line",
            )
        )
        for review in history:
            status = _value(review, "status", "") or "reviewed"
            when = _value(review, "created_at")
            widgets.append(
                Static(
                    Text(f"{_date(when)}  {status}", style=token_color(self.app, "text-muted", "")),
                    classes="decision-line",
                )
            )
            note = _value(review, "note")
            if note:
                widgets.append(Static(str(note), classes="decision-field", markup=False))
        due = Text("next review due  ", style=token_color(self.app, "text-warning", ""))
        due.append(_date(_value(row, "review_date")))
        widgets.append(Static(due, classes="decision-line"))
        meta.set_badge(f"{len(history)} reviews")
        await pane.mount(*widgets)

    def _now_price(self, instrument_id: str) -> str | None:
        """The latest stored close for the instrument, or ``None`` when absent."""
        try:
            from delta import services

            closes = services.recent_closes(self.delta.engine, instrument_id, limit=1)
        except Exception:
            return None
        return f"{closes[-1]:.2f}" if closes else None

    def action_new_decision(self) -> None:
        self.app.push_screen(DecisionForm(), self._save_decision)

    def action_edit_decision(self) -> None:
        if not require_selection(self, self.selected, "a decision"):
            return
        row = self.rows.get(self.selected or "")
        self.app.push_screen(DecisionForm(row), self._save_decision)

    async def _save_decision(self, fields: dict[str, Any] | None) -> None:
        if fields is None:
            return
        try:
            if self.selected and self.selected in self.rows:
                row = decisions.update_decision(self.delta.engine, self.selected, **fields)
            else:
                row = decisions.create_decision(self.delta.engine, **fields)
        except (ValueError, KeyError) as exc:
            self.notify(str(exc), severity="error")
            return
        self.selected = str(_value(row, "id"))
        await self.refresh_view()

    def action_delete_decision(self) -> None:
        if not require_selection(self, self.selected, "a decision"):
            return
        self._confirm_delete = True
        self.query_one("#decision-detail-pane", Pane).set_hints(
            hint_markup(("y", "confirm delete"), ("esc", "cancel"))
        )

    async def action_confirm_delete(self) -> None:
        if not self._confirm_delete:
            return
        self._confirm_delete = False
        if self.selected is None:
            return
        try:
            decisions.delete_decision(self.delta.engine, self.selected)
        except (ValueError, KeyError) as exc:
            self.notify(str(exc), severity="error")
            return
        self.selected = None
        await self.refresh_view()

    def action_back(self) -> None:
        if self._confirm_delete:
            self._confirm_delete = False
            self._paint_hints()
            return
        self.query_one("#decisions-table", DeltaTable).focus()

    def action_review(self) -> None:
        if not require_selection(self, self.selected, "a decision"):
            return
        self.app.push_screen(ReviewForm(), self._save_review)

    async def _save_review(self, fields: dict[str, str] | None) -> None:
        if fields is None or self.selected is None:
            return
        try:
            decisions.append_review(self.delta.engine, self.selected, **fields)
        except (ValueError, KeyError) as exc:
            self.notify(str(exc), severity="error")
            return
        await self.refresh_view()

    def action_open_research(self) -> None:
        row = self.rows.get(self.selected or "")
        if not require_selection(self, row, "a decision"):
            return
        self.state.company = str(_value(row, "instrument_id"))
        goto(self.app, "data")