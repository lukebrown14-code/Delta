"""Chat screen: grounded Q&A over the evidence pool with an opt-in web toggle."""

from __future__ import annotations

from typing import Any

from textual.app import ComposeResult
from textual.containers import VerticalScroll
from textual.widgets import Checkbox, Input, SelectionList, Static

from rigger import services
from rigger.chat import ChatMessage, chat
from rigger.core.ids import make_instrument_id
from rigger.targets import WatchTarget
from rigger.tui.shell import RiggerScreen


class Chat(RiggerScreen):
    """Standalone chat surface: pick targets, ask, watch the cited answer arrive."""

    name = "chat"

    CSS = """
    #chat-targets {
        height: auto;
        max-height: 8;
        margin: 0 2;
    }
    #chat-web {
        margin: 0 2;
    }
    #chat-scroll {
        height: 1fr;
        margin: 0 2;
    }
    #chat-input {
        margin: 1 2;
    }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self.history: list[ChatMessage] = []

    def compose_content(self) -> ComposeResult:
        yield SelectionList(id="chat-targets")
        yield Checkbox("Allow web search (labelled Web, never stored)", id="chat-web")
        yield VerticalScroll(Static(id="chat-transcript"), id="chat-scroll")
        yield Input(placeholder="Ask about the selected targets...", id="chat-input")

    def on_mount(self) -> None:
        selections = self.query_one("#chat-targets", SelectionList)
        for target in sorted(services.target_specs().values(), key=lambda t: t.id):
            selections.add_option((f"{target.id} ({target.kind})", target.id, False))
        self._render_transcript()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id != "chat-input":
            return
        text = event.value.strip()
        if not text:
            return
        event.input.value = ""
        self.history.append(ChatMessage(role="user", text=text, source="user"))
        self._render_transcript()
        self.run_worker(self._answer(), exclusive=True)

    async def _answer(self) -> None:
        try:
            reply = await chat(
                self.rig,
                self.history,
                targets=self._selected_targets(),
                allow_web=self.query_one("#chat-web", Checkbox).value,
            )
        except Exception as exc:
            self.notify(f"chat failed: {exc}", severity="error")
            return
        self.history.append(reply)
        self._render_transcript()

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

    def _render_transcript(self) -> None:
        lines = []
        for message in self.history:
            who = "You" if message.role == "user" else f"Assistant ({message.source})"
            line = f"[bold]{who}[/bold]: {message.text}"
            if message.citations:
                line += f"\n  citations: {', '.join(message.citations)}"
            lines.append(line)
        text = "\n\n".join(lines) if lines else "No messages yet: pick targets and ask below."
        self.query_one("#chat-transcript", Static).update(text)
