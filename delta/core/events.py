"""In-process async event bus.

Events are emitted during the pipeline and can be consumed by any subscriber.
For Phase 1 this is a simple async fan-out; sinks (SQLite, logs) subscribe here.
"""

from __future__ import annotations

from collections import defaultdict
from collections.abc import Awaitable, Callable
from typing import Any

EventHandler = Callable[[str, dict[str, Any]], Awaitable[None]]


class EventBus:
    def __init__(self) -> None:
        self._subscribers: dict[str, list[EventHandler]] = defaultdict(list)

    def subscribe(self, event: str, handler: EventHandler) -> None:
        self._subscribers[event].append(handler)

    def unsubscribe(self, event: str, handler: EventHandler) -> None:
        if handler in self._subscribers[event]:
            self._subscribers[event].remove(handler)

    async def emit(self, event: str, **payload: Any) -> None:
        for handler in self._subscribers.get(event, []):
            try:
                await handler(event, payload)
            except Exception:  # pragma: no cover - subscribers must not break the pipeline
                import logging

                logging.getLogger("delta.events").exception("event handler failed for %s", event)

    def emit_sync_async(self, event: str, **payload: Any) -> Awaitable[None]:
        return self.emit(event, **payload)


bus = EventBus()
