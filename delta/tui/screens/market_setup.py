"""Settings dialog for config-backed exchange markets.

Typing an exchange id suggests known world exchanges; picking one fills the
id, name, currency and Yahoo suffix, so adding a market is one keystroke
instead of four fields by hand. Styled as the watchlist's "add" dialog.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.widgets import Button, Input, Label, OptionList, Static
from textual.widgets.option_list import Option

from delta.tui.widgets import MODAL_WIDTH, Dialog, hint_markup, token_color


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


class MarketSetupModal(Dialog):
    """Add or edit a market; autocomplete fills the form when adding."""

    dialog_width = MODAL_WIDTH

    BINDINGS = [Binding("escape", "dismiss_dialog", "Close")]

    #: Suggestions shown at most in the autocomplete dropdown.
    SUGGESTION_CAP = 5
    #: Suggestion rows kept in reserve while empty, so the form never reflows.
    SUGGESTION_ROWS = 4

    DEFAULT_CSS = f"""
    MarketSetupModal #market-form {{ height: auto; }}
    MarketSetupModal .market-field {{ height: 3; }}
    MarketSetupModal .market-field Label {{ width: 10; padding: 1 0; color: $text-muted; }}
    MarketSetupModal .market-field Input {{ width: 1fr; margin: 0; }}
    MarketSetupModal #market-hint {{ height: 1; color: $text-muted; content-align-horizontal: center; }}
    MarketSetupModal #market-suggestions {{
        height: {SUGGESTION_ROWS};
        margin: 0 0 0 10;
        border: none;
        background: $panel;
        scrollbar-size-horizontal: 0;
    }}
    MarketSetupModal #market-modal-actions {{ height: 1; margin-top: 1; }}
    MarketSetupModal #market-modal-actions Button {{
        height: 1; min-width: 0; border: none; padding: 0 1; margin: 0 1 0 0;
    }}
    """

    def __init__(self, current: dict[str, str] | None = None, *, editable_id: bool = True) -> None:
        super().__init__()
        self.current = current or {}
        self.editable_id = editable_id
        self.dialog_title = "add market" if editable_id else "edit market"
        self.dialog_hint = (
            hint_markup(("↑↓", "choose"), ("enter", "pick · save"), ("esc", "cancel"))
            if editable_id
            else hint_markup(("enter", "save"), ("esc", "cancel"))
        )
        self._suppress = False
        self._suggestions: list[Exchange] = []
        self._highlight = 0

    def compose_dialog(self) -> ComposeResult:
        if self.editable_id:
            yield Static("type an exchange id to autocomplete", id="market-hint", markup=False)
        yield Vertical(
            Horizontal(
                Label("ID"),
                Input(value=self.current.get("id", ""), placeholder="e.g. lse", id="market-id", disabled=not self.editable_id),
                classes="market-field",
            ),
            *(OptionList(id="market-suggestions"),) if self.editable_id else (),
            Horizontal(
                Label("Name"),
                Input(value=self.current.get("label", ""), placeholder="Exchange name", id="market-label"),
                classes="market-field",
            ),
            Horizontal(
                Label("Currency"),
                Input(value=self.current.get("currency", ""), placeholder="GBP", id="market-currency"),
                classes="market-field",
            ),
            Horizontal(
                Label("Yahoo"),
                Input(value=self.current.get("yahoo_suffix", ""), placeholder=".L (optional)", id="market-suffix"),
                classes="market-field",
            ),
            id="market-form",
        )
        yield Horizontal(
            Button("Save", id="market-save", variant="primary"),
            Button("Cancel", id="market-cancel"),
            id="market-modal-actions",
        )

    def on_mount(self) -> None:
        if self.editable_id:
            # Suggestions are browsed through the id field's arrows; the list
            # must never steal focus or tab stops.
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
        if not self.editable_id:
            return
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
        if not self.editable_id or getattr(self.focused, "id", None) != "market-id":
            return
        if not self._suggestions or event.key not in {"down", "up"}:
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

    def action_dismiss_dialog(self) -> None:
        """Escape: close the dropdown first, the dialog second."""
        if self._suggestions:
            self._update_suggestions("")
            self.query_one("#market-id", Input).focus()
            return
        self.dismiss(None)
