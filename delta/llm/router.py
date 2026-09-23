"""Task name -> model id mapping from config."""

from __future__ import annotations


def model_for(config: object, task: str, *, plugin: str | None = None) -> str:
    """Model id for ``task``: ``[llm] model`` first, then plugin, then routing.

    A single ``[llm] model`` set in Settings is the one model for all tasks and
    beats everything, including per-plugin overrides. Without it,
    ``[plugins.<plugin>].model`` beats ``[llm.routing]``. Strategies must not
    fall back to hard-coded model literals: a missing route fails loudly
    rather than silently diverging from config.toml. An empty-string value is
    treated as unset at every level.
    """
    single = getattr(config, "llm_model", "")
    if isinstance(single, str) and single:
        return single
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
            f"no model routed for task {task!r}; set [llm] model or add [llm.routing].{task} "
            "to config.toml"
        ) from None
