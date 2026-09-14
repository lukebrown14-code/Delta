"""Configuration: pydantic-settings (.env) + TOML loader (config.toml)."""

from __future__ import annotations

import tomllib
from pathlib import Path
from typing import Any

from pydantic import BaseModel, Field
from pydantic_settings import BaseSettings, SettingsConfigDict

CONFIG_PATH = Path("config.toml")
ENV_PATH = Path(".env")


class Settings(BaseSettings):
    """Secrets and environment overrides, never committed."""

    model_config = SettingsConfigDict(env_file=ENV_PATH, env_file_encoding="utf-8", extra="ignore")

    openrouter_api_key: str = ""
    litellm_proxy_key: str = ""
    live_trading: bool = False


class AppConfig(BaseModel):
    """The merged view of config.toml (loaded lazily)."""

    base_currency: str = "AUD"
    db_path: str = "data/rigger.db"
    reports_dir: str = "reports"

    universe: dict[str, list[str]] = Field(default_factory=dict)
    llm_provider: str = "litellm"
    llm_proxy_base_url: str = "http://localhost:4000"
    llm_routing: dict[str, str] = Field(default_factory=dict)
    llm_ensemble_models: list[str] = Field(default_factory=list)
    paper_starting_cash: float = 100_000.0
    paper_slippage_bps: float = 5.0
    risk: dict[str, float] = Field(
        default_factory=lambda: {
            "max_position_pct": 5.0,
            "max_sector_pct": 25.0,
            "max_gross_exposure_pct": 100.0,
            "min_conviction": 0.6,
            "daily_loss_halt_pct": 3.0,
        }
    )
    schedule: dict[str, str] = Field(default_factory=dict)
    plugins: dict[str, dict[str, Any]] = Field(default_factory=dict)


def load_toml(path: Path = CONFIG_PATH) -> dict[str, Any]:
    if not path.exists():
        return {}
    with path.open("rb") as f:
        return tomllib.load(f)


def build_config(raw: dict[str, Any] | None = None) -> AppConfig:
    raw = raw if raw is not None else load_toml()
    cfg = AppConfig()

    cfg.base_currency = raw.get("base_currency", cfg.base_currency)
    cfg.db_path = raw.get("db_path", cfg.db_path)
    cfg.reports_dir = raw.get("reports_dir", cfg.reports_dir)

    cfg.universe = raw.get("universe", {})

    llm = raw.get("llm", {})
    cfg.llm_provider = llm.get("provider", cfg.llm_provider)
    cfg.llm_proxy_base_url = llm.get("proxy_base_url", cfg.llm_proxy_base_url)
    cfg.llm_routing = llm.get("routing", {})
    cfg.llm_ensemble_models = llm.get("ensemble", {}).get("models", [])

    paper = raw.get("paper", {})
    cfg.paper_starting_cash = float(paper.get("starting_cash", cfg.paper_starting_cash))
    cfg.paper_slippage_bps = float(paper.get("slippage_bps", cfg.paper_slippage_bps))

    risk = raw.get("risk", {})
    if risk:
        cfg.risk.update({k: float(v) for k, v in risk.items() if isinstance(v, (int, float))})

    cfg.schedule = raw.get("schedule", {})
    cfg.plugins = raw.get("plugins", {})
    return cfg


def load_config(path: Path = CONFIG_PATH) -> tuple[Settings, AppConfig]:
    settings = Settings()
    return settings, build_config(load_toml(path))
