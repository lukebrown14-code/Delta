"""Typer CLI exposing the ``rig`` command."""

from __future__ import annotations

import asyncio
from typing import Annotated

import typer
from rich.console import Console
from rich.table import Table

from rigger import services
from rigger.runtime import Rigger

app = typer.Typer(
    help="Rigger — AI investment research assistant",
    invoke_without_command=True,
)
console = Console()
plugins_app = typer.Typer(help="Plugin management")
watchlist_app = typer.Typer(help="Watchlist management")
llm_app = typer.Typer(help="LLM cost/model inspection")
config_app = typer.Typer(help="Configuration")
app.add_typer(plugins_app, name="plugins")
app.add_typer(watchlist_app, name="watchlist")
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


@watchlist_app.command("add")
def watchlist_add(
    name: str,
    market: Annotated[str, typer.Option("--market")],
    tickers: Annotated[str, typer.Option("--tickers")],
    max_pct: Annotated[float | None, typer.Option("--max-pct")] = None,
) -> None:
    """Create a watchlist of tickers."""
    try:
        services.add_watchlist(name, market=market, tickers=tickers.split(","), max_pct=max_pct)
    except (ValueError, KeyError) as exc:
        console.print(f"[red]{exc.args[0]}[/red]")
        raise typer.Exit(1) from exc
    console.print(f"[green]Added watchlist [bold]{name}[/bold].[/green]")


@watchlist_app.command("remove")
def watchlist_remove(name: str) -> None:
    """Delete a watchlist."""
    try:
        services.remove_watchlist(name)
    except KeyError as exc:
        console.print(f"[red]{exc.args[0]}[/red]")
        raise typer.Exit(1) from exc
    console.print(f"[green]Removed watchlist [bold]{name}[/bold].[/green]")


@watchlist_app.command("list")
def watchlist_list() -> None:
    """List watchlists."""
    table = Table(title="Watchlists")
    table.add_column("Name")
    table.add_column("Market")
    table.add_column("Holdings")
    table.add_column("Max")
    for name, spec in sorted(services.watchlist_specs().items()):
        if spec.get("kind", "tickers") != "tickers":
            continue
        max_pct = spec.get("max_pct")
        table.add_row(
            name,
            spec.get("market", ""),
            str(len(spec.get("tickers", []))),
            f"{max_pct:.0f}%" if max_pct is not None else "",
        )
    console.print(table)


@watchlist_app.command("show")
def watchlist_show(name: str) -> None:
    """Show one watchlist's holdings."""
    specs = services.watchlist_specs()
    if name not in specs or specs[name].get("kind", "tickers") != "tickers":
        console.print(f"[red]Unknown watchlist: {name}[/red]")
        raise typer.Exit(1)
    spec = specs[name]
    console.print(
        f"[bold]{name}[/bold]  market={spec['market']}  tickers={', '.join(spec['tickers'])}"
    )
    if spec.get("max_pct") is not None:
        console.print(f"max_pct = {spec['max_pct']}")


@llm_app.command("costs")
def llm_costs(since: Annotated[str | None, typer.Option("--since")] = None) -> None:
    rig = Rigger()
    rows = services.llm_costs(rig.engine, since)
    table = Table(title="LLM Costs")
    table.add_column("Task")
    table.add_column("Model")
    table.add_column("Calls")
    table.add_column("Cost (USD)")
    for row in rows:
        table.add_row(row.task, row.model, str(row.calls), f"{row.cost_usd:.6f}")
    console.print(table)
    console.print(f"Total: [bold]{sum(row.cost_usd for row in rows):.6f} USD[/bold]")


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
