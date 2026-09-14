"""Plugin base classes, registry, and entry-point discovery."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime
from importlib.metadata import entry_points
from pathlib import Path
from typing import TYPE_CHECKING, Any

from rigger.core.models import (
    Bar,
    Event,
    Fill,
    Fundamental,
    Instrument,
    NewsItem,
    Order,
    Position,
    Signal,
)

if TYPE_CHECKING:
    from sqlalchemy.engine import Engine


class Plugin:
    name: str  # unique, snake_case
    version: str = "0.1.0"
    enabled: bool = True
    # Name of another plugin whose [plugins.<shared>] table supplies defaults
    # for this one, so sibling plugins (e.g. the yfinance bars and calendar
    # plugins) read one mapping instead of two that drift.
    shared_config: str | None = None

    def configure(self, cfg: dict[str, Any]) -> None:
        """Receives its [plugins.<name>] TOML table."""


class MarketPlugin(Plugin):
    currency: str

    def universe(self) -> list[Instrument]:
        raise NotImplementedError

    def is_open(self, ts: datetime) -> bool:
        raise NotImplementedError

    def next_open(self, ts: datetime) -> datetime:
        raise NotImplementedError


class DataPlugin(Plugin):
    market: str | None = None  # None = works for any market

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental | Event]:
        raise NotImplementedError


@dataclass
class Context:
    """Read access to db, llm client, config, and the current instrument universe."""

    engine: Engine
    settings: Any
    config: Any
    llm: Any = None
    universe: list[Instrument] = field(default_factory=list)
    plugins: dict[str, Plugin] = field(default_factory=dict)


class StrategyPlugin(Plugin):
    async def generate(self, ctx: Context) -> list[Signal]:
        raise NotImplementedError


class BrokerPlugin(Plugin):
    async def submit(self, order: Order) -> Fill:
        raise NotImplementedError

    async def positions(self) -> list[Position]:
        raise NotImplementedError

    async def cash(self) -> float:
        raise NotImplementedError


class ReportPlugin(Plugin):
    def render(self, report: Report) -> Path:
        raise NotImplementedError


@dataclass
class Report:
    date: str  # YYYY-MM-DD
    signals: list[Signal]
    orders: list[Order]
    fills: list[Fill]
    positions: list[Position]
    cash: float


PLUGIN_GROUP = "rigger.plugins"


def discover_plugins() -> dict[str, Plugin]:
    """Load all registered plugins from entry points, keyed by name."""
    discovered: dict[str, Plugin] = {}
    eps = entry_points()
    group = eps.select(group=PLUGIN_GROUP)
    for ep in group:
        cls = ep.load()
        instance = cls()
        discovered[instance.name] = instance
    return discovered


def apply_config(plugins: dict[str, Plugin], plugin_cfg: dict[str, dict[str, Any]]) -> None:
    for name, plugin in plugins.items():
        table = dict(plugin_cfg.get(name, {}))
        if "enabled" in table:
            plugin.enabled = bool(table["enabled"])
        shared = plugin.shared_config
        if shared and shared != name:
            defaults = {k: v for k, v in plugin_cfg.get(shared, {}).items() if k != "enabled"}
            table = {**defaults, **table}
        plugin.configure(table)
