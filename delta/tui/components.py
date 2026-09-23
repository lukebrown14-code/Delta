"""Shared screen behaviour: live quotes, suggestion dropdowns, guards, states.

``widgets.py`` holds the drawing primitives (panes, chips, charts); this module
holds the behaviour several screens used to re-implement each on their own —
the quote-feed lifecycle, the arrow-browsed dropdown under an input, the
"select a row first" guard, screen switching, empty states, section headings
and turning cited evidence into a thesis.
"""

from __future__ import annotations

import asyncio
from collections.abc import Iterable, Sequence
from contextlib import suppress
from typing import Any

from textual.markup import escape
from textual.widgets import OptionList, Static
from textual.widgets.option_list import Option

from delta import theses
from delta.core.models import Instrument
from delta.plugins.data.yfinance import DEFAULT_SUFFIXES
from delta.quotes import Quote, YahooQuotes

# ----------------------------------------------------------------- quotes


def quote_suffixes(delta: Any) -> dict[str, str]:
    """Yahoo symbol suffix per market: the defaults plus the yfinance config."""
    plugins = getattr(delta.cfg, "plugins", {}) or {}
    return DEFAULT_SUFFIXES | plugins.get("yfinance", {}).get("suffixes", {})


class QuoteFeedMixin:
    """One screen's live Yahoo quote subscription.

    ``sync_quotes`` points the feed at a set of instruments and is cheap to
    call on every refresh: an unchanged set keeps the running socket, a
    changed one swaps it while carrying the quotes already received so rows
    never blank. ``stop_quotes`` cancels and awaits the task; screens call it
    on suspend and unmount so no websocket outlives the view.
    """

    feed: YahooQuotes | None = None
    feed_task: asyncio.Task[None] | None = None
    #: Injected by tests to replace the websocket client.
    quote_client_factory: Any = None
    #: The feed's last reported connection state ("live", "offline", ...).
    quote_state: str = ""
    _feed_signature: tuple[Any, ...] = ()

    delta: Any

    def sync_quotes(self, instruments: Sequence[Instrument]) -> None:
        """Stream ``instruments``, restarting the feed only when they change."""
        suffixes = quote_suffixes(self.delta)
        signature = (
            tuple(sorted(i.id for i in instruments)),
            tuple(sorted(suffixes.items())),
        )
        running = self.feed_task is not None and not self.feed_task.done()
        if running and signature == self._feed_signature:
            return
        old_task = self.feed_task
        if old_task:
            old_task.cancel()
        old_quotes = self.feed.quotes if self.feed else {}
        self._feed_signature = signature
        feed = YahooQuotes(
            list(instruments), suffixes, self._on_quote_state, self.quote_client_factory
        )
        feed.quotes.update({k: v for k, v in old_quotes.items() if k in signature[0]})
        self.feed = feed

        async def start() -> None:
            if old_task:
                with suppress(asyncio.CancelledError):
                    await old_task
            await feed.run()

        try:
            self.feed_task = asyncio.create_task(start())
        except RuntimeError:
            # No running loop (a standalone mount in a test): stay on stored bars.
            self.feed, self.feed_task = None, None

    def quote_for(self, ident: str | None) -> Quote | None:
        """The latest streamed quote for an instrument id, from cache only."""
        return self.feed.quotes.get(ident) if self.feed and ident else None

    async def stop_quotes(self) -> None:
        """Cancel the feed and wait for its socket to close."""
        task, self.feed_task = self.feed_task, None
        self._feed_signature = ()
        if task:
            task.cancel()
            with suppress(asyncio.CancelledError):
                await task

    def _on_quote_state(self, state: str) -> None:
        self.quote_state = state


# ------------------------------------------------------------ suggestions


class SuggestionList(OptionList):
    """The dropdown under an autocomplete input.

    It never takes focus: the input keeps the cursor and its up/down keys
    browse the list (``browse``), wrapping at either end. Enter on the input
    adopts ``highlighted_index``.
    """

    can_focus = False

    def show(self, options: Iterable[Option]) -> None:
        """Replace the suggestions and highlight the first one."""
        self.clear_options()
        self.add_options(list(options))
        if self.option_count:
            self.highlighted = 0

    def browse(self, event: Any) -> bool:
        """Handle an up/down key from the input; True when it was consumed."""
        count = self.option_count
        if not count or event.key not in {"down", "up"}:
            return False
        event.stop()
        event.prevent_default()
        step = 1 if event.key == "down" else -1
        self.highlighted = ((self.highlighted or 0) + step) % count
        return True

    @property
    def highlighted_index(self) -> int:
        """The highlighted row, clamped into range (0 when nothing is shown)."""
        if not self.option_count:
            return 0
        return min(self.highlighted or 0, self.option_count - 1)


# --------------------------------------------------------- guards and nav


def require_selection(screen: Any, value: object, what: str) -> bool:
    """False, with a warning naming ``what``, when nothing is selected."""
    if value:
        return True
    screen.notify(f"select {what} first", severity="warning")
    return False


def goto(app: Any, screen_name: str) -> bool:
    """Switch the app to a named panel; False under a host without panels."""
    switch = getattr(app, "action_switch_screen", None)
    if not callable(switch):
        return False
    switch(screen_name)
    return True


# ---------------------------------------------------------- empty states


class EmptyState(Static):
    """A muted one-liner for an empty pane that names the key that fills it.

    ``EmptyState("no theses yet", key="n", action="create one")`` renders
    ``no theses yet — press n to create one`` with the key in the accent, so
    every empty pane reads the same and always points at a next step.
    """

    DEFAULT_CSS = """
    EmptyState {
        height: auto;
        padding: 0 1;
        color: $text-muted;
    }
    """

    def __init__(
        self,
        message: str = "",
        *,
        key: str = "",
        action: str = "",
        id: str | None = None,
        classes: str = "",
    ) -> None:
        super().__init__(id=id, classes=classes)
        self.set_message(message, key=key, action=action)

    def set_message(self, message: str, *, key: str = "", action: str = "") -> None:
        text = escape(message)
        if key:
            text += f" — press [bold $text-primary]{escape(key)}[/]"
            if action:
                text += f" to {escape(action)}"
        self.update(text)


# ------------------------------------------------------------- headings


class SectionHeading(Static):
    """A section label inside a pane: muted, bold, lowercase like pane titles."""

    DEFAULT_CSS = """
    SectionHeading {
        height: 1;
        margin: 1 0 0 0;
        padding: 0 1;
        color: $text-muted;
        text-style: bold;
    }
    SectionHeading.-first {
        margin-top: 0;
    }
    """

    def __init__(self, text: str, *, id: str | None = None, classes: str = "") -> None:
        super().__init__(text.lower(), id=id, classes=classes, markup=False)


# --------------------------------------------------------------- theses


def thesis_from_citations(
    screen: Any,
    claim: str,
    targets: Sequence[str],
    evidence_ids: Sequence[str],
    note: str,
) -> None:
    """Open the thesis form prefilled with ``claim``; link ``evidence_ids`` on save.

    The evidence is attached accepted, not queued as candidates: it already
    passed the citation contract against the gathered pool, and the reader
    just read it in context.
    """
    from delta.tui.screens.theses import ThesisForm

    def created(fields: dict[str, Any] | None) -> None:
        if fields is None:
            return
        text = fields.pop("claim")
        fields["targets"] = fields["targets"] or tuple(targets)
        try:
            thesis = theses.create_thesis(screen.delta.engine, text, **fields)
        except ValueError as exc:
            screen.notify(str(exc), severity="error")
            return
        for evidence_id in evidence_ids:
            theses.add_evidence(
                screen.delta.engine, thesis.id, evidence_id, "support", note, accepted=True
            )
        screen.notify(f"thesis created with {len(evidence_ids)} linked evidence items", timeout=6)

    screen.app.push_screen(ThesisForm(claim=claim, targets=", ".join(targets)), created)
