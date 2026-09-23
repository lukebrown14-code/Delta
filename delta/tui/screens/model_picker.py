"""Model picker modal: autocomplete over the catalog with a free-text fallback."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal
from textual.widgets import Button, DataTable, Input, OptionList, Static
from textual.widgets.option_list import Option

from delta.llm.catalog import ModelInfo, cached_catalog, catalog
from delta.llm.providers import Provider
from delta.tui.widgets import (
    MODAL_WIDTH_WIDE,
    ActionChip,
    DeltaTable,
    Dialog,
    hint_markup,
    token_color,
)


def _per_million(price: float) -> str:
    return f"{price * 1_000_000:.2f}"


def _price_style(price_per_million: float) -> str:
    """Grade a price string: green cheap, amber mid, red premium."""
    if price_per_million < 1:
        return "success"
    if price_per_million < 10:
        return "warning"
    return "error"


class ModelPicker(Dialog):
    """Browse and pick a model; typing autocompletes, enter selects, esc cancels.

    Loads from the on-disk catalog cache so it opens instantly. While typing,
    a dropdown suggests matching ids — ↑↓ browse, enter adopts the highlighted
    one. An empty catalog degrades to free-text model entry.
    """

    BINDINGS = [Binding("ctrl+r", "refresh", "Refresh catalog")]

    # ctrl+r is left to the chip: saying it twice on one dialog is noise.
    dialog_hint = hint_markup(("enter", "select"), ("esc", "cancel"))
    #: The documented exception to MODAL_WIDTH: four columns of catalog.
    dialog_width = MODAL_WIDTH_WIDE
    #: Suggestions shown at most in the autocomplete dropdown.
    SUGGESTION_CAP = 5

    DEFAULT_CSS = """
    ModelPicker #mp-filter {
        margin: 0 0 1 0;
    }
    ModelPicker #mp-suggestions {
        display: none;
        height: auto;
        max-height: 5;
        border: none;
        background: $panel;
        scrollbar-size-horizontal: 0;
    }
    ModelPicker.-suggesting #mp-suggestions {
        display: block;
    }
    /* A header plus nine models is what is left once the frame, title, filter,
       chip row and hint have taken their share of a 24-row terminal. One row
       more and the hint disappears under the bottom border; while the dropdown
       is open the table gives most of that back. */
    ModelPicker #mp-table {
        height: auto;
        max-height: 9;
    }
    ModelPicker.-suggesting #mp-table {
        max-height: 4;
    }
    ModelPicker .modal-chips {
        height: 1;
        margin: 1 0 0 0;
    }
    ModelPicker #mp-status {
        width: 1fr;
        color: $text-muted;
    }
    """

    def __init__(
        self,
        on_select: Callable[[ModelInfo], None],
        *,
        provider: Provider | None = None,
        provider_name: str = "",
    ) -> None:
        super().__init__()
        self.on_select = on_select
        self.provider = provider
        self.provider_name = provider_name or (provider.name if provider else "")
        self._models: list[ModelInfo] = []
        self._visible: list[ModelInfo] = []
        self._suggestions: list[ModelInfo] = []
        self._highlight = 0
        self.dialog_title = f"select a model ({self.provider_name or 'any'})"

    def compose_dialog(self) -> ComposeResult:
        yield Input(placeholder="filter by id or name", id="mp-filter")
        yield OptionList(id="mp-suggestions")
        yield DeltaTable(id="mp-table")
        yield Horizontal(
            ActionChip("ctrl+r", "refresh", id="mp-refresh"),
            Static("", id="mp-status", markup=False),
            classes="modal-chips",
        )

    def on_mount(self) -> None:
        table = self.query_one("#mp-table", DataTable)
        table.add_columns("Model", "Context", "$/1M in", "$/1M out")
        # Suggestions are browsed through the filter's arrows; the list itself
        # must never steal focus or tab stops.
        self.query_one("#mp-suggestions", OptionList).can_focus = False
        self._load(cached_catalog(self.provider_name))

    def _load(self, models: list[ModelInfo]) -> None:
        self._models = models
        self._update_rows()
        self._update_suggestions(self.query_one("#mp-filter", Input).value)

    def _matching(self, needle: str) -> list[ModelInfo]:
        """Autocomplete candidates: prefix matches first, then contains."""
        needle = needle.strip().lower()
        if not needle:
            return []

        def hit(m: ModelInfo) -> bool:
            return needle in m.id.lower() or needle in m.name.lower()

        starts = [
            m
            for m in self._models
            if m.id.lower().startswith(needle) or m.name.lower().startswith(needle)
        ]
        rest = [m for m in self._models if m not in starts and hit(m)]
        return (starts + rest)[: self.SUGGESTION_CAP]

    def _update_suggestions(self, value: str) -> None:
        options = self.query_one("#mp-suggestions", OptionList)
        options.clear_options()
        self._suggestions = self._matching(value)
        for m in self._suggestions:
            prompt = Text.assemble(
                (m.id, "bold"),
                # Rich styles need a resolved colour, not a CSS token name.
                (f" — {m.name}", token_color(self.app, "text-muted", "dim")),
            )
            options.add_option(Option(prompt, id=m.id))
        self._highlight = 0
        if self._suggestions:
            options.highlighted = 0
        self.set_class(bool(self._suggestions), "-suggesting")

    def _update_rows(self) -> None:
        table = self.query_one("#mp-table", DataTable)
        filter_input = self.query_one("#mp-filter", Input)
        status = self.query_one("#mp-status", Static)
        table.clear()
        if not self._models:
            filter_input.placeholder = "no catalog: type a model id and press enter"
            status.update("catalog empty — enter a model id as free text")
            self._visible = []
            return
        filter_input.placeholder = "filter by id or name"
        needle = filter_input.value.strip().lower()
        self._visible = [
            m
            for m in self._models
            if not needle or needle in m.id.lower() or needle in m.name.lower()
        ]
        for m in self._visible:
            context = "" if m.context_length is None else str(m.context_length)
            prompt = m.prompt_price * 1_000_000
            completion = m.completion_price * 1_000_000
            table.add_row(
                m.id,
                context,
                Text(_per_million(m.prompt_price), style=_price_style(prompt)),
                Text(_per_million(m.completion_price), style=_price_style(completion)),
                key=m.id,
            )
        status.update(f"{table.row_count} models")

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "mp-filter":
            self._update_suggestions(event.value)
            self._update_rows()

    def on_key(self, event: Any) -> None:
        """Arrows browse the dropdown while the filter keeps focus."""
        if getattr(self.focused, "id", None) != "mp-filter" or not self._suggestions:
            return
        if event.key not in {"down", "up"}:
            return
        event.stop()
        event.prevent_default()
        delta = 1 if event.key == "down" else -1
        self._highlight = (self._highlight + delta) % len(self._suggestions)
        self.query_one("#mp-suggestions", OptionList).highlighted = self._highlight

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id != "mp-filter":
            return
        # Enter with the dropdown open adopts the highlighted suggestion.
        if self._suggestions:
            self._select(self._suggestions[min(self._highlight, len(self._suggestions) - 1)])
            return
        value = event.value.strip()
        if not self._models:
            if value:
                self._select(
                    ModelInfo(
                        id=value,
                        name=value,
                        context_length=None,
                        prompt_price=0.0,
                        completion_price=0.0,
                    )
                )
            return
        if self._visible:
            self._select(self._visible[0])
        else:
            self.notify("no models match the filter", severity="warning")

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        if getattr(event.option_list, "id", None) != "mp-suggestions" or not event.option.id:
            return
        event.stop()
        for m in self._suggestions:
            if m.id == event.option.id:
                self._select(m)
                return

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        for m in self._visible:
            if m.id == event.row_key.value:
                self._select(m)
                return

    async def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "mp-refresh":
            await self._refresh()

    async def action_refresh(self) -> None:
        await self._refresh()

    async def _refresh(self) -> None:
        if self.provider is None:
            self._load(cached_catalog(self.provider_name))
            return
        self._load(await catalog(self.provider, force=True))

    def action_dismiss_dialog(self) -> None:
        """Escape: close the dropdown first, the dialog second."""
        if self._suggestions:
            self._update_suggestions("")
            self.query_one("#mp-filter", Input).focus()
            return
        self.dismiss(None)

    def _select(self, model: ModelInfo) -> None:
        self.dismiss(None)
        self.on_select(model)
