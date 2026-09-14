"""Task name -> model id mapping from config."""

from __future__ import annotations

DEFAULT_ROUTING = {
    "extract": "gemini/gemini-flash-1.5",
    "analyse": "anthropic/claude-sonnet-4",
    "critique": "openai/gpt-4o",
    "pm": "anthropic/claude-opus-4",
}


class Router:
    def __init__(self, routing: dict[str, str] | None = None) -> None:
        self._routing = {**DEFAULT_ROUTING, **(routing or {})}

    def model_for(self, task: str) -> str:
        return self._routing.get(task, self._routing.get("analyse", ""))

    def __getitem__(self, task: str) -> str:
        return self.model_for(task)
