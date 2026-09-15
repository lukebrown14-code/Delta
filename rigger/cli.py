"""Typer CLI exposing the ``rig`` command."""

from __future__ import annotations

import asyncio
from typing import Annotated

import typer
from rich.console import Console
from rich.table import Table

from rigger import services
from rigger.runtime import Rigger, _store_signal  # noqa: F401  # re-exported for tests

app = typer.Typer(
    help="Rigger — AI investment research and paper trading",
    invoke_without_command=True,
)
console = Console()
plugins_app = typer.Typer(help="Plugin management")
paper_app = typer.Typer(help="Paper portfolio management")
llm_app = typer.Typer(help="LLM cost/model inspection")
config_app = typer.Typer(help="Configuration")
app.add_typer(plugins_app, name="plugins")
app.add_typer(paper_app, name="paper")
app.add_typer(llm_app, name="llm")
app.add_typer(config_app, name="config")


@app.callback()
def main(ctx: typer.Context) -> None:
    if ctx.invoked_subcommand is None:
        from rigger.tui.app import run_tui

        run_tui()


@app.command()
def tui() -> None:
    """Open the terminal UI."""
    from rigger.tui.app import run_tui

    run_tui()


@plugins_app.command("list")
def plugins_list(type_: Annotated[str | None, typer.Option("--type")] = None) -> None:
    from rigger.core.plugin import discover_plugins

    table = Table(title="Plugins")
    table.add_column("Name")
    table.add_column("Type")
    table.add_column("Version")
    for name, plugin in sorted(discover_plugins().items()):
        ptype = type(plugin).__mro__[1].__name__.removesuffix("Plugin").lower()
        if type_ and ptype != type_:
            continue
        table.add_row(name, ptype, plugin.version)
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
    services.set_plugin_enabled(rig, name, value)
    console.print(f"{'Enabled' if value else 'Disabled'} [bold]{name}[/bold]")


@app.command()
def ingest(
    market: Annotated[str | None, typer.Option("--market")] = None,
    tickers: Annotated[str | None, typer.Option("--tickers")] = None,
    since: Annotated[str | None, typer.Option("--since")] = None,
) -> None:
    """Fetch market data into the database."""
    rig = Rigger()
    asyncio.run(
        services.ingest(rig, market=market, tickers=tickers, since=since, log=console.print)
    )
    console.print("[green]Ingest complete.[/green]")


@app.command()
def extract(since: Annotated[str | None, typer.Option("--since")] = None) -> None:
    """Turn unprocessed news into structured events."""
    rig = Rigger()
    asyncio.run(services.extract(rig, since=since, log=console.print))


@app.command()
def analyse(
    strategy: Annotated[str | None, typer.Option("--strategy")] = None,
    dry_run: Annotated[bool, typer.Option("--dry-run")] = False,
) -> None:
    """Generate signals via strategy plugins."""
    rig = Rigger()
    asyncio.run(services.analyse(rig, strategies=strategy, dry_run=dry_run, log=console.print))


@app.command()
def execute(
    since: Annotated[str | None, typer.Option("--since")] = None,
    all_: Annotated[bool, typer.Option("--all")] = False,
) -> None:
    """Route pending signals through risk and broker."""
    rig = Rigger()
    asyncio.run(services.execute(rig, since=since, all_=all_, log=console.print))


@app.command()
def report(
    format: Annotated[str, typer.Option("--format")] = "markdown",
    date: Annotated[str | None, typer.Option("--date")] = None,
) -> None:
    """Render a report for a date."""
    rig = Rigger()
    try:
        path = services.report(rig, date=date, fmt=format)
    except KeyError as exc:
        console.print(f"[red]{exc.args[0]}[/red]")
        raise typer.Exit(1) from exc
    console.print(f"[green]Report written to {path}[/green]")


@app.command()
def run() -> None:
    """ingest → extract → analyse → execute → report."""
    rig = Rigger()
    asyncio.run(services.ingest(rig, log=console.print))
    asyncio.run(services.extract(rig, log=console.print))
    asyncio.run(services.analyse(rig, log=console.print))
    asyncio.run(services.execute(rig, log=console.print))
    path = services.report(rig)
    console.print(f"[green]Report written to {path}[/green]")


@paper_app.command("status")
def paper_status() -> None:
    rig = Rigger()
    summary = services.portfolio_summary(rig)
    table = Table(title="Paper Portfolio")
    table.add_column("Instrument")
    table.add_column("Qty")
    table.add_column("Avg Price")
    table.add_column(f"Value ({summary['base_currency']})")
    for row in summary["positions"]:
        table.add_row(
            row["instrument_id"],
            f"{row['qty']:.4f}",
            f"{row['avg_price']:.2f} {row['currency']}",
            f"{row['value']:,.2f}",
        )
    console.print(table)
    console.print(f"Cash: [bold]{summary['cash']:,.2f} {summary['base_currency']}[/bold]")
    console.print(f"Equity: [bold]{summary['equity']:,.2f} {summary['base_currency']}[/bold]")


@paper_app.command("reset")
def paper_reset(
    signals: Annotated[bool, typer.Option("--signals", help="Also delete stored signals")] = False,
) -> None:
    """Wipe orders, fills, positions and cash."""
    rig = Rigger()
    services.reset_paper(rig, signals=signals)
    kept = "" if signals else " Signals kept."
    console.print(f"[green]Paper portfolio reset.{kept}[/green]")


@llm_app.command("costs")
def llm_costs(since: Annotated[str | None, typer.Option("--since")] = None) -> None:
    rig = Rigger()
    agg = services.llm_costs(rig.engine, since)
    table = Table(title="LLM Costs")
    table.add_column("Task")
    table.add_column("Model")
    table.add_column("Calls")
    table.add_column("Cost (USD)")
    for (task, model), costs in sorted(agg.items()):
        table.add_row(task, model, str(len(costs)), f"{sum(costs):.6f}")
    console.print(table)
    console.print(f"Total: [bold]{sum(sum(v) for v in agg.values()):.6f} USD[/bold]")


@llm_app.command("models")
def llm_models() -> None:
    import httpx

    rig = Rigger()
    table = Table(title="Models")
    table.add_column("ID")
    table.add_column("Prompt $/tok")
    table.add_column("Completion $/tok")
    if rig.cfg.llm_provider == "litellm":
        from litellm import model_cost

        for model, pricing in sorted(model_cost.items()):
            table.add_row(
                model,
                str(pricing.get("input_cost_per_token", "?")),
                str(pricing.get("output_cost_per_token", "?")),
            )
    else:
        if rig.cfg.llm_provider == "litellm-proxy":
            url = f"{rig.cfg.llm_proxy_base_url.rstrip('/')}/v1/models"
            headers = {"Authorization": f"Bearer {rig.settings.litellm_proxy_key}"}
            table.title = "LiteLLM Proxy Models"
        else:
            url = "https://openrouter.ai/api/v1/models"
            headers = {"Authorization": f"Bearer {rig.settings.openrouter_api_key}"}
            table.title = "OpenRouter Models"
        response = httpx.get(url, headers=headers, timeout=15.0)
        response.raise_for_status()
        for model in sorted(response.json().get("data", []), key=lambda item: item["id"]):
            pricing = model.get("pricing", {})
            table.add_row(
                model["id"], str(pricing.get("prompt", "?")), str(pricing.get("completion", "?"))
            )
    console.print(table)


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
