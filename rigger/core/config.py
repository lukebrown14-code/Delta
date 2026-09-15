"""Configuration: pydantic-settings (.env) + TOML loader (config.toml)."""

from __future__ import annotations

import tomllib
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field
from pydantic_settings import BaseSettings, SettingsConfigDict

from rigger.targets import DEFAULT_KIND, LEGACY_KIND

CONFIG_PATH = Path("config.toml")
ENV_PATH = Path(".env")


class Settings(BaseSettings):
    """Secrets and environment overrides, never committed."""

    model_config = SettingsConfigDict(env_file=ENV_PATH, env_file_encoding="utf-8", extra="ignore")

    openrouter_api_key: str = ""
    litellm_proxy_key: str = ""


class AppConfig(BaseModel):
    """The merged view of config.toml (loaded lazily)."""

    base_currency: str = "AUD"
    db_path: str = "data/rigger.db"
    reports_dir: str = "reports"

    universe: dict[str, list[str]] = Field(default_factory=dict)
    targets: dict[str, dict[str, Any]] = Field(default_factory=dict)
    llm_provider: str = "litellm"
    llm_proxy_base_url: str = "http://localhost:4000"
    llm_routing: dict[str, str] = Field(default_factory=dict)
    plugins: dict[str, dict[str, Any]] = Field(default_factory=dict)


def load_toml(path: Path = CONFIG_PATH) -> dict[str, Any]:
    if not path.exists():
        return {}
    with path.open("rb") as f:
        return tomllib.load(f)


def _target_tables(raw: dict[str, Any]) -> dict[str, dict[str, Any]]:
    """Merge [targets] and legacy [watchlists] tables, then the [universe] shim.

    Each spec carries an explicit kind: ``company`` for bare [targets] tables,
    the legacy ``tickers`` kind for [watchlists] tables and universe shim
    entries. [targets] wins on name collisions; user entries beat shim names.
    """
    targets: dict[str, dict[str, Any]] = {}
    for name, spec in raw.get("targets", {}).items():
        targets[str(name)] = {"kind": DEFAULT_KIND, **dict(spec)}
    for name, spec in raw.get("watchlists", {}).items():
        targets.setdefault(str(name), {"kind": LEGACY_KIND, **dict(spec)})
    for market, tickers in raw.get("universe", {}).items():
        targets.setdefault(
            f"universe_{market}",
            {"kind": LEGACY_KIND, "market": market, "tickers": list(tickers), "legacy": True},
        )
    return targets


def build_config(raw: dict[str, Any] | None = None) -> AppConfig:
    raw = raw if raw is not None else load_toml()
    cfg = AppConfig()

    cfg.base_currency = raw.get("base_currency", cfg.base_currency)
    cfg.db_path = raw.get("db_path", cfg.db_path)
    cfg.reports_dir = raw.get("reports_dir", cfg.reports_dir)

    cfg.universe = raw.get("universe", {})
    cfg.targets = _target_tables(raw)

    llm = raw.get("llm", {})
    cfg.llm_provider = llm.get("provider", cfg.llm_provider)
    cfg.llm_proxy_base_url = llm.get("proxy_base_url", cfg.llm_proxy_base_url)
    cfg.llm_routing = llm.get("routing", {})

    cfg.plugins = raw.get("plugins", {})
    return cfg


def load_config(path: Path = CONFIG_PATH) -> tuple[Settings, AppConfig]:
    settings = Settings()
    return settings, build_config(load_toml(path))
