"""Textual App: screen registry, key bindings, service wiring."""

from __future__ import annotations

from typing import Any

from textual.app import App, ComposeResult
from textual.binding import Binding
from textual.command import Hit, Hits, Provider

from delta import services
from delta.core import state
from delta.llm.catalog import ModelInfo, set_llm_route
from delta.runtime import Delta
from delta.tui.screens.chat import Chat
from delta.tui.screens.config import Config
from delta.tui.screens.data import Data
from delta.tui.screens.decisions import Decisions
from delta.tui.screens.help import HelpScreen
from delta.tui.screens.home import Home
from delta.tui.screens.model_picker import ModelPicker
from delta.tui.screens.research import ResearchState
from delta.tui.screens.targets import Targets
from delta.tui.screens.theses import Theses
from delta.tui.shell import ALL_ITEMS
from delta.tui.theme import THEMES
from delta.tui.widgets import MODAL_WIDTH, Dialog, KeyGrid, PaneRow, hint_markup


async def _gather(delta: Any, app: App) -> None:
    """Command-palette action: ingest then extract, with a toast result."""
    ingested = await services.ingest(delta)
    extracted = await services.extract(delta)
    rows = sum(ingested.counts.values())
    app.notify(f"Gathered {rows} rows, {extracted.events} events")


class DeltaCommands(Provider):
    """Command palette: jump to any screen, gather evidence, flip theme."""

    def __init__(self, screen: Any, match_style: Any = None) -> None:
        super().__init__(screen, match_style)

    async def search(self, query: str) -> Hits:
        matcher = self.matcher(query)
        app = self.app
        delta = getattr(self.screen, "delta", None)
        for key, name, label in ALL_ITEMS:
            text = f"Go to {label}"
            score = matcher.match(text)
            if score > 0:
                yield Hit(
                    score,
                    matcher.highlight(text),
                    lambda n=name: app.action_switch_screen(n),
                    text,
                    f"shortcut: {key}",
                )
        text = "Gather evidence"
        score = matcher.match(text)
        if score > 0 and delta is not None:

            async def gather() -> None:
                await _gather(delta, app)

            yield Hit(score, matcher.highlight(text), gather, text, "ingest + extract")
        text = "Toggle light/dark theme"
        score = matcher.match(text)
        if score > 0:
            yield Hit(
                score,
                matcher.highlight(text),
                lambda: app.action_toggle_theme(),
                text,
            )


class GoPicker(Dialog):
    """Centred keymap of every panel: the replacement for the nav rail."""

    dialog_title = "go"
    dialog_hint = hint_markup(("key", "open"), ("esc", "exit"))
    dialog_width = MODAL_WIDTH

    def compose_dialog(self) -> ComposeResult:
        yield KeyGrid([(key, label) for key, _name, label in ALL_ITEMS])

    def on_key(self, event: Any) -> None:
        for key, name, _label in ALL_ITEMS:
            if event.key == key:
                event.stop()
                self.dismiss(None)
                self.app.action_switch_screen(name)
                return


class DeltaApp(App):
    TITLE = "Delta"
    CSS_PATH = "delta.tcss"
    BINDINGS = [
        Binding("1", "switch_screen('targets')", "Watchlist", tooltip="Manage what is watched"),
        Binding("2", "switch_screen('data')", "Research", tooltip="Read reports and evidence"),
        Binding("4", "switch_screen('theses')", "Theses", tooltip="Track claims and evidence"),
        Binding("5", "switch_screen('chat')", "Ask", tooltip="Grounded Q&A over evidence"),
        Binding("6", "switch_screen('decisions')", "Decisions", tooltip="Record and review decision context"),
        Binding(
            "c",
            "switch_screen('config')",
            "Settings",
            tooltip="Providers, routing, plugins and diagnostics",
        ),
        Binding("h", "switch_screen('home')", "Home", tooltip="The landing dashboard"),
        Binding("m", "show_model_picker", "Model", tooltip="Pick the model for this screen"),
        Binding(
            "p", "show_provider_picker", "Provider", tooltip="Connect or switch the AI provider"
        ),
        Binding("g", "show_go", "Go", tooltip="Jump to a panel"),
        Binding("question_mark", "show_help", "Help", tooltip="Show the keymap"),
        Binding("q", "quit", "Quit", tooltip="Leave Delta"),
        Binding("f2", "toggle_theme", "Theme", tooltip="Switch light/dark palette"),
    ]
    COMMANDS = App.COMMANDS | {DeltaCommands}

    def __init__(self, delta: Delta | None = None) -> None:
        super().__init__()
        # Register before the first stylesheet parse so the app never paints
        # a frame in Textual's stock theme.
        for theme in THEMES:
            self.register_theme(theme)
        self.theme = "delta-dark"
        self.delta = delta if delta is not None else Delta()
        #: Public registry of the installed screens. Screens that keep each
        #: other in sync read this rather than Textual's private ``_screens``.
        self.screens_by_name: dict[str, Any] = {}
        self._screens: dict[str, Any] = {}
        self.services = services
        self.log_lines: list[str] = []
        self.narrow = False
        # Read once, then stamp this visit immediately: Home renders the whole
        # session against the *previous* value, so re-reading would zero it out.
        self.last_seen = state.read_last_seen(self.delta.cfg)
        state.write_last_seen(self.delta.cfg)

    def on_mount(self) -> None:
        research_state = ResearchState()
        self._screens = {
            "home": Home(self.delta, last_seen=self.last_seen),
            "data": Data(self.delta, research_state),
            "config": Config(self.delta),
            "theses": Theses(self.delta),
            "decisions": Decisions(self.delta, research_state),
            "chat": Chat(self.delta),
            "targets": Targets(self.delta),
        }
        self.screens_by_name = dict(self._screens)
        for screen in self._screens.values():
            self.install_screen(screen, screen.name)
        self.push_screen("home")

    def on_resize(self, event: Any) -> None:
        """Track the narrow breakpoint; each PaneRow stacks itself on resize."""
        self.narrow = event.size.width < PaneRow.NARROW_WIDTH

    def action_switch_screen(self, name: str) -> None:
        self.switch_screen(name)

    def action_toggle_theme(self) -> None:
        self.theme = "delta-light" if self.theme == "delta-dark" else "delta-dark"
        self.notify(f"Theme: {self.theme}")

    def action_show_go(self) -> None:
        self.push_screen(GoPicker())

    def action_show_help(self) -> None:
        if self.screen.name == "help":
            self.pop_screen()
        else:
            self.push_screen(HelpScreen())

    def action_show_model_picker(self, task: str | None = None) -> None:
        """Pick the model for a routing task.

        With no ``task`` the task is derived from the current screen, which is
        what the ``m`` binding wants. Settings passes the task explicitly, so
        it can re-route any row without re-implementing this action.
        """
        task = task or {
            "data": "report",
            "chat": "chat",
            "theses": "thesis",
        }.get(self.screen.name or "", "extract")
        provider = getattr(getattr(self.delta, "llm", None), "provider", None)

        def on_select(model: ModelInfo) -> None:
            set_llm_route(task, model.id)
            self.notify(f"{task} route set to {model.id}")
            if isinstance(self.delta, Delta):
                self.delta.reload_llm()

        self.push_screen(
            ModelPicker(on_select, provider=provider, provider_name=self.delta.cfg.llm_provider)
        )

    def action_show_provider_picker(self) -> None:
        from delta.tui.screens.provider_picker import (
            ProviderPicker,
            connect_provider,
            provider_key_status,
        )

        def on_select(name: str) -> None:
            self.run_worker(connect_provider(self, self.delta, name), exclusive=True)

        self.push_screen(ProviderPicker(on_select, key_status=provider_key_status(self.delta)))


def run_tui(delta: Delta | None = None) -> None:
    app = DeltaApp(delta)
    app.run()
