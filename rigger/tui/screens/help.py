"""Keyboard help screen."""

from textual.app import ComposeResult
from textual.containers import VerticalScroll
from textual.screen import Screen
from textual.widgets import Static


class HelpScreen(Screen):
    name = "help"

    def compose(self) -> ComposeResult:
        yield VerticalScroll(
            Static(
                "[bold cyan]Rigger help[/bold cyan]\n\n"
                "How it works\n"
                "  Rigger gathers market data and filings, stores them as\n"
                "  cited evidence, and lets an AI summarise them into sourced\n"
                "  reports. Facts come from the data, not the model.\n\n"
                "Navigation\n"
                "  1  Home\n"
                "  2  Data & Costs\n"
                "  3  Config\n"
                "  c  Command console\n"
                "  w  Targets\n\n"
                "Actions\n"
                "  ?  Show this help\n"
                "  q  Quit"
            )
        )
