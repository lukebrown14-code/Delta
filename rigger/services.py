"""Pipeline services shared by the CLI and TUI."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from sqlmodel import Session, select

from rigger.core.db import (
    BarTable,
    EventTable,
    FundamentalTable,
    LLMCallTable,
    NewsItemTable,
    store_items,
)
from rigger.core.time import parse_date

Log = Callable[[str], None]


def _noop_log(_message: str) -> None:
    pass


@dataclass
class IngestResult:
    counts: dict[str, int] = field(default_factory=dict)


@dataclass
class ExtractResult:
    events: int
    instruments: int


async def ingest(
    rig: Any,
    *,
    market: str | None = None,
    tickers: str | None = None,
    since: str | None = None,
    log: Log = _noop_log,
) -> IngestResult:
    since = since or (datetime.now(UTC) - timedelta(days=365)).strftime("%Y-%m-%d")
    instruments = rig.universe()
    if market:
        instruments = [i for i in instruments if i.market == market]
    if tickers:
        wanted = set(tickers.split(","))
        instruments = [i for i in instruments if i.symbol in wanted]
    total: dict[str, int] = {}
    for name, plugin in rig.plugins.items():
        if not plugin.enabled or not hasattr(plugin, "fetch"):
            continue
        if plugin.market and market and plugin.market != market:
            continue
        target = [i for i in instruments if plugin.market is None or i.market == plugin.market]
        if not target:
            continue
        log(f"Ingesting via [bold]{name}[/bold] ({len(target)} instruments)...")
        counts = store_items(rig.engine, await plugin.fetch(target, parse_date(since)))
        for table, count in counts.items():
            total[table] = total.get(table, 0) + count
        log("  stored " + ", ".join(f"{n} {t}" for t, n in counts.items()))
    return IngestResult(total)


async def extract(rig: Any, *, since: str | None = None, log: Log = _noop_log) -> ExtractResult:
    from rigger.extract import extract_events

    since = since or (datetime.now(UTC) - timedelta(days=14)).strftime("%Y-%m-%d")
    events = await extract_events(rig.context(rig.universe()), parse_date(since))
    instruments = len({event.instrument_id for event in events})
    log(
        f"[green]Extracted {len(events)} events across {instruments} instruments since {since}.[/green]"
    )
    return ExtractResult(len(events), instruments)


def set_plugin_enabled(rig: Any, name: str, value: bool) -> None:
    import tomli_w

    from rigger.core import config as config_mod

    raw = config_mod.load_toml()
    raw.setdefault("plugins", {}).setdefault(name, {})["enabled"] = value
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")


def watchlist_specs() -> dict[str, dict[str, Any]]:
    from rigger.core import config as config_mod

    return {
        name: dict(values) for name, values in config_mod.load_toml().get("watchlists", {}).items()
    }


def add_watchlist(
    name: str, *, market: str, tickers: list[str], max_pct: float | None = None
) -> None:
    import tomli_w

    from rigger.core import config as config_mod
    from rigger.core.plugin import MarketPlugin, discover_plugins

    known = sorted(
        plugin_name
        for plugin_name, plugin in discover_plugins().items()
        if isinstance(plugin, MarketPlugin)
    )
    market = market.lower()
    if market not in known:
        raise ValueError(
            f"watchlist {name!r} names market {market!r}; known markets are {', '.join(known)}"
        )
    raw = config_mod.load_toml()
    watchlists = raw.setdefault("watchlists", {})
    if name in watchlists:
        raise ValueError(f"watchlist {name!r} already exists")
    spec: dict[str, Any] = {
        "kind": "tickers",
        "market": market,
        "tickers": [t.upper() for t in tickers],
    }
    if max_pct is not None:
        spec["max_pct"] = max_pct
    watchlists[name] = spec
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")


def remove_watchlist(name: str) -> None:
    import tomli_w

    from rigger.core import config as config_mod

    raw = config_mod.load_toml()
    watchlists = raw.setdefault("watchlists", {})
    if name not in watchlists:
        raise KeyError(f"unknown watchlist: {name}")
    del watchlists[name]
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")


def brief_for(rig: Any, instrument_id: str) -> str | None:
    from rigger.brief import build_brief

    instrument = next((item for item in rig.universe() if item.id == instrument_id), None)
    if instrument is None:
        return None
    brief = build_brief(rig.context(rig.universe()), instrument)
    return brief.render() if brief else None


@dataclass
class DataHealth:
    counts: dict[str, int]
    latest_bar: dict[str, datetime]
    last_llm: datetime | None


def _count(session: Session, table: Any) -> int:
    from sqlmodel import func

    return int(session.exec(select(func.count()).select_from(table)).one())


def data_health(rig: Any) -> DataHealth:
    tables = {
        "bar": BarTable,
        "newsitem": NewsItemTable,
        "event": EventTable,
        "fundamental": FundamentalTable,
        "llmcall": LLMCallTable,
    }
    with Session(rig.engine) as session:
        counts = {name: _count(session, table) for name, table in tables.items()}
        latest: dict[str, datetime] = {}
        for instrument in rig.universe():
            row = session.exec(
                select(BarTable)
                .where(BarTable.instrument_id == instrument.id)
                .order_by(BarTable.ts.desc())  # type: ignore[attr-defined]
            ).first()
            if row:
                latest[instrument.id] = row.ts
        last_llm = session.exec(select(LLMCallTable.ts).order_by(LLMCallTable.ts.desc())).first()  # type: ignore[attr-defined]
    return DataHealth(counts, latest, last_llm)


@dataclass
class CostRow:
    task: str
    model: str
    calls: int
    cost_usd: float


def llm_costs(engine: Any, since: str | None = None) -> list[CostRow]:
    with Session(engine) as session:
        query = select(LLMCallTable)
        if since:
            query = query.where(LLMCallTable.ts >= parse_date(since))
        rows = session.exec(query).all()
    grouped: dict[tuple[str, str], list[float]] = {}
    for row in rows:
        grouped.setdefault((row.task, row.model), []).append(row.cost_usd)
    return [
        CostRow(task, model, len(costs), sum(costs))
        for (task, model), costs in sorted(grouped.items())
    ]


@dataclass
class Check:
    name: str
    ok: bool
    fix: str


def setup_checks(rig: Any) -> list[Check]:
    provider = rig.cfg.llm_provider
    key = (
        rig.settings.openrouter_api_key
        if provider == "openrouter"
        else rig.settings.litellm_proxy_key
        if provider == "litellm-proxy"
        else "env"
    )
    checks = [
        Check(
            f"LLM provider ({provider})",
            bool(key),
            "Set OPENROUTER_API_KEY or LITELLM_PROXY_KEY in .env",
        )
    ]
    checks.append(Check("Config file", Path("config.toml").exists(), "Create config.toml"))
    try:
        with Session(rig.engine) as session:
            session.get(LLMCallTable, "probe")  # never matches; just checks reachability
        reachable = True
    except Exception:
        reachable = False
    checks.append(Check("Database", reachable, "Check db_path in config.toml"))
    checks.append(
        Check(
            "SEC EDGAR contact",
            bool(rig.cfg.plugins.get("sec_edgar", {}).get("contact")),
            "Set [plugins.sec_edgar].contact",
        )
    )
    health = data_health(rig)
    checks.append(Check("Price history", bool(health.latest_bar), "Run rig ingest"))
    return checks
