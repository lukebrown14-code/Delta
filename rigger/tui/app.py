"""Textual App: screen registry, key bindings, service wiring."""

from __future__ import annotations

from typing import Any

from textual.app import App
from textual.binding import Binding

from rigger import services
from rigger.runtime import Rigger
from rigger.tui.screens.config import Config
from rigger.tui.screens.data import Data
from rigger.tui.screens.help import HelpScreen
from rigger.tui.screens.home import Home
from rigger.tui.screens.pipeline import Pipeline
from rigger.tui.screens.portfolio import Portfolio
from rigger.tui.screens.signals import Signals


class RiggerApp(App):
    TITLE = "Rigger"
    CSS_PATH = "rigger.tcss"
    BINDINGS = [
        Binding("1", "switch_screen('home')", "Home"),
        Binding("2", "switch_screen('signals')", "Signals"),
        Binding("3", "switch_screen('portfolio')", "Portfolio"),
        Binding("4", "switch_screen('pipeline')", "Pipeline"),
        Binding("5", "switch_screen('data')", "Data"),
        Binding("6", "switch_screen('config')", "Config"),
        Binding("question_mark", "show_help", "Help"),
        Binding("q", "quit", "Quit"),
    ]

    def __init__(self, rig: Rigger | None = None) -> None:
        super().__init__()
        self.rig = rig if rig is not None else Rigger()
        self._screens: dict[str, Any] = {}
        self.services = services
        self.log_lines: list[str] = []

    def on_mount(self) -> None:
        self._screens = {
            "home": Home(self.rig),
            "signals": Signals(self.rig),
            "portfolio": Portfolio(self.rig),
            "pipeline": Pipeline(self.rig),
            "data": Data(self.rig),
            "config": Config(self.rig),
        }
        for screen in self._screens.values():
            self.install_screen(screen, screen.name)
        self.push_screen("home")

    def action_switch_screen(self, name: str) -> None:
        self.switch_screen(name)

    def action_show_help(self) -> None:
        if self.screen.name == "help":
            self.pop_screen()
        else:
            self.push_screen(HelpScreen())


def run_tui(rig: Rigger | None = None) -> None:
    app = RiggerApp(rig)
    app.run()
