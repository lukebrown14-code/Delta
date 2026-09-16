"""Keyboard help screen: the keymap is generated from the app's BINDINGS."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, VerticalScroll
from textual.screen import Screen
from textual.widgets import Static

from rigger.tui.widgets import KeyHint

KEY_DISPLAY = {"question_mark": "?"}


class HelpScreen(Screen):
    name = "help"

    BINDINGS = [Binding("escape", "app.show_help", "Close help", show=False)]

    DEFAULT_CSS = """
    HelpScreen {
        align: center middle;
        background: $background 60%;
    }
    HelpScreen #help-dialog {
        width: 76;
        max-height: 90%;
        background: $surface;
        border: round $primary;
        padding: 1 2;
    }
    HelpScreen .help-title {
        color: $primary;
        text-style: bold;
        margin: 0 0 1 0;
    }
    HelpScreen .help-row {
        height: 1;
        margin: 0 0 1 0;
    }
    HelpScreen .help-row Static {
        width: 1fr;
    }
    """

    def compose(self) -> ComposeResult:
        with VerticalScroll(id="help-dialog"):
            yield Static("Rigger help", classes="help-title", markup=False)
            yield Static(
                "Rigger gathers market data and filings, stores them as cited\n"
                "evidence, and lets an AI summarise them into sourced reports.\n"
                "Facts come from the data, not the model.",
                markup=False,
                classes="muted",
            )
            # Generated from app.BINDINGS so this list cannot drift from the footer.
            for binding in self.app.BINDINGS:
                if not binding.show:
                    continue
                with Horizontal(classes="help-row"):
                    yield KeyHint(
                        KEY_DISPLAY.get(binding.key, binding.key_display or binding.key)
                    )
                    yield Static(
                        f"{binding.description}"
                        + (f" — {binding.tooltip}" if binding.tooltip else ""),
                        markup=False,
                    )
