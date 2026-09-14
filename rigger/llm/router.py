"""Task name -> model id mapping from config."""

from __future__ import annotations


def model_for(config: object, task: str) -> str:
    """Model id for ``task`` from ``config.llm_routing``; fails loudly when unrouted.

    Strategies must not fall back to hard-coded model literals: a missing route
    would silently diverge from config.toml.
    """
    routing: dict[str, str] = getattr(config, "llm_routing", {}) or {}
    try:
        return routing[task]
    except KeyError:
        raise KeyError(
            f"no model routed for task {task!r}; add [llm.routing].{task} to config.toml"
        ) from None
