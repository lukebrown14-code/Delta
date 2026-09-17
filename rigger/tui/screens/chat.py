"""Chat screen: grounded Q&A over the evidence pool with an opt-in web toggle."""

from __future__ import annotations

from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Input, SelectionList, Static, Switch

from rigger import services
from rigger.chat import ChatMessage, chat
from rigger.core.ids import make_instrument_id
from rigger.targets import WatchTarget
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Pane, PaneRow, PaneStack, Pill


class Chat(RiggerScreen):
    """Standalone chat surface: pick targets, ask, watch the cited answer arrive."""

    name = "chat"
    # No AUTO_FOCUS here: the input would swallow the global single-letter
    # navigation keys (1..6, c, w) before they can reach the app bindings.

    CSS = """
    #chat-split {
        height: 1fr;
    }
    #chat-stack {
        width: 34;
        border-left: solid $panel;
        padding-left: 1;
    }
    #chat-targets-pane {
        height: 1fr;
    }
    #chat-options-pane {
        height: auto;
    }
    #chat-targets {
        height: 1fr;
    }
    #chat-web-row {
        height: auto;
    }
    #chat-main {
        width: 1fr;
        margin-right: 1;
    }
    #chat-scroll {
        height: 1fr;
    }
    #chat-input {
        margin: 1 0 0 0;
    }
    .msg {
        height: auto;
        margin: 0 0 1 0;
        padding: 0 0 0 1;
    }
    .msg-user {
        border-left: solid $panel;
        color: $text-muted;
    }
    .msg-assistant {
        border-left: thick $primary;
    }
    .msg-head {
        height: 1;
        color: $text-primary;
        text-style: bold;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self.history: list[ChatMessage] = []

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="chat-split"):
            with Pane(title="chat", icon="", id="chat-main"):
                yield VerticalScroll(id="chat-scroll")
                yield Input(placeholder="Ask about the selected targets...", id="chat-input")
            with PaneStack(id="chat-stack"):
                with Pane(title="targets", icon="", id="chat-targets-pane"):
                    yield SelectionList(id="chat-targets")
                with Pane(title="options", icon="", id="chat-options-pane", classes="-auto"):
                    with Horizontal(id="chat-web-row"):
                        yield Static("allow web search", markup=False, classes="muted")
                        yield Switch(id="chat-web")

    async def on_mount(self) -> None:
        selections = self.query_one("#chat-targets", SelectionList)
        specs = sorted(services.target_specs().values(), key=lambda t: t.id)
        for target in specs:
            selections.add_option((f"{target.id} ({target.kind})", target.id, False))
        self.query_one("#chat-targets-pane", Pane).set_badge(str(len(specs)))
        await self._render_transcript()

    async def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id != "chat-input":
            return
        text = event.value.strip()
        if not text:
            return
        event.input.value = ""
        self.history.append(ChatMessage(role="user", text=text, source="user"))
        await self._render_transcript()
        self.run_worker(self._answer(), exclusive=True)

    async def _answer(self) -> None:
        try:
            reply = await chat(
                self.rig,
                self.history,
                targets=self._selected_targets(),
                allow_web=self.query_one("#chat-web", Switch).value,
            )
        except Exception as exc:
            self.notify(f"chat failed: {exc}", severity="error")
            return
        self.history.append(reply)
        await self._render_transcript()

    def _selected_targets(self) -> list[str]:
        """Expand the selected watch targets into evidence target ids (market:symbol)."""
        specs = services.target_specs()
        ids: list[str] = []
        for value in self.query_one("#chat-targets", SelectionList).selected:
            target = specs.get(str(value))
            if isinstance(target, WatchTarget):
                for market in target.markets:
                    ids.extend(make_instrument_id(market.upper(), t) for t in target.tickers)
        return list(dict.fromkeys(ids))

    async def _render_transcript(self) -> None:
        scroll = self.query_one("#chat-scroll", VerticalScroll)
        await scroll.remove_children()
        if not self.history:
            await scroll.mount(
                Static("No messages yet: pick targets and ask below.", classes="muted")
            )
            self.query_one("#chat-main", Pane).set_badge("")
            return
        for message in self.history:
            user = message.role == "user"
            title = "You" if user else f"Assistant ({message.source})"
            body: list[Any] = [
                Static(title, markup=False, classes="msg-head"),
                Static(message.text, markup=False),
            ]
            if message.citations:
                pills = [Pill(f"[{n + 1}]", variant="dim") for n in range(len(message.citations))]
                body.append(Horizontal(*pills, classes="check-row"))
            await scroll.mount(
                Vertical(*body, classes=f"msg {'msg-user' if user else 'msg-assistant'}")
            )
        self.query_one("#chat-main", Pane).set_badge(f"{len(self.history)} messages")
        scroll.scroll_end(animate=False)
