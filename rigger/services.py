"""Pipeline services shared by the CLI and TUI."""

from __future__ import annotations

import uuid
from collections.abc import Callable
from dataclasses import dataclass, field
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any, Literal, cast

from sqlmodel import Session, delete, select

from rigger.core.db import (
    BarTable,
    CashTable,
    EventTable,
    FillTable,
    FundamentalTable,
    LLMCallTable,
    NewsItemTable,
    OrderTable,
    PositionTable,
    SignalTable,
    store_items,
)
from rigger.core.json import from_json
from rigger.core.models import Fill, Order, Signal
from rigger.core.plugin import Report
from rigger.core.time import parse_date
from rigger.paper.fx import fx_instruments, latest_fx_rate
from rigger.paper.risk import RiskLimits, size_signal

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


@dataclass
class ExecuteResult:
    fills: list[Fill] = field(default_factory=list)
    skipped: list[tuple[str, str]] = field(default_factory=list)


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
        if name == "yfinance":
            target += fx_instruments(target, rig.cfg.base_currency)
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


async def analyse(
    rig: Any,
    *,
    strategies: list[str] | str | None = None,
    dry_run: bool = False,
    log: Log = _noop_log,
) -> list[Signal]:
    from rigger.runtime import _store_signal

    names = ["llm_analyst"] if strategies is None else strategies
    if isinstance(names, str):
        names = names.split(",")
    ctx = rig.context(rig.universe())
    result: list[Signal] = []
    for name in names:
        plugin = rig.plugins.get(name)
        if plugin is None or not hasattr(plugin, "generate"):
            log(f"[red]Unknown strategy: {name}[/red]")
            continue
        if not plugin.enabled:
            log(f"[yellow]Strategy {name} is disabled; skipping[/yellow]")
            continue
        log(f"Running strategy [bold]{name}[/bold]...")
        generated = await plugin.generate(ctx)
        result.extend(generated)
        log(f"  generated {len(generated)} signals")
    if not dry_run:
        with Session(rig.engine) as session:
            for signal in result:
                _store_signal(session, signal)
            session.commit()
        log(f"[green]Stored {len(result)} signals.[/green]")
    else:
        for signal in result:
            log(
                f"  {signal.instrument_id}: {signal.direction} (conviction {signal.conviction:.2f})"
            )
    return result


async def execute(
    rig: Any, *, since: str | None = None, all_: bool = False, log: Log = _noop_log
) -> ExecuteResult:
    limits = RiskLimits(**{key: value for key, value in rig.cfg.risk.items()})
    with Session(rig.engine) as session:
        executed = set(session.exec(select(OrderTable.signal_id)).all())
        query = select(SignalTable)
        if not all_:
            query = query.where(
                SignalTable.ts
                >= (parse_date(since) if since else datetime.now(UTC) - timedelta(days=1))
            )
        rows = session.exec(query).all()
    instruments = {instrument.id: instrument for instrument in rig.universe()}
    broker = rig.plugins["paper"]
    orders: list[Order] = []
    result = ExecuteResult()

    def exposures(equity: float) -> tuple[dict[str, float], float]:
        sectors: dict[str, float] = {}
        gross = 0.0
        if equity <= 0:
            return sectors, gross
        for position in rig.portfolio.positions():
            percent = (
                abs(
                    position.qty
                    * position.avg_price
                    * rig.portfolio.fx_rate(position.instrument_id)
                )
                / equity
                * 100
            )
            gross += percent
            instrument = instruments.get(position.instrument_id)
            if instrument and instrument.sector:
                sectors[instrument.sector] = sectors.get(instrument.sector, 0.0) + percent
        return sectors, gross

    for row in rows:
        if row.id in executed or row.direction == "flat":
            continue
        instrument = instruments.get(row.instrument_id)
        if instrument is None:
            result.skipped.append((row.instrument_id, "not in current universe"))
            continue
        try:
            price = rig.portfolio.latest_price_base(row.instrument_id) or 0.0
            equity = rig.portfolio.equity()
            sector_pct, gross_pct = exposures(equity)
            decision = size_signal(
                Signal(
                    id=row.id,
                    ts=row.ts,
                    instrument_id=row.instrument_id,
                    strategy=row.strategy,
                    direction=cast(Literal["long", "short", "flat"], row.direction),
                    conviction=row.conviction,
                    horizon_days=row.horizon_days,
                    thesis=row.thesis,
                    invalidation=row.invalidation,
                ),
                instrument,
                equity,
                price,
                limits,
                sector_exposure_pct=sector_pct.get(instrument.sector or "", 0.0),
                gross_exposure_pct=gross_pct,
                cash=rig.portfolio.cash(),
                held_qty=rig.portfolio.position_qty(row.instrument_id),
            )
        except ValueError as exc:
            result.skipped.append((row.instrument_id, str(exc)))
            log(f"[yellow]{row.instrument_id}: skipped — {exc}[/yellow]")
            continue
        if not decision.approved:
            result.skipped.append((row.instrument_id, decision.reason))
            log(f"[yellow]{row.instrument_id}: skipped — {decision.reason}[/yellow]")
            continue
        side = "buy" if row.direction == "long" else "sell"
        quantity = decision.qty
        if side == "sell":
            held = rig.portfolio.position_qty(row.instrument_id)
            if held <= 0:
                result.skipped.append((row.instrument_id, "short signal with no position"))
                continue
            quantity = min(quantity, held)
        order = Order(
            id=uuid.uuid4().hex,
            signal_id=row.id,
            instrument_id=row.instrument_id,
            side=side,  # type: ignore[arg-type]
            qty=quantity,
            type="market",
            submitted_ts=datetime.now(UTC),
            broker="paper",
        )
        fill = await broker.submit(order)
        orders.append(order)
        result.fills.append(fill)
        log(f"[green]{side.upper()} {quantity:.4f} {row.instrument_id} @ {fill.price:.2f}[/green]")
    with Session(rig.engine) as session:
        for order in orders:
            session.add(
                OrderTable(
                    id=order.id,
                    signal_id=order.signal_id,
                    instrument_id=order.instrument_id,
                    side=order.side,
                    qty=order.qty,
                    type=order.type,
                    limit_price=order.limit_price,
                    submitted_ts=order.submitted_ts,
                    broker=order.broker,
                )
            )
        session.commit()
    log(f"[green]Executed {len(result.fills)} fills.[/green]")
    return result


def _signal(row: Any) -> Signal:
    return Signal(
        id=row.id,
        ts=row.ts,
        instrument_id=row.instrument_id,
        strategy=row.strategy,
        direction=cast(Literal["long", "short", "flat"], row.direction),
        conviction=row.conviction,
        horizon_days=row.horizon_days,
        thesis=row.thesis,
        invalidation=row.invalidation,
        evidence_ids=from_json(row.evidence_ids),
        model=row.model,
        prompt_version=row.prompt_version,
        cost_usd=row.cost_usd,
        metadata=row.metadata_ or {},
    )


def report(rig: Any, *, date: str | None = None, fmt: str = "markdown") -> Path:
    plugin = rig.plugins.get(fmt)
    if plugin is None or not hasattr(plugin, "render") or not plugin.enabled:
        raise KeyError(f"unknown report format: {fmt}")
    with Session(rig.engine) as session:
        signals_rows = session.exec(select(SignalTable)).all()
        order_rows = session.exec(select(OrderTable)).all()
        fill_rows = session.exec(select(FillTable)).all()
    return cast(
        Path,
        plugin.render(
            Report(
                date=date or datetime.now(UTC).date().isoformat(),
                signals=[_signal(row) for row in signals_rows],
                orders=[
                    Order(
                        id=row.id,
                        signal_id=row.signal_id,
                        instrument_id=row.instrument_id,
                        side=cast(Literal["buy", "sell"], row.side),
                        qty=row.qty,
                        type=cast(Literal["market", "limit"], row.type),
                        limit_price=row.limit_price,
                        submitted_ts=row.submitted_ts,
                        broker=row.broker,
                    )
                    for row in order_rows
                ],
                fills=[
                    Fill(
                        order_id=row.order_id,
                        ts=row.ts,
                        qty=row.qty,
                        price=row.price,
                        fee=row.fee,
                        slippage=row.slippage,
                    )
                    for row in fill_rows
                ],
                positions=rig.portfolio.positions(),
                cash=rig.portfolio.cash(),
                equity=rig.portfolio.equity(),
                base_currency=rig.cfg.base_currency,
            )
        ),
    )


def reset_paper(rig: Any, *, signals: bool = False) -> None:
    with Session(rig.engine) as session:
        for table in [FillTable, OrderTable] + ([SignalTable] if signals else []):
            session.exec(delete(table))
        session.exec(delete(PositionTable))
        session.exec(delete(CashTable))
        session.add(
            CashTable(
                id=1, balance=rig.cfg.paper_starting_cash, base_currency=rig.cfg.base_currency
            )
        )
        session.commit()


def set_plugin_enabled(rig: Any, name: str, value: bool) -> None:
    import tomli_w

    from rigger.core import config as config_mod

    raw = config_mod.load_toml()
    raw.setdefault("plugins", {}).setdefault(name, {})["enabled"] = value
    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")


def signals(
    engine: Any, *, since: str | None = None, strategy: str | None = None, limit: int = 200
) -> list[Signal]:
    with Session(engine) as session:
        query = select(SignalTable)
        if since:
            query = query.where(SignalTable.ts >= parse_date(since))
        if strategy:
            query = query.where(SignalTable.strategy == strategy)
        rows = session.exec(query.order_by(SignalTable.ts.desc()).limit(limit)).all()  # type: ignore[attr-defined]
    return [_signal(row) for row in rows]


def signal_by_id(engine: Any, id: str) -> Signal | None:
    with Session(engine) as session:
        row = session.get(SignalTable, id)
    return _signal(row) if row else None


@dataclass
class BarEvidence:
    count: int = 0
    low: float | None = None
    high: float | None = None
    rows: list[Any] = field(default_factory=list)


@dataclass
class Evidence:
    bars: BarEvidence = field(default_factory=BarEvidence)
    news: list[Any] = field(default_factory=list)
    events: list[Any] = field(default_factory=list)
    fundamentals: list[Any] = field(default_factory=list)
    unresolved: list[str] = field(default_factory=list)

    @property
    def total(self) -> int:
        return self.bars.count + len(self.news) + len(self.events) + len(self.fundamentals)


def resolve_evidence(engine: Any, evidence_ids: list[str]) -> Evidence:
    evidence = Evidence()
    with Session(engine) as session:
        for evidence_id in evidence_ids:
            if evidence_id.startswith("fundamental:"):
                try:
                    row = session.get(FundamentalTable, int(evidence_id.split(":", 1)[1]))
                except ValueError:
                    row = None
                if row:
                    evidence.fundamentals.append(row)
                else:
                    evidence.unresolved.append(evidence_id)
                continue
            try:
                bar = session.get(BarTable, int(evidence_id))
            except ValueError:
                bar = None
            if bar:
                evidence.bars.rows.append(bar)
                continue
            news = session.get(NewsItemTable, evidence_id)
            if news:
                evidence.news.append(news)
                continue
            event = session.get(EventTable, evidence_id)
            if event:
                evidence.events.append(event)
                continue
            evidence.unresolved.append(evidence_id)
    evidence.bars.count = len(evidence.bars.rows)
    if evidence.bars.rows:
        closes = [row.close for row in evidence.bars.rows]
        evidence.bars.low = min(closes)
        evidence.bars.high = max(closes)
    return evidence


def brief_for(rig: Any, instrument_id: str) -> str | None:
    from rigger.brief import build_brief

    instrument = next((item for item in rig.universe() if item.id == instrument_id), None)
    if instrument is None:
        return None
    brief = build_brief(rig.context(rig.universe()), instrument)
    return brief.render() if brief else None


@dataclass
class PositionSummary:
    instrument_id: str
    qty: float
    avg_price: float
    currency: str
    value: float


@dataclass
class PortfolioSummary:
    positions: list[PositionSummary]
    cash: float
    equity: float
    base_currency: str
    fills: list[Any]


def portfolio_summary(rig: Any) -> PortfolioSummary:
    positions = [
        PositionSummary(
            position.instrument_id,
            position.qty,
            position.avg_price,
            rig.portfolio.currency_of(position.instrument_id),
            position.qty * position.avg_price * rig.portfolio.fx_rate(position.instrument_id),
        )
        for position in rig.portfolio.positions()
    ]
    with Session(rig.engine) as session:
        fills = list(
            session.exec(select(FillTable).order_by(FillTable.ts.desc()).limit(20)).all()  # type: ignore[attr-defined]
        )
    return PortfolioSummary(
        positions, rig.portfolio.cash(), rig.portfolio.equity(), rig.cfg.base_currency, fills
    )


@dataclass
class DataHealth:
    counts: dict[str, int]
    latest_bar: dict[str, datetime]
    fx_rates: dict[str, float]
    last_signal: datetime | None
    last_llm: datetime | None


def data_health(rig: Any) -> DataHealth:
    tables = {
        "bar": BarTable,
        "newsitem": NewsItemTable,
        "event": EventTable,
        "fundamental": FundamentalTable,
        "signal": SignalTable,
        "order": OrderTable,
        "fill": FillTable,
        "llmcall": LLMCallTable,
    }
    with Session(rig.engine) as session:
        counts = {name: len(session.exec(select(table)).all()) for name, table in tables.items()}
        latest = {}
        for instrument in rig.universe():
            row = session.exec(
                select(BarTable)
                .where(BarTable.instrument_id == instrument.id)
                .order_by(BarTable.ts.desc())  # type: ignore[attr-defined]
            ).first()
            if row:
                latest[instrument.id] = row.ts
        last_signal = session.exec(select(SignalTable.ts).order_by(SignalTable.ts.desc())).first()  # type: ignore[attr-defined]
        last_llm = session.exec(select(LLMCallTable.ts).order_by(LLMCallTable.ts.desc())).first()  # type: ignore[attr-defined]
    fx_rates = {}
    for currency in {instrument.currency for instrument in rig.universe()}:
        rate = latest_fx_rate(rig.engine, currency, rig.cfg.base_currency)
        if rate is not None:
            fx_rates[f"{currency}/{rig.cfg.base_currency}"] = rate
    return DataHealth(counts, latest, fx_rates, last_signal, last_llm)


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
            f"Set {'OPENROUTER_API_KEY' if provider == 'openrouter' else 'LITELLM_PROXY_KEY'} in .env",
        )
    ]
    checks.append(Check("Config file", Path("config.toml").exists(), "Create config.toml"))
    try:
        with Session(rig.engine) as session:
            session.exec(select(CashTable)).first()
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
    currencies = {
        instrument.currency
        for instrument in rig.universe()
        if instrument.currency != rig.cfg.base_currency
    }
    checks.append(
        Check(
            "FX rates",
            not currencies
            or all(
                f"{currency}/{rig.cfg.base_currency}" in health.fx_rates for currency in currencies
            ),
            "Run rig ingest to fetch FX bars",
        )
    )
    return checks
