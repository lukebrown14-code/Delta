"""Model catalog: what each provider offers, plus config.toml route writeback.

The catalog is best-effort everywhere: a failed fetch or an unwritable cache
yields an empty list, and the picker degrades to free-text model entry.
"""

from __future__ import annotations

import json
import time
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import TYPE_CHECKING, Any

if TYPE_CHECKING:
    from delta.llm.providers import Provider

CATALOG_PATH = Path("data") / "model_catalog.json"
CACHE_TTL_SECONDS = 86400.0


@dataclass(frozen=True)
class ModelInfo:
    """One model a provider offers; prices are USD per token."""

    id: str
    name: str
    context_length: int | None
    prompt_price: float
    completion_price: float


def _read_cache_entry(provider: str) -> tuple[float, list[ModelInfo]] | None:
    """(fetched_at, models) for ``provider`` from the disk cache, None when unusable."""
    try:
        entry = json.loads(CATALOG_PATH.read_text(encoding="utf-8"))[provider]
        models = [
            ModelInfo(
                id=str(m["id"]),
                name=str(m.get("name") or m["id"]),
                context_length=m.get("context_length"),
                prompt_price=float(m.get("prompt_price", 0.0)),
                completion_price=float(m.get("completion_price", 0.0)),
            )
            for m in entry["models"]
        ]
        return float(entry["fetched_at"]), models
    except Exception:
        return None


def _write_cache(provider: str, models: list[ModelInfo]) -> None:
    """Stamp and store ``models`` for ``provider``, keeping other providers' entries."""
    try:
        raw: dict[str, Any] = {}
        if CATALOG_PATH.exists():
            raw = json.loads(CATALOG_PATH.read_text(encoding="utf-8"))
        raw[provider] = {"fetched_at": time.time(), "models": [asdict(m) for m in models]}
        CATALOG_PATH.parent.mkdir(parents=True, exist_ok=True)
        CATALOG_PATH.write_text(json.dumps(raw, indent=2), encoding="utf-8")
    except Exception:
        pass


def cached_catalog(provider: str) -> list[ModelInfo]:
    """Models for ``provider`` from the disk cache, whatever their age; [] when absent."""
    entry = _read_cache_entry(provider)
    return [] if entry is None else entry[1]


async def catalog(provider: Provider, *, force: bool = False) -> list[ModelInfo]:
    """A provider's models: the disk cache within 24h, else a fetch; never raises."""
    entry = _read_cache_entry(provider.name)
    if entry is not None and not force and time.time() - entry[0] < CACHE_TTL_SECONDS:
        return entry[1]
    try:
        models = await provider.models(force=force)
    except Exception:
        models = []
    # A failed fetch must not overwrite a good catalog: writing [] here would
    # also stamp a fresh fetched_at and hide the real models for a full TTL.
    # Providers report a failure as an empty list, so an empty result is
    # treated the same way and the cache is served instead.
    if not models and entry is not None:
        return entry[1]
    _write_cache(provider.name, models)
    return models


def set_llm_route(task: str, model: str) -> None:
    """Write ``[llm.routing].<task> = model`` to config.toml in the cwd.

    Follows the ``set_plugin_enabled`` writeback shape; comments in config.toml
    are lost on write.
    """
    import tomli_w

    from delta.core import config as config_mod

    raw = config_mod.load_toml()
    raw.setdefault("llm", {}).setdefault("routing", {})[task] = model
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")


def set_llm_provider(name: str) -> None:
    """Write ``[llm] provider = name`` to config.toml in the cwd."""
    import tomli_w

    from delta.core import config as config_mod

    raw = config_mod.load_toml()
    raw.setdefault("llm", {})["provider"] = name
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")


def set_llm_custom(*, base_url: str, api_key_env: str = "CUSTOM_API_KEY") -> None:
    """Point the ``custom`` provider at a self-hosted or other endpoint.

    Writes ``[llm] provider/base_url/api_key_env``; the key itself goes to
    .env under ``api_key_env`` via :func:`delta.core.config.set_env_value`.
    """
    import tomli_w

    from delta.core import config as config_mod

    raw = config_mod.load_toml()
    llm = raw.setdefault("llm", {})
    llm["provider"] = "custom"
    llm["base_url"] = base_url
    llm["api_key_env"] = api_key_env
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")


def set_plugin_model(plugin_name: str, model: str | None) -> None:
    """Write ``[plugins.<plugin_name>].model``; ``None`` removes the override.

    ``plugin_name`` is the plugin's ``name`` attribute (its config table key),
    not its entry-point name. Comments in config.toml are lost on write.
    """
    import tomli_w

    from delta.core import config as config_mod

    raw = config_mod.load_toml()
    table = raw.setdefault("plugins", {}).setdefault(plugin_name, {})
    if model is None:
        table.pop("model", None)
    else:
        table["model"] = model
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")
