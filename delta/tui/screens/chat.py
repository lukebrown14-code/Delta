"""Ask screen: grounded Q&A over the evidence pool with an opt-in web toggle.

Two boxes: the transcript with its prompt on the left, the scope (targets)
and options on the right. Every key acts from the transcript; while the
prompt has focus letters type, and ``esc`` steps back out. Below 100 columns
the right stack folds away behind ``t``, which opens it full width.
"""

from __future__ import annotations

import time
from contextlib import suppress
from dataclasses import dataclass
from datetime import datetime
from typing import Any
from urllib.parse import urlsplit

from rich.text import Text
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.markup import escape
from textual.widgets import Input, Static

from delta import services
from delta.chat import ChatMessage, chat
from delta.core.ids import make_instrument_id
from delta.evidence import evidence, evidence_by_ids
from delta.llm.router import model_for
from delta.targets import WatchTarget
from delta.tui.components import goto, thesis_from_citations
from delta.tui.shell import DeltaScreen
from delta.tui.widgets import (
    DeltaTable,
    Pane,
    PaneRow,
    PaneStack,
    hint_markup,
    token_color,
)

#: Evidence kind -> theme token, mirroring the Research pane so citations read
#: the same everywhere.
KIND_TOKENS = {
    "news": "kind-news",
    "filing": "kind-filing",
    "event": "kind-event",
    "fundamental": "kind-fundamental",
    "bar": "kind-price",
}
KIND_LABELS = {"bar": "price", "fundamental": "fundam."}

#: ``delta.chat`` offers the model this many items per instrument, so the
#: "evidence in scope" figure counts what the model will actually see.
EVIDENCE_CAP = 50

_WEB = ("http://", "https://")


@dataclass
class TurnMeta:
    """What the screen measured about a turn; the message model carries none of it."""

    at: datetime
    seconds: float | None = None
    cost: float | None = None


class CiteRow(Static):
    """The citation-pill row under an answer; remembers which turn it belongs to."""

    def __init__(self, turn: int, markup: str) -> None:
        super().__init__(markup, classes="msg-cites")
        self.turn = turn


class Chat(DeltaScreen):
    """Standalone chat surface: pick targets, ask, watch the cited answer arrive."""

    name = "chat"
    # No AUTO_FOCUS here: the input would swallow the global single-letter
    # navigation keys (1..5, c, h, m, p, g, q) before they reach the app.

    #: Keys dodge the app-level bindings (1-5, c, h, m, p, g, q, ?, f2, ^p).
    #: While the Input has focus it consumes letters, space and enter first,
    #: so none of these fire mid-sentence.
    BINDINGS = [
        ("i", "focus_input", "ask"),
        # enter does the same thing from the transcript; hidden so the keymap
        # does not list one action twice.
        Binding("enter", "focus_input", "ask", show=False),
        ("escape", "back", "back"),
        ("t", "focus_targets", "targets"),
        ("space", "toggle_target", "toggle target"),
        ("a", "toggle_all", "all / none"),
        ("z", "zoom", "zoom"),
        ("x", "clear_transcript", "clear"),
        Binding("y", "confirm_clear", "confirm", show=False),
        Binding("left", "prev_citation", "citation", show=False),
        Binding("right", "next_citation", "citation", show=False),
        ("o", "open_citation", "open citation"),
        ("s", "save_answer", "save to thesis"),
    ]

    CSS = """
    #chat-split { height: 1fr; }
    #chat-main { width: 1fr; }
    #chat-stack { width: 36; }
    Chat.-narrow #chat-stack { width: 1fr; }
    #chat-scope { height: 1; padding: 0 1; color: $text-muted; }
    #chat-scroll { height: 1fr; }
    #chat-prompt { height: 2; border-top: solid $panel; }
    #chat-prompt .prompt-mark { width: 3; height: 1; padding: 0 1; color: $text-primary; }
    #chat-input {
        width: 1fr;
        height: 1;
        border: none;
        padding: 0;
        background: transparent;
    }
    #chat-input:focus { border: none; background: transparent; }
    #chat-targets-pane { height: 1fr; }
    #chat-targets { height: 1fr; }
    #chat-scope-count { height: 1; padding: 0 1; color: $text-muted; }
    #chat-citations-pane { height: 1fr; }
    #chat-citations { height: 1fr; }
    #chat-session { height: 1; padding: 0 1; color: $text-muted; }
    .msg { height: auto; margin: 0 0 1 0; padding: 0 0 0 1; }
    .msg-user { border-left: solid $panel; color: $text-muted; }
    .msg-assistant { border-left: thick $primary; }
    .msg-head, .msg-meta, .msg-cites { height: auto; }
    .msg-meta { color: $text-muted; }
    .msg-empty { height: auto; padding: 0 1; color: $text-muted; }
    """

    def __init__(self, delta: Any) -> None:
        super().__init__(delta)
        self.history: list[ChatMessage] = []
        self.meta: dict[int, TurnMeta] = {}
        self.scope: set[str] = set()
        self.picker_open = False
        self.confirm_pending = False
        self.selected_answer = -1
        self.selected_citation = 0
        self.busy_since: float | None = None
        self.session_cost = 0.0
        self.ready = False
        self.zoomed = False

    # ------------------------------------------------------------ compose

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="chat-split"):
            with Pane(title="ask", key="5", id="chat-main"):
                yield Static("", id="chat-scope", markup=False)
                yield VerticalScroll(id="chat-scroll")
                with Horizontal(id="chat-prompt"):
                    yield Static(">", classes="prompt-mark", markup=False)
                    yield Input(placeholder="ask about the targets in scope…", id="chat-input")
            with PaneStack(id="chat-stack"):
                with Pane(
                    title="targets",
                    key="t",
                    hints=hint_markup(("space", "toggle"), ("a", "all"), ("enter", "ask")),
                    id="chat-targets-pane",
                ):
                    yield DeltaTable(id="chat-targets")
                    yield Static("", id="chat-scope-count", markup=False)
                with Pane(
                    title="citations",
                    key="o",
                    hints=hint_markup(("←→", "citation"), ("o", "open"), ("s", "save")),
                    id="chat-citations-pane",
                ):
                    yield DeltaTable(id="chat-citations")
                    yield Static("", id="chat-session", markup=False)

    async def on_mount(self) -> None:
        table = self.query_one("#chat-targets", DeltaTable)
        table.add_column("", key="dot")
        table.add_column("Target", key="target")
        table.add_column("Kind", key="kind")
        table.add_column("Evidence", key="evidence")
        cites = self.query_one("#chat-citations", DeltaTable)
        cites.add_column("#", key="n", width=3)
        cites.add_column("Evidence", key="evidence")
        cites.add_column("Kind", key="kind", width=11)
        self.scope = set(services.target_specs())
        self.ready = True
        self.refresh_targets()
        self.render_citations()
        self.layout_views()
        self.set_interval(1, self.tick)
        await self._render_transcript()

    def refresh_view(self) -> None:
        """Targets may have changed on another screen since the last visit."""
        if self.ready:
            self.refresh_targets()
            self.render_citations()

    # ------------------------------------------------------------ targets

    def specs(self) -> list[WatchTarget]:
        return sorted(services.target_specs().values(), key=lambda t: t.id)

    def instrument_ids(self, target: WatchTarget) -> list[str]:
        return list(
            dict.fromkeys(
                make_instrument_id(market.upper(), ticker)
                for market in target.markets
                for ticker in target.tickers
            )
        )

    def evidence_count(self, ids: list[str]) -> tuple[int, bool]:
        """Evidence the model would be offered for ``ids``; ``True`` when a cap was hit."""
        total, capped = 0, False
        for instrument in ids:
            n = len(evidence(self.delta.engine, target=instrument, limit=EVIDENCE_CAP))
            capped = capped or n >= EVIDENCE_CAP
            total += n
        return total, capped

    def refresh_targets(self) -> None:
        specs = self.specs()
        self.scope &= {spec.id for spec in specs}
        table = self.query_one("#chat-targets", DeltaTable)
        cursor = table.cursor_row
        with self.prevent(DeltaTable.RowHighlighted):
            table.clear()
            for spec in specs:
                count, capped = self.evidence_count(self.instrument_ids(spec))
                table.add_row(
                    self.dot(spec.id in self.scope),
                    spec.id,
                    spec.kind,
                    f"{count}+" if capped else (str(count) if count else "—"),
                    key=spec.id,
                )
            if specs:
                table.move_cursor(row=min(cursor, len(specs) - 1))
        self.query_one("#chat-targets-pane", Pane).set_badge(str(len(specs)))
        self.render_scope()

    def dot(self, on: bool) -> Text:
        tokens = self.app.theme_variables
        if on:
            return Text("●", style=tokens["text-success"])
        return Text("○", style=tokens["text-muted"])

    def scoped_targets(self) -> list[WatchTarget]:
        return [spec for spec in self.specs() if spec.id in self.scope]

    def _selected_targets(self) -> list[str]:
        """Expand the in-scope watch targets into evidence target ids (market:symbol)."""
        ids: list[str] = []
        for spec in self.scoped_targets():
            ids.extend(self.instrument_ids(spec))
        return list(dict.fromkeys(ids))

    def render_scope(self) -> None:
        specs = self.specs()
        scoped = self.scoped_targets()
        ids = self._selected_targets()
        names = " ".join(spec.id for spec in scoped) or "none"
        narrow = self.has_class("-narrow")
        if narrow:
            line = " ".join(part for part in ("scope:", names, " ".join(ids)) if part)
        else:
            count, capped = self.evidence_count(ids)
            pool = f"{count}{'+' if capped else ''} evidence items"
            line = f"scope: {names} → {' '.join(ids) or '—'} · {pool}"
        self.query_one("#chat-scope", Static).update(line)
        count_line = f"{len(scoped)} of {len(specs)} in scope"
        if ids and not narrow:
            count_line += f" · {' '.join(ids)}"
        self.query_one("#chat-scope-count", Static).update(count_line)

    def cursor_target(self) -> str | None:
        table = self.query_one("#chat-targets", DeltaTable)
        if table.row_count == 0:
            return None
        try:
            return str(table.coordinate_to_cell_key(table.cursor_coordinate).row_key.value)
        except Exception:
            return None

    def toggle_target(self, target: str) -> None:
        if target in self.scope:
            self.scope.discard(target)
        else:
            self.scope.add(target)
        self.query_one("#chat-targets", DeltaTable).update_cell(
            target, "dot", self.dot(target in self.scope)
        )
        self.render_scope()

    def action_toggle_target(self) -> None:
        target = self.cursor_target()
        if target is not None:
            self.toggle_target(target)

    def action_toggle_all(self) -> None:
        specs = self.specs()
        self.scope = set() if len(self.scope) == len(specs) else {spec.id for spec in specs}
        table = self.query_one("#chat-targets", DeltaTable)
        for spec in specs:
            table.update_cell(spec.id, "dot", self.dot(spec.id in self.scope))
        self.render_scope()

    def action_focus_targets(self) -> None:
        self.cancel_confirm()
        if self.has_class("-narrow"):
            self.picker_open = True
            self.layout_views()
        self.query_one("#chat-targets", DeltaTable).focus()

    # ------------------------------------------------------------ citations sidebar

    def model_name(self) -> str:
        try:
            return model_for(self.delta.cfg, "chat")
        except (KeyError, AttributeError, TypeError):
            return "—"

    def provider_name(self) -> str:
        return str(getattr(self.delta.cfg, "llm_provider", "") or "—")

    def render_citations(self) -> None:
        """Populate the citations sidebar: the evidence behind the highlighted answer."""
        table = self.query_one("#chat-citations", DeltaTable)
        with self.prevent(DeltaTable.RowHighlighted):
            table.clear()
            for n, citation in enumerate(self.citations()):
                table.add_row(
                    str(n + 1),
                    self._citation_label(citation),
                    self._kind_cell(citation),
                    key=citation,
                )
        answers = sum(1 for m in self.history if m.role == "assistant")
        plural = "s" if answers != 1 else ""
        self.query_one("#chat-session", Static).update(
            f"this session: {answers} answer{plural} · ${self.session_cost:.3f}"
        )
        self._place_citation_cursor()

    def _citation_label(self, citation: str) -> str:
        """Short evidence label for the sidebar: title · date, or the host for a url."""
        if citation.startswith(_WEB):
            return urlsplit(citation).netloc or citation
        items = evidence_by_ids(self.delta.engine, [citation])
        if items:
            item = items[0]
            return f"{item.title} · {item.ts:%-d %b}"
        return citation

    def _kind_cell(self, citation: str) -> Text:
        """Kind cell for a citation, colouring by evidence kind like the Research pane."""
        if citation.startswith(_WEB):
            return Text("web", style=token_color(self.app, "text-muted"))
        items = evidence_by_ids(self.delta.engine, [citation])
        kind = items[0].kind if items else "other"
        label = KIND_LABELS.get(kind, kind)
        return Text(label, style=token_color(self.app, KIND_TOKENS.get(kind, "text-muted")))

    def _place_citation_cursor(self) -> None:
        table = self.query_one("#chat-citations", DeltaTable)
        cites = self.citations()
        if not cites:
            return
        index = min(self.selected_citation, len(cites) - 1)
        with suppress(Exception):
            with self.prevent(DeltaTable.RowHighlighted):
                table.move_cursor(row=index)

    def on_data_table_row_selected(self, event: DeltaTable.RowSelected) -> None:
        if event.data_table.id == "chat-targets":
            self.action_focus_input()
        elif event.data_table.id == "chat-citations":
            # Enter on a citation jumps to it in Research, matching ``o``.
            self.action_open_citation()

    # ------------------------------------------------------------ clear

    def action_clear_transcript(self) -> None:
        if not self.history:
            return
        self.confirm_pending = True
        self.render_hints()

    async def action_confirm_clear(self) -> None:
        if not self.confirm_pending:
            return
        self.confirm_pending = False
        self.history.clear()
        self.meta.clear()
        self.selected_answer = -1
        self.selected_citation = 0
        await self._render_transcript()

    def cancel_confirm(self) -> None:
        if self.confirm_pending:
            self.confirm_pending = False
            self.render_hints()

    # ------------------------------------------------------------ focus / layout

    def action_focus_input(self) -> None:
        self.cancel_confirm()
        if self.picker_open:
            self.picker_open = False
            self.layout_views()
        self.query_one("#chat-input", Input).focus()

    def action_back(self) -> None:
        if self.confirm_pending:
            self.cancel_confirm()
        elif self.query_one("#chat-input", Input).has_focus:
            self.query_one("#chat-scroll", VerticalScroll).focus()
        elif self.picker_open:
            self.picker_open = False
            self.layout_views()
            self.query_one("#chat-scroll", VerticalScroll).focus()

    def action_zoom(self) -> None:
        """Maximise the focused pane (``z`` toggles back).

        The transcript and the stack fight for width on a wide terminal, and
        on a narrow one they share the single column. ``z`` gives the focused
        half the whole surface, tmux-style.
        """
        if not self.ready:
            return
        self.zoomed = not self.zoomed
        self.layout_views()

    def _focused_side(self) -> str:
        """Which side of the split has focus: the transcript or the stack."""
        focused = self.focused
        if focused is not None and self.query_one("#chat-stack") in focused.ancestors_with_self:
            return "stack"
        return "main"

    def layout_views(self) -> None:
        """Wide: transcript and stack. Narrow: the transcript, or the picker after ``t``."""
        narrow = self.apply_breakpoint()
        if not narrow:
            self.picker_open = False
        if self.zoomed:
            side = self._focused_side()
            self.query_one("#chat-main").display = side == "main"
            self.query_one("#chat-stack").display = side == "stack"
        else:
            self.query_one("#chat-main").display = not (narrow and self.picker_open)
            self.query_one("#chat-stack").display = not narrow or self.picker_open
        self.query_one("#chat-targets-pane", Pane).set_hints(
            hint_markup(("space", "toggle"), ("a", "all"), ("esc", "back to ask"))
            if narrow
            else hint_markup(("space", "toggle"), ("a", "all"), ("enter", "ask"))
        )
        self.query_one("#chat-citations-pane", Pane).set_hints(
            hint_markup(("←→", "citation"), ("o", "open"), ("s", "save"))
        )
        self.render_scope()
        self.render_hints()

    def on_resize(self) -> None:
        if self.ready:
            self.layout_views()

    def on_descendant_focus(self) -> None:
        if self.ready:
            self.render_hints()

    def on_descendant_blur(self) -> None:
        if self.ready:
            self.render_hints()

    def render_hints(self) -> None:
        pane = self.query_one("#chat-main", Pane)
        if self.confirm_pending:
            pane.set_hints(hint_markup(("y", "confirm clear"), ("esc", "cancel")))
            return
        typing = self.query_one("#chat-input", Input).has_focus
        narrow = self.has_class("-narrow")
        if typing:
            hints = [("enter", "send"), ("esc", "leave input")]
            if not narrow:
                hints.append(("↑↓", "scroll"))
            pane.set_hints(hint_markup(*hints))
            return
        hints = [("i", "ask")]
        if narrow:
            hints += [("t", "targets")]
        else:
            hints.append(("↑↓", "scroll"))
        if self.citations():
            hints += [("←→", "citation"), ("o", "open citation")]
        if self.answer() is not None:
            hints.append(("s", "save"))
        if self.history:
            hints.append(("x", "clear"))
        hints.append(("z", "zoom"))
        pane.set_hints(hint_markup(*hints))

    # ------------------------------------------------------------ ask

    async def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id != "chat-input":
            return
        text = event.value.strip()
        if not text:
            return
        event.input.value = ""
        self.history.append(ChatMessage(role="user", text=text, source="user"))
        self.meta[len(self.history) - 1] = TurnMeta(at=datetime.now())
        self.busy_since = time.monotonic()
        await self._render_transcript()
        self.run_worker(self._answer(), exclusive=True)

    def spend(self) -> float:
        return services.total_spend(self.delta.engine)

    async def _answer(self) -> None:
        started = time.monotonic()
        before = self.spend()
        try:
            reply = await chat(
                self.delta,
                self.history,
                targets=self._selected_targets(),
            )
        except Exception as exc:
            self.notify(f"ask failed: {exc} — press i to try again", severity="error")
            self.busy_since = None
            await self._render_transcript()
            return
        cost = self.spend() - before
        self.session_cost += cost
        self.history.append(reply)
        self.meta[len(self.history) - 1] = TurnMeta(
            at=datetime.now(), seconds=time.monotonic() - started, cost=cost
        )
        self.selected_answer = len(self.history) - 1
        self.selected_citation = 0
        self.busy_since = None
        await self._render_transcript()

    def tick(self) -> None:
        if self.busy_since is None:
            return
        for pending in self.query("#chat-pending"):
            if isinstance(pending, Static):
                pending.update(self.pending_markup())

    def pending_markup(self) -> str:
        elapsed = int(time.monotonic() - (self.busy_since or time.monotonic()))
        return (
            f"[bold $text-primary]assistant[/] [$text-muted]· {escape(self.provider_name())} · "
            f"{escape(self.model_name())} ·[/] [$text-warning]● answering… {elapsed}s[/]"
        )

    # ------------------------------------------------------------ citations

    def answer(self) -> ChatMessage | None:
        if 0 <= self.selected_answer < len(self.history):
            message = self.history[self.selected_answer]
            if message.role == "assistant":
                return message
        return None

    def citations(self) -> tuple[str, ...]:
        message = self.answer()
        return message.citations if message else ()

    def action_next_citation(self) -> None:
        self.walk_citation(1)

    def action_prev_citation(self) -> None:
        self.walk_citation(-1)

    def walk_citation(self, step: int) -> None:
        cites = self.citations()
        if not cites:
            return
        self.selected_citation = (self.selected_citation + step) % len(cites)
        for row in self.query(CiteRow):
            if row.turn == self.selected_answer:
                row.update(self.citation_markup(self.selected_answer))
        self._place_citation_cursor()
        self.render_hints()

    def citation_labels(self, citations: tuple[str, ...]) -> list[str]:
        """Short labels: ``title · date`` for stored items, the host for web urls."""
        stored = {
            item.id: item
            for item in evidence_by_ids(
                self.delta.engine, [c for c in citations if not c.startswith(_WEB)]
            )
        }
        labels = []
        for citation in citations:
            if citation.startswith(_WEB):
                labels.append(urlsplit(citation).netloc or citation)
            elif citation in stored:
                item = stored[citation]
                labels.append(f"{item.title} · {item.ts:%-d %b}")
            else:
                labels.append(citation)
        return labels

    def citation_markup(self, index: int) -> str:
        labels = self.citation_labels(self.history[index].citations)
        parts = []
        for n, label in enumerate(labels):
            chosen = index == self.selected_answer and n == self.selected_citation
            style = "$block-cursor-foreground on $primary" if chosen else "$text-muted on $panel"
            parts.append(f"[{style}] [bold]\\[{n + 1}][/bold] {escape(label)} [/]")
        return " ".join(parts)

    async def action_open_citation(self) -> None:
        cites = self.citations()
        if not cites:
            return
        citation = cites[self.selected_citation % len(cites)]
        if citation.startswith(_WEB):
            self.app.open_url(citation)
            return
        research = getattr(self.app, "screens_by_name", {}).get("data")
        inspect = getattr(research, "inspect_evidence", None)
        if not callable(inspect) or not goto(self.app, "data"):
            self.notify("the research panel is not available here", severity="warning")
            return
        await inspect(citation)

    # ------------------------------------------------------------ save

    def action_save_answer(self) -> None:
        """Turn the selected answer into a thesis, carrying its stored citations across."""
        message = self.answer()
        if message is None:
            self.notify("no answer to save yet — press i to ask something", severity="warning")
            return
        stored = [c for c in message.citations if not c.startswith(_WEB)]
        claim = message.text.split("\n\n")[0]
        thesis_from_citations(self, claim, self._selected_targets(), stored, "from ask")

    # ------------------------------------------------------------ transcript

    async def _render_transcript(self) -> None:
        scroll = self.query_one("#chat-scroll", VerticalScroll)
        await scroll.remove_children()
        blocks: list[Vertical | Static] = [
            self.turn_block(index, message) for index, message in enumerate(self.history)
        ]
        if self.busy_since is not None:
            blocks.append(
                Vertical(
                    Static(self.pending_markup(), id="chat-pending", classes="msg-head"),
                    classes="msg msg-assistant",
                )
            )
        if not blocks:
            blocks.append(
                Static(
                    "no messages yet — press t to pick targets, then i to ask",
                    classes="msg-empty",
                    markup=False,
                )
            )
        await scroll.mount_all(blocks)
        count = len(self.history)
        self.query_one("#chat-main", Pane).set_badge(
            f"{count} message{'s' if count != 1 else ''}" if count else ""
        )
        self.render_citations()
        self.render_hints()
        scroll.scroll_end(animate=False)

    def turn_block(self, index: int, message: ChatMessage) -> Vertical:
        meta = self.meta.get(index)
        stamp = f"{meta.at:%H:%M}" if meta else ""
        user = message.role == "user"
        if user:
            tail = f" · {stamp}" if stamp else ""
            head = f"[bold $text-muted]you[/][$text-muted]{tail}[/]"
        else:
            bits = [self.provider_name(), self.model_name()]
            if stamp:
                bits.append(stamp)
            if meta and meta.seconds is not None:
                bits.append(f"{meta.seconds:.1f}s")
            head = f"[bold $text-primary]assistant[/] [$text-muted]· {escape(' · '.join(bits))}[/]"
        body: list[Any] = [
            Static(head, classes="msg-head"),
            Static(message.text, markup=False),
        ]
        if not user:
            if message.citations:
                body.append(CiteRow(index, self.citation_markup(index)))
            n = len(message.citations)
            parts = [f"{n} citation{'s' if n != 1 else ''}", message.source]
            if meta and meta.cost:
                parts.append(f"${meta.cost:.3f}")
            body.append(Static(" · ".join(parts), classes="msg-meta", markup=False))
        return Vertical(*body, classes=f"msg {'msg-user' if user else 'msg-assistant'}")
