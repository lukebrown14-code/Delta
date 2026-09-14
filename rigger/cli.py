"""Typer CLI exposing the `rig` command."""

from __future__ import annotations

import asyncio
import uuid
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Annotated

import typer
from rich.console import Console
from rich.table import Table
from sqlalchemy.dialects.sqlite import insert as sqlite_insert
from sqlmodel import Session, select

from rigger.core import config as config_mod
from rigger.core.db import (
    BarTable,
    FillTable,
    LLMCallTable,
    OrderTable,
    SignalTable,
    ensure_cash,
    init_engine,
)
from rigger.core.json import from_json, to_json
from rigger.core.models import Bar, Instrument, Order, Signal
from rigger.core.plugin import (
    Context,
    MarketPlugin,
    Report,
    ReportPlugin,
    apply_config,
    discover_plugins,
)
from rigger.llm.client import build_client
from rigger.paper.portfolio import PaperPortfolio
from rigger.paper.risk import RiskLimits, size_signal

app = typer.Typer(help="Rigger — AI investment research and paper trading")
console = Console()
plugins_app = typer.Typer(help="Plugin management")
paper_app = typer.Typer(help="Paper portfolio management")
llm_app = typer.Typer(help="LLM cost/model inspection")
config_app = typer.Typer(help="Configuration")
app.add_typer(plugins_app, name="plugins")
app.add_typer(paper_app, name="paper")
app.add_typer(llm_app, name="llm")
app.add_typer(config_app, name="config")


# --------------------------------------------------------------------------- #
# Wiring
# --------------------------------------------------------------------------- #
class Rigger:
    def __init__(self) -> None:
        self.settings, self.cfg = config_mod.load_config()
        self.engine = init_engine(self.cfg.db_path)
        ensure_cash(self.engine, self.cfg.base_currency, self.cfg.paper_starting_cash)

        self.plugins = discover_plugins()
        apply_config(self.plugins, self.cfg.plugins)
        for plugin in self.plugins.values():
            if isinstance(plugin, ReportPlugin):
                table = dict(self.cfg.plugins.get(plugin.name, {}))
                table.setdefault("reports_dir", self.cfg.reports_dir)
                plugin.configure(table)

        for market_name, tickers in self.cfg.universe.items():
            mp = self.plugins.get(market_name)
            if mp is not None:
                mp.configure({"tickers": tickers})

        self.llm = build_client(
            provider=self.cfg.llm_provider,
            engine=self.engine,
            openrouter_api_key=self.settings.openrouter_api_key,
            litellm_proxy_key=self.settings.litellm_proxy_key,
            proxy_base_url=self.cfg.llm_proxy_base_url,
        )
        self.portfolio = PaperPortfolio(
            self.engine,
            self.cfg.base_currency,
            self.cfg.paper_slippage_bps,
        )
        broker = self.plugins.get("paper")
        if broker is not None:
            broker.bind(self.portfolio)

    def universe(self) -> list[Instrument]:
        out: list[Instrument] = []
        for plugin in self.plugins.values():
            if plugin.enabled and isinstance(plugin, MarketPlugin):
                out.extend(plugin.universe())
        return out

    def context(self, universe: list[Instrument]) -> Context:
        return Context(
            engine=self.engine,
            settings=self.settings,
            config=self.cfg,
            llm=self.llm,
            universe=universe,
            plugins=self.plugins,
        )


def _store_signal(session: Session, s: Signal) -> None:
    session.add(
        SignalTable(
            id=s.id,
            ts=s.ts,
            instrument_id=s.instrument_id,
            strategy=s.strategy,
            direction=s.direction,
            conviction=s.conviction,
            horizon_days=s.horizon_days,
            thesis=s.thesis,
            invalidation=s.invalidation,
            evidence_ids=to_json(s.evidence_ids),
            model=s.model,
            prompt_version=s.prompt_version,
            cost_usd=s.cost_usd,
            metadata_=s.metadata,
        )
    )


# --------------------------------------------------------------------------- #
# plugins
# --------------------------------------------------------------------------- #
@plugins_app.command("list")
def plugins_list(
    type_: Annotated[str | None, typer.Option("--type")] = None,
) -> None:
    """List discovered plugins."""
    plugins = discover_plugins()
    table = Table(title="Plugins")
    table.add_column("Name")
    table.add_column("Type")
    table.add_column("Version")
    for name, p in sorted(plugins.items()):
        ptype = type(p).__mro__[1].__name__.removesuffix("Plugin").lower()
        if type_ and ptype != type_:
            continue
        table.add_row(name, ptype, p.version)
    console.print(table)


@plugins_app.command("enable")
def plugins_enable(name: str) -> None:
    _set_enabled(name, True)


@plugins_app.command("disable")
def plugins_disable(name: str) -> None:
    _set_enabled(name, False)


def _set_enabled(name: str, value: bool) -> None:
    rig = Rigger()
    if name not in rig.plugins:
        console.print(f"[red]Unknown plugin: {name}[/red]")
        raise typer.Exit(1)
    raw = config_mod.load_toml()
    plugins = raw.setdefault("plugins", {})
    plugins.setdefault(name, {})["enabled"] = value
    import tomli_w

    Path("config.toml").write_text(tomli_w.dumps(raw), encoding="utf-8")
    console.print(f"{'Enabled' if value else 'Disabled'} [bold]{name}[/bold]")


# --------------------------------------------------------------------------- #
# ingest
# --------------------------------------------------------------------------- #
@app.command()
def ingest(
    market: Annotated[str | None, typer.Option("--market")] = None,
    tickers: Annotated[str | None, typer.Option("--tickers")] = None,
    since: Annotated[str | None, typer.Option("--since")] = None,
) -> None:
    """Fetch market data into the database."""
    if since is None:
        since = (datetime.now(UTC) - timedelta(days=365)).strftime("%Y-%m-%d")
    rig = Rigger()
    instruments = rig.universe()
    if market:
        instruments = [i for i in instruments if i.market == market]
    if tickers:
        wanted = set(tickers.split(","))
        instruments = [i for i in instruments if i.symbol in wanted]

    since_dt = datetime.strptime(since, "%Y-%m-%d").replace(tzinfo=UTC)

    async def run() -> None:
        for name, plugin in rig.plugins.items():
            if not plugin.enabled or not hasattr(plugin, "fetch"):
                continue
            if plugin.market and market and plugin.market != market:
                continue
            target = [i for i in instruments if plugin.market is None or i.market == plugin.market]
            if not target:
                continue
            console.print(f"Ingesting via [bold]{name}[/bold] ({len(target)} instruments)...")
            fetched = await plugin.fetch(target, since_dt)
            bars = [x for x in fetched if isinstance(x, Bar)]
            if bars:
                # Re-ingesting an overlapping window must not duplicate history:
                # (instrument_id, ts) is unique on `bar`, so ignore existing rows.
                with Session(rig.engine) as session:
                    session.exec(
                        sqlite_insert(BarTable)
                        .values([b.model_dump() for b in bars])
                        .on_conflict_do_nothing(index_elements=["instrument_id", "ts"])
                    )
                    session.commit()
            console.print(f"  stored {len(bars)} bars")

    asyncio.run(run())
    console.print("[green]Ingest complete.[/green]")


# --------------------------------------------------------------------------- #
# analyse
# --------------------------------------------------------------------------- #
@app.command()
def analyse(
    strategy: Annotated[str | None, typer.Option("--strategy")] = None,
    dry_run: Annotated[bool, typer.Option("--dry-run")] = False,
) -> None:
    """Generate signals via strategy plugins."""
    rig = Rigger()
    universe = rig.universe()
    strategies = strategy.split(",") if strategy else ["llm_analyst"]
    ctx = rig.context(universe)

    all_signals: list[Signal] = []

    async def run() -> None:
        for sname in strategies:
            plugin = rig.plugins.get(sname)
            if plugin is None or not hasattr(plugin, "generate"):
                console.print(f"[red]Unknown strategy: {sname}[/red]")
                continue
            if not plugin.enabled:
                console.print(f"[yellow]Strategy {sname} is disabled; skipping[/yellow]")
                continue
            console.print(f"Running strategy [bold]{sname}[/bold]...")
            signals = await plugin.generate(ctx)
            all_signals.extend(signals)
            console.print(f"  generated {len(signals)} signals")

    asyncio.run(run())

    if not dry_run:
        with Session(rig.engine) as session:
            for s in all_signals:
                _store_signal(session, s)
            session.commit()
        console.print(f"[green]Stored {len(all_signals)} signals.[/green]")
    else:
        for s in all_signals:
            console.print(f"  {s.instrument_id}: {s.direction} (conviction {s.conviction:.2f})")


# --------------------------------------------------------------------------- #
# execute
# --------------------------------------------------------------------------- #
@app.command()
def execute() -> None:
    """Route pending signals through risk + broker."""
    rig = Rigger()
    limits = RiskLimits(**{k: v for k, v in rig.cfg.risk.items()})

    with Session(rig.engine) as session:
        executed_signal_ids = set(session.exec(select(OrderTable.signal_id)).all())
        signals = session.exec(select(SignalTable)).all()

    broker = rig.plugins["paper"]
    universe_by_id = {i.id: i for i in rig.universe()}
    orders: list[Order] = []
    fills = []

    def exposures(equity: float) -> tuple[dict[str, float], float]:
        """Current (sector -> pct of equity, gross pct of equity) from open positions."""
        by_sector: dict[str, float] = {}
        gross = 0.0
        if equity <= 0:
            return by_sector, gross
        for pos in rig.portfolio.positions():
            pct = abs(pos.qty * pos.avg_price) / equity * 100
            gross += pct
            held = universe_by_id.get(pos.instrument_id)
            sector = held.sector if held else None
            if sector:
                by_sector[sector] = by_sector.get(sector, 0.0) + pct
        return by_sector, gross

    async def run() -> None:
        for sig in signals:
            if sig.id in executed_signal_ids or sig.direction == "flat":
                continue
            inst = universe_by_id.get(sig.instrument_id)
            if inst is None:
                console.print(
                    f"[yellow]{sig.instrument_id}: skipped — not in current universe[/yellow]"
                )
                continue
            price = rig.portfolio.latest_price(sig.instrument_id) or 0.0
            equity = rig.portfolio.equity()
            sector_pct, gross_pct = exposures(equity)
            decision = size_signal(
                Signal(
                    id=sig.id,
                    ts=sig.ts,
                    instrument_id=sig.instrument_id,
                    strategy=sig.strategy,
                    direction=sig.direction,  # type: ignore[arg-type]
                    conviction=sig.conviction,
                    horizon_days=sig.horizon_days,
                    thesis=sig.thesis,
                    invalidation=sig.invalidation,
                ),
                inst,
                equity,
                price,
                limits,
                sector_exposure_pct=sector_pct.get(inst.sector or "", 0.0),
                gross_exposure_pct=gross_pct,
                cash=rig.portfolio.cash(),
            )
            if not decision.approved:
                console.print(f"[yellow]{sig.instrument_id}: skipped — {decision.reason}[/yellow]")
                continue
            side = "buy" if sig.direction == "long" else "sell"
            qty = decision.qty
            if side == "sell":
                # The paper portfolio is long-only: a short signal can only close what is held.
                held = rig.portfolio.position_qty(sig.instrument_id)
                if held <= 0:
                    console.print(
                        f"[yellow]{sig.instrument_id}: skipped — short signal with no position[/yellow]"
                    )
                    continue
                qty = min(qty, held)
            order = Order(
                id=uuid.uuid4().hex,
                signal_id=sig.id,
                instrument_id=sig.instrument_id,
                side=side,  # type: ignore[arg-type]
                qty=qty,
                type="market",
                submitted_ts=datetime.now(UTC),
                broker="paper",
            )
            fill = await broker.submit(order)
            orders.append(order)
            fills.append(fill)
            console.print(
                f"[green]{side.upper()} {order.qty:.4f} {sig.instrument_id} @ {fill.price:.2f}[/green]"
            )

    asyncio.run(run())

    with Session(rig.engine) as session:
        for o in orders:
            session.add(
                OrderTable(
                    id=o.id,
                    signal_id=o.signal_id,
                    instrument_id=o.instrument_id,
                    side=o.side,
                    qty=o.qty,
                    type=o.type,
                    limit_price=o.limit_price,
                    submitted_ts=o.submitted_ts,
                    broker=o.broker,
                )
            )
        session.commit()
    console.print(f"[green]Executed {len(fills)} fills.[/green]")


# --------------------------------------------------------------------------- #
# report
# --------------------------------------------------------------------------- #
@app.command()
def report(
    format: Annotated[str, typer.Option("--format")] = "markdown",
    date: Annotated[str | None, typer.Option("--date")] = None,
) -> None:
    """Render a report for a date."""
    if date is None:
        date = datetime.now(UTC).date().isoformat()
    rig = Rigger()
    plugin = rig.plugins.get(format)
    if plugin is None or not hasattr(plugin, "render") or not plugin.enabled:
        console.print(f"[red]Unknown report format: {format}[/red]")
        raise typer.Exit(1)

    with Session(rig.engine) as session:
        signals = session.exec(select(SignalTable)).all()
        orders = session.exec(select(OrderTable)).all()
        fills = session.exec(select(FillTable)).all()

    positions = rig.portfolio.positions()

    from rigger.core.models import Fill
    from rigger.core.models import Signal as SignalModel

    rep = Report(
        date=date,
        signals=[
            SignalModel(
                id=s.id,
                ts=s.ts,
                instrument_id=s.instrument_id,
                strategy=s.strategy,
                direction=s.direction,  # type: ignore[arg-type]
                conviction=s.conviction,
                horizon_days=s.horizon_days,
                thesis=s.thesis,
                invalidation=s.invalidation,
                evidence_ids=from_json(s.evidence_ids),
                model=s.model,
                prompt_version=s.prompt_version,
                metadata=s.metadata_ or {},
            )
            for s in signals
        ],
        orders=[
            Order(
                id=o.id,
                signal_id=o.signal_id,
                instrument_id=o.instrument_id,
                side=o.side,  # type: ignore[arg-type]
                qty=o.qty,
                type=o.type,  # type: ignore[arg-type]
                limit_price=o.limit_price,
                submitted_ts=o.submitted_ts,
                broker=o.broker,
            )
            for o in orders
        ],
        fills=[
            Fill(
                order_id=f.order_id,
                ts=f.ts,
                qty=f.qty,
                price=f.price,
                fee=f.fee,
                slippage=f.slippage,
            )
            for f in fills
        ],
        positions=positions,
        cash=rig.portfolio.cash(),
    )
    path = plugin.render(rep)
    console.print(f"[green]Report written to {path}[/green]")


# --------------------------------------------------------------------------- #
# run
# --------------------------------------------------------------------------- #
@app.command()
def extract(
    since: Annotated[str | None, typer.Option("--since")] = None,
) -> None:
    """Turn unprocessed news into structured events (Phase 2)."""
    from rigger.extract import extract_events

    if since is None:
        since = (datetime.now(UTC) - timedelta(days=14)).strftime("%Y-%m-%d")
    since_dt = datetime.strptime(since, "%Y-%m-%d").replace(tzinfo=UTC)
    rig = Rigger()
    ctx = rig.context(rig.universe())
    events = asyncio.run(extract_events(ctx, since_dt))
    instruments = len({e.instrument_id for e in events})
    console.print(
        f"[green]Extracted {len(events)} events across {instruments} instruments "
        f"since {since}.[/green]"
    )


@app.command()
def run() -> None:
    """ingest → extract → analyse → execute → report."""
    ingest()
    extract()
    analyse()
    execute()
    report()


# --------------------------------------------------------------------------- #
# paper
# --------------------------------------------------------------------------- #
@paper_app.command("status")
def paper_status() -> None:
    rig = Rigger()
    cash = rig.portfolio.cash()
    positions = rig.portfolio.positions()
    equity = rig.portfolio.equity()
    table = Table(title="Paper Portfolio")
    table.add_column("Instrument")
    table.add_column("Qty")
    table.add_column("Avg Price")
    for p in positions:
        table.add_row(p.instrument_id, f"{p.qty:.4f}", f"{p.avg_price:.2f}")
    console.print(table)
    console.print(f"Cash: [bold]{cash:,.2f}[/bold]")
    console.print(f"Equity: [bold]{equity:,.2f}[/bold]")


@paper_app.command("reset")
def paper_reset() -> None:
    rig = Rigger()
    from sqlmodel import delete

    with Session(rig.engine) as session:
        for model in (FillTable, OrderTable, SignalTable):
            session.exec(delete(model))
        from rigger.core.db import CashTable, PositionTable

        session.exec(delete(PositionTable))
        session.exec(delete(CashTable))
        session.add(
            CashTable(
                id=1, balance=rig.cfg.paper_starting_cash, base_currency=rig.cfg.base_currency
            )
        )
        session.commit()
    console.print("[green]Paper portfolio reset.[/green]")


# --------------------------------------------------------------------------- #
# llm
# --------------------------------------------------------------------------- #
@llm_app.command("costs")
def llm_costs(
    since: Annotated[str | None, typer.Option("--since")] = None,
) -> None:
    rig = Rigger()
    since_dt = datetime.strptime(since, "%Y-%m-%d").replace(tzinfo=UTC) if since else None
    with Session(rig.engine) as session:
        query = select(LLMCallTable)
        if since_dt:
            query = query.where(LLMCallTable.ts >= since_dt)
        rows = session.exec(query).all()

    table = Table(title="LLM Costs")
    table.add_column("Task")
    table.add_column("Model")
    table.add_column("Calls")
    table.add_column("Cost (USD)")
    agg: dict[tuple[str, str], list[float]] = {}
    for r in rows:
        agg.setdefault((r.task, r.model), []).append(r.cost_usd)
    for (task, model), costs in sorted(agg.items()):
        table.add_row(task, model, str(len(costs)), f"{sum(costs):.6f}")
    console.print(table)
    console.print(f"Total: [bold]{sum(r.cost_usd for r in rows):.6f} USD[/bold]")


@llm_app.command("models")
def llm_models() -> None:
    import httpx

    rig = Rigger()
    provider = rig.cfg.llm_provider
    table = Table(title="Models")
    table.add_column("ID")
    table.add_column("Prompt $/tok")
    table.add_column("Completion $/tok")

    if provider == "litellm":
        from litellm import model_cost

        for mid in sorted(model_cost):
            p = model_cost[mid]
            table.add_row(
                mid,
                str(p.get("input_cost_per_token", "?")),
                str(p.get("output_cost_per_token", "?")),
            )
    else:
        if provider == "litellm-proxy":
            url = f"{rig.cfg.llm_proxy_base_url.rstrip('/')}/v1/models"
            headers = {"Authorization": f"Bearer {rig.settings.litellm_proxy_key}"}
            title = "LiteLLM Proxy Models"
        else:
            url = "https://openrouter.ai/api/v1/models"
            headers = {"Authorization": f"Bearer {rig.settings.openrouter_api_key}"}
            title = "OpenRouter Models"
        table.title = title
        r = httpx.get(url, headers=headers, timeout=15.0)
        r.raise_for_status()
        data = r.json().get("data", [])
        for m in sorted(data, key=lambda x: x["id"]):
            p = m.get("pricing", {})
            table.add_row(m["id"], str(p.get("prompt", "?")), str(p.get("completion", "?")))
    console.print(table)


# --------------------------------------------------------------------------- #
# config
# --------------------------------------------------------------------------- #
@config_app.command("show")
def config_show() -> None:
    rig = Rigger()
    console.print_json(rig.cfg.model_dump_json())


@config_app.command("validate")
def config_validate() -> None:
    try:
        Rigger()
        console.print("[green]Configuration is valid.[/green]")
    except Exception as exc:
        console.print(f"[red]Invalid configuration: {exc}[/red]")
        raise typer.Exit(1) from exc


if __name__ == "__main__":
    app()
