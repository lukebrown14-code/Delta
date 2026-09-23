"""Thesis pane widgets.

Extracted from :mod:`delta.tui.screens.theses` so the screen and its widgets
each live in a file of manageable size (see the C9 simplification).
"""

from __future__ import annotations

from textual.markup import escape

from delta.tui.widgets import Pane


class HealthPane(Pane):
    """A ``Pane`` whose badge can carry a theme colour.

    ``Pane`` paints every badge in ``$text-muted``; the thesis pane's badge
    is the health glyph and state, which reads in its own colour everywhere
    else on the screen. Local until ``Pane.set_badge`` grows a token argument.
    """

    def __init__(self, *children, **kwargs) -> None:
        self._badge_token = "text-muted"
        super().__init__(*children, **kwargs)

    def set_badge(self, text: str, token: str = "text-muted") -> None:  # type: ignore[override]
        self._badge_token = token
        super().set_badge(text)

    def _paint(self) -> None:
        super()._paint()
        if self._badge:
            parts = []
            if self._key:
                parts.append(f"[bold]{escape(self._key)}[/bold]")
            if self._title:
                parts.append(escape(self._title))
            parts.append(f"[${self._badge_token}]· {escape(self._badge)}[/]")
            self.border_title = " ".join(parts)
