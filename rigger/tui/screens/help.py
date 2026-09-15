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
                "  Rigger gathers market data, builds a facts-only brief per\n"
                "  instrument, asks an LLM for evidence-backed trade signals,\n"
                "  paper-trades them with realistic fees and slippage, and\n"
                "  scores every decision against real outcomes.\n"
                "  Facts come from the data, not the model; every signal must\n"
                "  cite the evidence behind it. The system is paper-trading only;\n"
                "  no live trades are placed.\n\n"
                "Navigation\n"
                "  1  Home\n"
                "  2  Signals\n"
                "  3  Portfolio\n"
                "  4  Pipeline\n"
                "  5  Data & Costs\n"
                "  6  Config\n\n"
                "Actions\n"
                "  b  Show the selected signal's brief\n"
                "  r  Reset the paper portfolio from Portfolio\n"
                "  ?  Show this help\n"
                "  q  Quit\n\n"
                "Pipeline runs use the same services as the CLI."
            )
        )
