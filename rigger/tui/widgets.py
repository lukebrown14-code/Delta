"""Shared TUI widgets."""

from textual.containers import VerticalScroll
from textual.widgets import Static


class Panel(VerticalScroll):
    def __init__(self, title: str, content: str = "") -> None:
        super().__init__()
        self.title = title
        self.content = content

    def compose(self):
        yield Static(f"[bold]{self.title}[/bold]")
        yield Static(self.content, id=f"{self.title.lower().replace(' ', '-')}-content")
