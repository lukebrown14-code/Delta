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
target_app = typer.Typer(help="Target management")
thesis_app = typer.Typer(help="Thesis management")
llm_app = typer.Typer(help="LLM cost/model inspection")
config_app = typer.Typer(help="Configuration")
app.add_typer(plugins_app, name="plugins")
app.add_typer(target_app, name="target")
app.add_typer(thesis_app, name="thesis")
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


@target_app.command("add")
def target_add(
    name: str,
    market: Annotated[str, typer.Option("--market")],
    kind: Annotated[str, typer.Option("--kind")] = "company",
    tickers: Annotated[str | None, typer.Option("--tickers")] = None,
    tags: Annotated[str | None, typer.Option("--tags")] = None,
    notes: Annotated[str | None, typer.Option("--notes")] = None,
    label: Annotated[str | None, typer.Option("--label")] = None,
) -> None:
    """Add a watch target (company, sector, industry, market, or theme)."""
    try:
        services.add_target(
            name,
            kind=kind,
            market=market,
            tickers=[t.strip() for t in (tickers or "").split(",") if t.strip()],
            tags=[t.strip() for t in (tags or "").split(",") if t.strip()],
            notes=notes or "",
            label=label,
        )
    except (ValueError, KeyError) as exc:
        console.print(f"[red]{exc.args[0]}[/red]")
        raise typer.Exit(1) from exc
    console.print(f"[green]Added target [bold]{name}[/bold] ({kind}).[/green]")


@target_app.command("remove")
def target_remove(name: str) -> None:
    """Delete a watch target."""
    try:
        services.remove_target(name)
    except KeyError as exc:
        console.print(f"[red]{exc.args[0]}[/red]")
        raise typer.Exit(1) from exc
    console.print(f"[green]Removed target [bold]{name}[/bold].[/green]")


@target_app.command("list")
def target_list() -> None:
    """List watch targets."""
    table = Table(title="Targets")
    table.add_column("Name")
    table.add_column("Kind")
    table.add_column("Market")
    table.add_column("Tickers")
    table.add_column("Tags")
    for target in sorted(services.target_specs().values(), key=lambda t: t.id):
        table.add_row(
            target.id,
            target.kind,
            ",".join(target.markets),
            ",".join(target.tickers),
            ",".join(sorted(target.tags)),
        )
    console.print(table)


@target_app.command("show")
def target_show(name: str) -> None:
    """Show one watch target."""
    target = services.target_specs().get(name)
    if target is None:
        console.print(f"[red]Unknown target: {name}[/red]")
        raise typer.Exit(1)
    console.print(
        f"[bold]{target.id}[/bold]  kind={target.kind}  market={','.join(target.markets)}"
    )
    if target.tickers:
        console.print(f"tickers = {', '.join(target.tickers)}")
    if target.tags:
        console.print(f"tags = {', '.join(sorted(target.tags))}")
    if target.notes:
        console.print(f"notes = {target.notes}")
    if target.name != target.id:
        console.print(f"label = {target.name}")


@app.command()
def report(
    target: str,
    since: Annotated[str | None, typer.Option("--since")] = None,
) -> None:
    """Generate an AI research report for a target."""
    from rigger import reports as reports_mod

    rig = Rigger()
    rep = asyncio.run(reports_mod.build_report(rig, target, since=since))
    path = reports_mod.write_report(rep, rig.cfg.reports_dir)
    console.print(f"[green]Report written to {path}[/green]")


@thesis_app.command("create")
def thesis_create(
    claim: str,
    targets: Annotated[str | None, typer.Option("--targets")] = None,
    time_horizon: Annotated[str, typer.Option("--time-horizon")] = "long",
    scope: Annotated[str | None, typer.Option("--scope")] = None,
) -> None:
    """Create a thesis to research for and against."""
    from rigger import theses as theses_mod

    rig = Rigger()
    thesis = theses_mod.create_thesis(
        rig.engine,
        claim,
        scope=scope or "",
        targets=tuple(t.strip() for t in (targets or "").split(",") if t.strip()),
        time_horizon=time_horizon,
    )
    console.print(f"[green]Created thesis [bold]{thesis.id[:12]}[/bold].[/green]")


@thesis_app.command("list")
def thesis_list() -> None:
    """List theses."""
    from rigger import theses as theses_mod

    rig = Rigger()
    table = Table(title="Theses")
    table.add_column("Id")
    table.add_column("Claim")
    table.add_column("Status")
    table.add_column("Horizon")
    for thesis in theses_mod.list_theses(rig.engine):
        table.add_row(thesis.id[:12], thesis.claim[:60], thesis.status, thesis.time_horizon)
    console.print(table)


@thesis_app.command("show")
def thesis_show(thesis_id: str) -> None:
    """Show one thesis and its evidence balance."""
    from rigger import theses as theses_mod

    rig = Rigger()
    thesis = theses_mod.get_thesis(rig.engine, thesis_id)
    if thesis is None:
        console.print(f"[red]Unknown thesis: {thesis_id}[/red]")
        raise typer.Exit(1)
    console.print(f"[bold]{thesis.claim}[/bold]")
    console.print(f"status={thesis.status}  horizon={thesis.time_horizon}")
    rows = theses_mod.evidence_for(rig.engine, thesis.id)
    for side in ("support", "against", "neutral"):
        n = sum(1 for r in rows if r.side == side)
        console.print(f"{side}: {n}")


@thesis_app.command("propose")
def thesis_propose(
    thesis_id: str,
    since: Annotated[str | None, typer.Option("--since")] = None,
) -> None:
    """Ask the model to propose candidate evidence for a thesis."""
    from rigger import theses as theses_mod

    rig = Rigger()
    candidates = asyncio.run(theses_mod.propose_evidence(rig, thesis_id, since=since))
    console.print(f"[green]Proposed {len(candidates)} candidate evidence items.[/green]")


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
