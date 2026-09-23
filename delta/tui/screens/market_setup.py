"""Settings dialog for config-backed exchange markets.

Typing an exchange id suggests known world exchanges; picking one fills the
id, name, currency and Yahoo suffix, so adding a market is one keystroke
instead of four fields by hand.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.screen import ModalScreen
from textual.widgets import Button, Input, OptionList, Static
from textual.widgets.option_list import Option

from delta.tui.widgets import token_color


@dataclass(frozen=True)
class Exchange:
    """One known exchange a market can be created from."""

    id: str
    label: str
    currency: str
    yahoo_suffix: str = ""


#: Common world exchanges offered by autocomplete, with their Yahoo suffix.
#: ``us`` and ``asx`` are built-in markets already, so they are not suggested.
KNOWN_EXCHANGES: tuple[Exchange, ...] = (
    Exchange("lse", "London Stock Exchange", "GBP", ".L"),
    Exchange("tor", "Toronto Stock Exchange", "CAD", ".TO"),
    Exchange("hk", "Hong Kong Stock Exchange", "HKD", ".HK"),
    Exchange("nse", "National Stock Exchange of India", "INR", ".NS"),
    Exchange("jp", "Tokyo Stock Exchange", "JPY", ".T"),
    Exchange("de", "Deutsche Börse (XETRA)", "EUR", ".DE"),
    Exchange("par", "Euronext Paris", "EUR", ".PA"),
    Exchange("ams", "Euronext Amsterdam", "EUR", ".AS"),
    Exchange("ss", "Shanghai Stock Exchange", "CNY", ".SS"),
    Exchange("sz", "Shenzhen Stock Exchange", "CNY", ".SZ"),
    Exchange("swx", "SIX Swiss Exchange", "CHF", ".SW"),
    Exchange("ks", "Korea Exchange", "KRW", ".KS"),
    Exchange("tw", "Taiwan Stock Exchange", "TWD", ".TW"),
    Exchange("sa", "Saudi Exchange", "SAR", ".SR"),
    Exchange("sgx", "Singapore Exchange", "SGD", ".SI"),
    Exchange("br", "B3 (Brazil)", "BRL", ".SA"),
    Exchange("mx", "Mexican Stock Exchange", "MXN", ".MX"),
    Exchange("nz", "NZX (New Zealand)", "NZD", ".NZ"),
)


class MarketSetupModal(ModalScreen[dict[str, str] | None]):
    BINDINGS = [Binding("escape", "cancel", "Cancel")]

    #: Suggestions shown at most in the autocomplete dropdown.
    SUGGESTION_CAP = 5

    DEFAULT_CSS = """
    MarketSetupModal > Vertical { width: 64; }
    MarketSetupModal Input { margin: 1 0 0 0; }
    MarketSetupModal #market-suggestions {
        display: none;
        height: auto;
        max-height: 5;
        border: none;
        background: $panel;
        scrollbar-size-horizontal: 0;
    }
    MarketSetupModal.-suggesting #market-suggestions { display: block; }
    """

    def __init__(self, current: dict[str, str] | None = None, *, editable_id: bool = True) -> None:
        super().__init__()
        self.current = current or {}
        self.editable_id = editable_id
        self._suppress = False
        self._suggestions: list[Exchange] = []
        self._highlight = 0

    def compose(self) -> ComposeResult:
        title = "Add market" if self.editable_id else "Edit market"
        yield Vertical(
            Static(f"[bold]{title}[/bold]", markup=True),
            Static("Type an exchange id to autocomplete, or fill the fields below.", markup=False),
            Input(value=self.current.get("id", ""), placeholder="ID, e.g. lse", id="market-id", disabled=not self.editable_id),
            OptionList(id="market-suggestions"),
            Input(value=self.current.get("label", ""), placeholder="Exchange name", id="market-label"),
            Input(value=self.current.get("currency", ""), placeholder="Currency, e.g. GBP", id="market-currency"),
            Input(value=self.current.get("yahoo_suffix", ""), placeholder="Yahoo suffix, e.g. .L (optional)", id="market-suffix"),
            Horizontal(Button("Save", id="market-save"), Button("Cancel", id="market-cancel")),
        )

    def on_mount(self) -> None:
        # Suggestions are browsed through the id field's arrows; the list must
        # never steal focus or tab stops.
        self.query_one("#market-suggestions", OptionList).can_focus = False

    # ----- autocomplete ----------------------------------------------------

    def _matching(self, needle: str) -> list[Exchange]:
        needle = needle.strip().lower()
        if not needle:
            return []

        def hit(exchange: Exchange) -> bool:
            return needle in exchange.id.lower() or needle in exchange.label.lower()

        starts = [
            ex
            for ex in KNOWN_EXCHANGES
            if ex.id.lower().startswith(needle) or ex.label.lower().startswith(needle)
        ]
        rest = [ex for ex in KNOWN_EXCHANGES if ex not in starts and hit(ex)]
        return (starts + rest)[: self.SUGGESTION_CAP]

    def _update_suggestions(self, value: str) -> None:
        options = self.query_one("#market-suggestions", OptionList)
        options.clear_options()
        self._suggestions = self._matching(value)
        muted = token_color(self.app, "text-muted", "dim")
        for exchange in self._suggestions:
            prompt = Text.assemble(
                (exchange.id, "bold"),
                (f" — {exchange.label}", muted),
                (f" · {exchange.currency}", muted),
            )
            options.add_option(Option(prompt, id=exchange.id))
        self._highlight = 0
        if self._suggestions:
            options.highlighted = 0
        self.set_class(bool(self._suggestions), "-suggesting")

    def _adopt(self, exchange: Exchange) -> None:
        self._suppress = True
        self.query_one("#market-id", Input).value = exchange.id
        self.query_one("#market-label", Input).value = exchange.label
        self.query_one("#market-currency", Input).value = exchange.currency
        self.query_one("#market-suffix", Input).value = exchange.yahoo_suffix
        self._update_suggestions("")
        self.query_one("#market-label", Input).focus()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id != "market-id":
            return
        if self._suppress:
            self._suppress = False
            return
        self._update_suggestions(event.value)

    def on_key(self, event: Any) -> None:
        """Arrows browse the dropdown while the id field keeps focus."""
        if getattr(self.focused, "id", None) != "market-id" or not self._suggestions:
            return
        if event.key not in {"down", "up"}:
            return
        event.stop()
        event.prevent_default()
        delta = 1 if event.key == "down" else -1
        self._highlight = (self._highlight + delta) % len(self._suggestions)
        self.query_one("#market-suggestions", OptionList).highlighted = self._highlight

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        if getattr(event.option_list, "id", None) != "market-suggestions" or not event.option.id:
            return
        event.stop()
        for exchange in self._suggestions:
            if exchange.id == event.option.id:
                self._adopt(exchange)
                return

    def on_input_submitted(self, event: Input.Submitted) -> None:
        # Enter with the dropdown open adopts the highlighted exchange; the
        # next enter (or enter on an empty dropdown) saves.
        if event.input.id == "market-id" and self._suggestions:
            self._adopt(self._suggestions[min(self._highlight, len(self._suggestions) - 1)])
            return
        self._save()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "market-save":
            self._save()
        else:
            self.dismiss(None)

    def _save(self) -> None:
        values = {
            "id": self.query_one("#market-id", Input).value,
            "label": self.query_one("#market-label", Input).value,
            "currency": self.query_one("#market-currency", Input).value,
            "yahoo_suffix": self.query_one("#market-suffix", Input).value,
        }
        if not all(values[key].strip() for key in ("id", "label", "currency")):
            self.notify("ID, exchange name, and currency are required", severity="warning")
            return
        self.dismiss(values)

    def action_cancel(self) -> None:
        """Escape: close the dropdown first, the dialog second."""
        if self._suggestions:
            self._update_suggestions("")
            self.query_one("#market-id", Input).focus()
            return
        self.dismiss(None)
