"""Textual App: screen registry, key bindings, service wiring."""

from __future__ import annotations

from typing import Any

from textual.app import App
from textual.binding import Binding

from rigger import services
from rigger.core import config as config_mod
from rigger.llm.catalog import ModelInfo, set_llm_route
from rigger.runtime import Rigger
from rigger.tui.screens.chat import Chat
from rigger.tui.screens.config import Config
from rigger.tui.screens.console import Console
from rigger.tui.screens.data import Data
from rigger.tui.screens.help import HelpScreen
from rigger.tui.screens.home import Home
from rigger.tui.screens.model_picker import ModelPicker
from rigger.tui.screens.reports import Reports
from rigger.tui.screens.targets import Targets
from rigger.tui.screens.theses import Theses


class RiggerApp(App):
    TITLE = "Rigger"
    CSS_PATH = "rigger.tcss"
    BINDINGS = [
        Binding("1", "switch_screen('home')", "Home"),
        Binding("2", "switch_screen('data')", "Data"),
        Binding("3", "switch_screen('config')", "Config"),
        Binding("4", "switch_screen('reports')", "Reports"),
        Binding("5", "switch_screen('theses')", "Theses"),
        Binding("6", "switch_screen('chat')", "Chat"),
        Binding("c", "switch_screen('console')", "Console"),
        Binding("w", "switch_screen('targets')", "Targets"),
        Binding("m", "show_model_picker", "Model"),
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
            "data": Data(self.rig),
            "config": Config(self.rig),
            "reports": Reports(self.rig),
            "theses": Theses(self.rig),
            "chat": Chat(self.rig),
            "console": Console(self.rig),
            "targets": Targets(self.rig),
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

    def action_show_model_picker(self) -> None:
        task = {"reports": "report", "chat": "chat", "theses": "thesis"}.get(
            self.screen.name or "", "extract"
        )
        provider = getattr(getattr(self.rig, "llm", None), "provider", None)

        def on_select(model: ModelInfo) -> None:
            set_llm_route(task, model.id)
            self.notify(f"{task} route set to {model.id}")
            if isinstance(self.rig, Rigger):
                self.rig.settings, self.rig.cfg = config_mod.load_config()

        self.push_screen(
            ModelPicker(on_select, provider=provider, provider_name=self.rig.cfg.llm_provider)
        )


def run_tui(rig: Rigger | None = None) -> None:
    app = RiggerApp(rig)
    app.run()
