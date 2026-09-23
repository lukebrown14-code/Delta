"""The report reader widget for the Research desk (split out of ``research.py``).

``ResearchViewer`` is a ``MarkdownViewer`` that routes ``evidence:``/``thesis:``
links back to the hosting ``Research`` screen and lets external urls through to
the app's browser, instead of the default navigation.
"""

from __future__ import annotations

from textual.widgets import Markdown, MarkdownViewer


class ResearchViewer(MarkdownViewer):
    #: Focusable as itself, not via its inner document: focusing the document
    # widget scrolls it into view, which would drag the reader back to the
    # top of the report.
    can_focus = True

    def on_show(self) -> None:
        self.screen.restore_report_position()

    async def _on_markdown_link_clicked(self, message: Markdown.LinkClicked) -> None:
        message.prevent_default()
        if message.href.startswith("evidence:"):
            message.stop()
            await self.screen.inspect_evidence(message.href.removeprefix("evidence:"))
        elif message.href.startswith("thesis:"):
            message.stop()
            self.screen.promote_claim(message.href.removeprefix("thesis:"))
        elif message.href.startswith(("https://", "http://")):
            message.stop()
            self.app.open_url(message.href)
        elif message.href.startswith("#"):
            await super()._on_markdown_link_clicked(message)
        else:
            message.stop()
