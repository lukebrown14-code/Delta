"""Model picker modal: searchable catalog with a free-text fallback."""

from __future__ import annotations

from collections.abc import Callable

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal
from textual.widgets import Button, DataTable, Input, Static

from rigger.llm.catalog import ModelInfo, cached_catalog, catalog
from rigger.llm.providers import Provider
from rigger.tui.widgets import (
    MODAL_WIDTH_WIDE,
    ActionChip,
    Dialog,
    RiggerTable,
    hint_markup,
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
    """Browse and pick a model; enter selects, escape cancels, refresh refetches.

    Loads from the on-disk catalog cache so it opens instantly. An empty
    catalog degrades to free-text model entry.
    """

    BINDINGS = [Binding("ctrl+r", "refresh", "Refresh catalog")]

    # ctrl+r is left to the chip: saying it twice on one dialog is noise.
    dialog_hint = hint_markup(("enter", "select"), ("esc", "cancel"))
    #: The documented exception to MODAL_WIDTH: four columns of catalog.
    dialog_width = MODAL_WIDTH_WIDE

    DEFAULT_CSS = """
    ModelPicker #mp-filter {
        margin: 0 0 1 0;
    }
    /* A header plus nine models is what is left once the frame, title, filter,
       chip row and hint have taken their share of a 24-row terminal. One row
       more and the hint disappears under the bottom border. */
    ModelPicker #mp-table {
        height: auto;
        max-height: 9;
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
        self.dialog_title = f"select a model ({self.provider_name or 'any'})"

    def compose_dialog(self) -> ComposeResult:
        yield Input(placeholder="filter by id or name", id="mp-filter")
        yield RiggerTable(id="mp-table")
        yield Horizontal(
            ActionChip("ctrl+r", "refresh", id="mp-refresh"),
            Static("", id="mp-status", markup=False),
            classes="modal-chips",
        )

    def on_mount(self) -> None:
        table = self.query_one("#mp-table", DataTable)
        table.add_columns("Model", "Context", "$/1M in", "$/1M out")
        self._load(cached_catalog(self.provider_name))

    def _load(self, models: list[ModelInfo]) -> None:
        self._models = models
        self._update_rows()

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
            self._update_rows()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id != "mp-filter":
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

    def _select(self, model: ModelInfo) -> None:
        self.dismiss(None)
        self.on_select(model)
