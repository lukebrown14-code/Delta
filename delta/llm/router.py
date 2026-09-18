"""Task name -> model id mapping from config."""

from __future__ import annotations


def model_for(config: object, task: str, *, plugin: str | None = None) -> str:
    """Model id for ``task``; ``[plugins.<plugin>].model`` beats ``[llm.routing]``.

    Strategies must not fall back to hard-coded model literals: a missing route
    would silently diverge from config.toml. An empty-string override is
    ignored, and an unrouted task fails loudly.
    """
    if plugin:
        plugins: dict[str, dict[str, object]] = getattr(config, "plugins", {}) or {}
        override = plugins.get(plugin, {}).get("model")
        if isinstance(override, str) and override:
            return override
    routing: dict[str, str] = getattr(config, "llm_routing", {}) or {}
    try:
        return routing[task]
    except KeyError:
        raise KeyError(
            f"no model routed for task {task!r}; add [llm.routing].{task} to config.toml"
        ) from None
