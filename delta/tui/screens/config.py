"""Settings: provider, one model for all tasks, plugins and targets beside diagnostics.

Two columns at 100 columns and up (setup on the left, diagnostics on the
right); one column below that, with diagnostics folded to a one-line summary
until ``d`` expands it full-height. Everything is reachable by key: tab moves
between panes, ↑↓ move inside the focused table, enter acts on the row.
"""

from __future__ import annotations

from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from rich.text import Text
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Static

from delta import services
from delta.core.config import read_env_value
from delta.llm.catalog import ModelInfo, set_llm_model
from delta.llm.providers import PROVIDERS
from delta.tui.screens.model_picker import ModelPicker
from delta.tui.shell import DeltaScreen
from delta.tui.widgets import DeltaTable, Pane, hint_markup


class FocusLine(Static):
    """A one-row static that can take focus, so tab lands on a table-less pane."""

    can_focus = True


class Config(DeltaScreen):
    name = "config"

    #: The keys dodge the app-level bindings (1-5, c, h, m, p, g, q, ?, f2):
    #: ``m`` and ``p`` already open the pickers app-wide and are only surfaced
    #: in the pane hints.
    BINDINGS = [
        ("d", "toggle_diagnostics", "diagnostics"),
        ("r", "refresh", "refresh"),
        ("l", "focus_plugins", "plugins"),
        ("t", "focus_targets", "targets"),
        ("s", "configure_source", "source"),
        ("a", "add_market", "market"),
        ("e", "edit_market", "edit market"),
        ("x", "remove_market", "remove market"),
        ("escape", "close_diagnostics", "back"),
    ]

    #: Below this terminal width the two columns stack and diagnostics folds.
    NARROW_WIDTH = 100
    #: Land on the model row, the pane you most often act in.
    AUTO_FOCUS = "#cfg-model"

    CSS = """
    #cfg-body { height: 1fr; }
    #cfg-left { width: 62; height: 1fr; }
    #cfg-diag { width: 1fr; height: 1fr; }
    #cfg-ai-pane, #cfg-plugins-pane, #cfg-sources-pane { height: auto; }
    #cfg-model, #cfg-plugins { height: auto; max-height: 12; }
    #cfg-plugins-empty, #cfg-sources-hint { height: 1; padding: 0 1; color: $text-muted; }
    #cfg-targets-pane { height: 1fr; }
    #cfg-targets { height: auto; max-height: 12; }
    #cfg-targets-empty, #cfg-targets-legacy, #cfg-targets-signpost { height: 1; padding: 0 1; color: $text-muted; }
    #cfg-targets-line { display: none; height: 1; padding: 0 1; }
    #cfg-diag-body { height: 1fr; }
    #cfg-diag-summary { display: none; height: 1; padding: 0 1; }
    .cfg-heading { height: 1; padding: 0 1; }
    .cfg-heading Static { width: auto; text-style: bold; color: $text-muted; }
    .cfg-heading .cfg-heading-right { width: 1fr; text-align: right; text-style: none; }
    #health-table, #costs-table { height: auto; }
    #health-latest, #costs-total, #costs-today, #cfg-refreshed { height: auto; padding: 0 1; }
    #health-latest, #costs-today, #cfg-refreshed { color: $text-muted; }
    #costs-total { text-style: bold; }
    .cfg-gap { height: 1; }

    /* One column: the left stack sizes to content, diagnostics folds to its
       summary line; expanded, it takes the whole screen. */
    Config.-narrow #cfg-body { layout: vertical; }
    Config.-narrow #cfg-left { width: 1fr; height: auto; }
    Config.-narrow #cfg-diag { width: 1fr; }
    Config.-narrow #cfg-model { max-height: 3; }
    Config.-narrow #cfg-plugins { max-height: 3; }
    Config.-narrow #cfg-targets-pane { height: auto; }
    Config.-narrow #cfg-targets, Config.-narrow #cfg-targets-signpost { display: none; }
    Config.-narrow #cfg-targets-line { display: block; }
    Config.-diag-folded #cfg-diag { height: auto; }
    Config.-diag-folded #cfg-diag-body { display: none; }
    Config.-diag-folded #cfg-diag-summary { display: block; }
    Config.-diag-full #cfg-left { display: none; }
    Config.-diag-full #cfg-diag { height: 1fr; }
    """

    def __init__(self, delta: Any) -> None:
        super().__init__(delta)
        self.ready = False
        #: None follows the width (open wide, folded narrow); ``d`` pins it.
        self._diag_open: bool | None = None
        self._plugin_rows: list[str] = []

    # ------------------------------------------------------------ compose

    def compose_content(self) -> ComposeResult:
        with Horizontal(id="cfg-body"):
            with Vertical(id="cfg-left"):
                with Pane(
                    title="provider & model",
                    key="p",
                    hints=hint_markup(("↑↓", "choose"), ("enter", "change"), ("m", "model")),
                    id="cfg-ai-pane",
                ):
                    yield DeltaTable(id="cfg-model", show_header=False)
                with Pane(
                    title="plugins",
                    key="l",
                    hints=hint_markup(("↑↓", "plugin"), ("enter", "details")),
                    id="cfg-plugins-pane",
                ):
                    yield DeltaTable(id="cfg-plugins", show_header=False)
                    yield Static("no plugins discovered", id="cfg-plugins-empty", markup=False)
                with Pane(
                    title="data sources & markets",
                    key="s",
                    hints=hint_markup(("s", "configure source"), ("a/e/x", "market")),
                    id="cfg-sources-pane",
                ):
                    yield DeltaTable(id="cfg-sources")
                    yield DeltaTable(id="cfg-markets")
                    yield Static("s configure source · a add · e edit · x remove market", id="cfg-sources-hint", markup=False)
                with Pane(
                    title="targets",
                    key="t",
                    hints=hint_markup(("2", "watchlist")),
                    id="cfg-targets-pane",
                ):
                    yield DeltaTable(id="cfg-targets")
                    yield FocusLine("", id="cfg-targets-line")
                    yield Static(
                        "no targets yet — press 1, then a to add one",
                        id="cfg-targets-empty",
                        markup=False,
                    )
                    yield Static("", id="cfg-targets-legacy", markup=False)
                    yield Static("", id="cfg-targets-signpost")
            with Pane(
                title="diagnostics",
                key="d",
                hints=hint_markup(("r", "refresh"), ("d", "fold"), ("↑↓", "scroll")),
                id="cfg-diag",
            ):
                yield Static("", id="cfg-diag-summary")
                with VerticalScroll(id="cfg-diag-body"):
                    with Horizontal(classes="cfg-heading"):
                        yield Static("evidence", markup=False)
                        yield Static(
                            "", id="cfg-db-size", classes="cfg-heading-right", markup=False
                        )
                    yield DeltaTable(id="health-table")
                    yield Static("", id="health-latest", markup=False)
                    yield Static("", classes="cfg-gap")
                    with Horizontal(classes="cfg-heading"):
                        yield Static("model spend · cumulative", markup=False)
                    yield DeltaTable(id="costs-table")
                    yield Static("", id="costs-total", markup=False)
                    yield Static("", id="costs-today", markup=False)
                    yield Static("", classes="cfg-gap")
                    yield Static("", id="cfg-refreshed")

    async def on_mount(self) -> None:
        self.query_one("#health-table", DeltaTable).add_columns("Table", "Rows")
        self.query_one("#costs-table", DeltaTable).add_columns("Task", "Model", "Calls", "USD")
        self.query_one("#cfg-model", DeltaTable).add_columns("", "Status")
        self.query_one("#cfg-plugins", DeltaTable).add_columns("", "Plugin", "State")
        self.query_one("#cfg-sources", DeltaTable).add_columns("Source", "Quality", "Status")
        self.query_one("#cfg-markets", DeltaTable).add_columns("ID", "Market", "Currency", "Yahoo")
        self.query_one("#cfg-targets", DeltaTable).add_columns("Name", "Kind", "Market", "Tickers")
        self.ready = True
        await self.refresh_view()
        self.layout_views()

    # ------------------------------------------------------------ layout

    @property
    def narrow(self) -> bool:
        return self.size.width < self.NARROW_WIDTH

    @property
    def diag_open(self) -> bool:
        return (not self.narrow) if self._diag_open is None else self._diag_open

    def layout_views(self) -> None:
        narrow = self.narrow
        open_ = self.diag_open
        self.set_class(narrow, "-narrow")
        self.set_class(not open_, "-diag-folded")
        self.set_class(narrow and open_, "-diag-full")
        diag = self.query_one("#cfg-diag", Pane)
        if open_:
            hints = [("r", "refresh"), ("d", "fold"), ("↑↓", "scroll")]
            if narrow:
                hints.append(("esc", "close"))
        else:
            hints = [("d", "expand"), ("r", "refresh")]
        diag.set_hints(hint_markup(*hints))

    def on_resize(self) -> None:
        if self.ready:
            self.layout_views()

    # ------------------------------------------------------------ data

    async def refresh_view(self) -> None:
        self._refresh_ai()
        self._refresh_plugins()
        self._refresh_sources()
        self._refresh_markets()
        self._refresh_targets()
        self._refresh_diagnostics()

    def _refresh_ai(self) -> None:
        """Two rows: which provider you use, and the one model for all tasks."""
        cfg = self.delta.cfg
        table = self.query_one("#cfg-model", DeltaTable)
        table.clear()
        tokens = self.app.theme_variables

        name = str(getattr(cfg, "llm_provider", "") or "")
        spec = PROVIDERS.get(name)
        env = (
            (getattr(cfg, "llm_api_key_env", "") or (spec.env_var if spec else "")) if name else ""
        )
        connected = bool(env and read_env_value(env))
        if not name:
            provider = Text("no provider — press p", style=tokens["text-error"])
        elif connected:
            provider = Text.assemble(
                ("● ", tokens["text-success"]),
                (name, "bold"),
                f" · connected · key from {env}",
            )
        else:
            provider = Text.assemble(
                ("○ ", tokens["text-error"]),
                (name, "bold"),
                (f" · no key · {env} unset", tokens["text-warning"]),
            )
        table.add_row("provider", provider, key="provider")

        model = str(getattr(cfg, "llm_model", "") or "")
        model_cell = (
            Text(model) if model else Text("not chosen — press m", style=tokens["text-warning"])
        )
        table.add_row("model", model_cell, key="model")

        badge = f"{name or 'none'} · {model or 'choose'}"
        if name and not connected:
            badge += " · no key"
        self.query_one("#cfg-ai-pane", Pane).set_badge(badge)

    def _refresh_plugins(self) -> None:
        table = self.query_one("#cfg-plugins", DeltaTable)
        table.clear()
        tokens = self.app.theme_variables
        plugins = dict(getattr(self.delta, "plugins", {}) or {})
        self._plugin_rows = sorted(plugins)
        ok = 0
        for name in self._plugin_rows:
            enabled = bool(getattr(plugins[name], "enabled", False))
            ok += enabled
            dot = (
                Text("●", style=tokens["text-success"])
                if enabled
                else Text("○", style=tokens["text-error"])
            )
            label = Text(name) if enabled else Text(name, style=tokens["text-muted"])
            state = Text(
                "enabled" if enabled else "disabled",
                style=tokens["text-muted"] if enabled else tokens["text-error"],
            )
            table.add_row(dot, label, state, key=name)
        table.display = bool(plugins)
        self.query_one("#cfg-plugins-empty", Static).display = not plugins
        self.query_one("#cfg-plugins-pane", Pane).set_badge(
            f"{ok} of {len(plugins)} ok" if plugins else ""
        )

    def _refresh_sources(self) -> None:
        table = self.query_one("#cfg-sources", DeltaTable)
        table.clear()
        sources = services.data_provider_status(self.delta)
        for source in sources:
            table.add_row(
                source.label,
                "primary" if source.primary_disclosure else "secondary",
                "ready" if source.configured and source.enabled else "needs setup",
                key=source.name,
            )
        table.display = bool(sources)

    def _refresh_markets(self) -> None:
        table = self.query_one("#cfg-markets", DeltaTable)
        table.clear()
        for name, profile in sorted(getattr(self.delta.cfg, "markets", {}).items()):
            table.add_row(name, profile.label, profile.currency, profile.yahoo_suffix or "—", key=name)

    def _selected_market(self) -> str | None:
        table = self.query_one("#cfg-markets", DeltaTable)
        if table.cursor_row is None or table.row_count == 0:
            return None
        row_key = table.coordinate_to_cell_key((table.cursor_row, 0)).row_key
        return str(row_key.value) if row_key is not None else None

    async def action_configure_source(self) -> None:
        from delta.tui.screens.source_setup import configure_source

        table = self.query_one("#cfg-sources", DeltaTable)
        if table.cursor_row is None or table.row_count == 0:
            self.notify("select a data source first", severity="warning")
            return
        row_key = table.coordinate_to_cell_key((table.cursor_row, 0)).row_key
        if row_key is not None:
            await configure_source(self.app, self.delta, str(row_key.value), self.refresh_view)

    async def action_add_market(self) -> None:
        from delta.tui.screens.market_setup import MarketSetupModal

        values = await self.app.push_screen_wait(MarketSetupModal())
        if values is not None:
            await self._save_market(values)

    async def action_edit_market(self) -> None:
        from delta.tui.screens.market_setup import MarketSetupModal

        name = self._selected_market()
        if name is None:
            self.notify("select a market first", severity="warning")
            return
        profile = self.delta.cfg.markets[name]
        values = await self.app.push_screen_wait(
            MarketSetupModal(
                {"id": name, "label": profile.label, "currency": profile.currency, "yahoo_suffix": profile.yahoo_suffix},
                editable_id=False,
            )
        )
        if values is not None:
            await self._save_market(values)

    async def _save_market(self, values: dict[str, str]) -> None:
        try:
            services.save_market(**values)
        except ValueError as exc:
            self.notify(str(exc), severity="error")
            return
        reload_markets = getattr(self.delta, "reload_markets", None)
        if callable(reload_markets):
            reload_markets()
        await self.refresh_view()

    async def action_remove_market(self) -> None:
        name = self._selected_market()
        if name is None:
            self.notify("select a market first", severity="warning")
            return
        try:
            services.remove_market(name)
        except ValueError as exc:
            self.notify(str(exc), severity="error")
            return
        reload_markets = getattr(self.delta, "reload_markets", None)
        if callable(reload_markets):
            reload_markets()
        await self.refresh_view()

    def _refresh_targets(self) -> None:
        table = self.query_one("#cfg-targets", DeltaTable)
        table.clear()
        targets = dict(getattr(self.delta.cfg, "targets", {}) or {})
        configured = {name: spec for name, spec in targets.items() if not spec.get("legacy", False)}
        parts: list[str] = []
        for name, spec in sorted(configured.items()):
            kind = str(spec.get("kind", ""))
            market = str(spec.get("market", "")).upper() or "—"
            tickers = ",".join(spec.get("tickers", [])) or "—"
            table.add_row(name, kind, market, tickers, key=name)
            parts.append(" ".join(part for part in (name, kind, market, tickers) if part != "—"))
        tokens = self.app.theme_variables
        line = Text()
        for index, part in enumerate(parts):
            if index:
                line.append("  ·  ", style=tokens["text-muted"])
            head, _, rest = part.partition(" ")
            line.append(head)
            line.append(f" {rest}", style=tokens["text-muted"])
        self.query_one("#cfg-targets-line", FocusLine).update(line)
        legacy = len(targets) - len(configured)
        legacy_note = self.query_one("#cfg-targets-legacy", Static)
        legacy_note.update(
            f"{legacy} legacy universe entr{'y' if legacy == 1 else 'ies'} active · hidden here"
        )
        legacy_note.display = bool(legacy)
        self.query_one("#cfg-targets-empty", Static).display = not configured
        # With no targets the empty line above already names the keys; saying
        # it twice in one pane reads as two different instructions.
        self.query_one("#cfg-targets-signpost", Static).update(
            "[$text-muted]targets live on the[/] [bold $text-primary]1[/] [$text-muted]watchlist —[/]"
            " [bold $text-primary]a[/] [$text-muted]add,[/] [bold $text-primary]d[/] [$text-muted]remove[/]"
            if configured
            else ""
        )
        badge = str(len(configured)) if configured else ""
        if self.narrow and configured:
            badge += " · edit on the 1 watchlist"
        self.query_one("#cfg-targets-pane", Pane).set_badge(badge)

    def _refresh_diagnostics(self) -> None:
        health = services.data_health(self.delta)
        table = self.query_one("#health-table", DeltaTable)
        table.clear()
        for name, count in sorted(health.counts.items()):
            table.add_row(name, f"{count:,}")
        total_rows = sum(health.counts.values())
        latest_parts = [
            f"{name} {_stamp(stamp)}" for name, stamp in sorted(health.latest_bar.items())
        ]
        lines = []
        if latest_parts:
            lines.append("latest bar " + " · ".join(latest_parts))
        else:
            lines.append("no prices gathered yet — press 2, then U to gather")
        if health.last_llm:
            lines.append(f"last model call {_stamp(health.last_llm)}")
        self.query_one("#health-latest", Static).update("\n".join(lines))
        db_path = Path(str(getattr(self.delta.cfg, "db_path", "") or ""))
        size = ""
        try:
            if db_path.is_file():
                size = f"{db_path.name} · {_human_size(db_path.stat().st_size)}"
        except OSError:
            size = ""
        self.query_one("#cfg-db-size", Static).update(size)

        costs = self.query_one("#costs-table", DeltaTable)
        costs.clear()
        cost_rows = services.llm_costs(self.delta.engine)
        for row in cost_rows:
            costs.add_row(row.task, row.model, str(row.calls), f"${row.cost_usd:.3f}")
        total = sum(row.cost_usd for row in cost_rows)
        calls = sum(row.calls for row in cost_rows)
        self.query_one("#costs-total", Static).update(f"total  {calls} calls  ${total:.2f}")
        try:
            today = sum(
                row.cost_usd
                for row in services.llm_costs(
                    self.delta.engine, since=datetime.now(UTC).date().isoformat()
                )
            )
            self.query_one("#costs-today", Static).update(f"today ${today:.2f}")
        except Exception:
            self.query_one("#costs-today", Static).update("")
        now = datetime.now().strftime("%H:%M:%S")
        self.query_one("#cfg-refreshed", Static).update(
            f"[$text-muted]refreshed {now} · press [/][bold $text-primary]r[/]"
            "[$text-muted] to refresh[/]"
        )
        self.query_one("#cfg-diag", Pane).set_badge(f"{total_rows:,} rows · ${total:.2f}")
        newest = max(health.latest_bar.values()) if health.latest_bar else None
        summary = f"{total_rows:,} rows"
        if newest:
            summary += f" · latest bar {_stamp(newest)}"
        summary += f" · spend ${total:.2f}"
        self.query_one("#cfg-diag-summary", Static).update(f"[$text-muted]▸[/] {summary}")

    # ------------------------------------------------------------ events

    def on_data_table_row_selected(self, event: DeltaTable.RowSelected) -> None:
        key = event.row_key.value if event.row_key else None
        if event.data_table.id == "cfg-model" and key == "model":
            self.pick_model()
        elif event.data_table.id == "cfg-model" and key == "provider":
            self.app.action_show_provider_picker()
        elif event.data_table.id == "cfg-plugins" and key:
            self.show_plugin(str(key))

    def pick_model(self) -> None:
        """Open the model picker and save the choice as the one model for all tasks."""
        delta = self.delta
        provider = getattr(getattr(delta, "llm", None), "provider", None)

        def on_select(model: ModelInfo) -> None:
            set_llm_model(model.id)
            self.notify(f"model for all tasks set to {model.id}")
            reload = getattr(delta, "reload_llm", None)
            if callable(reload):
                reload()
            self.run_worker(self.refresh_view(), exclusive=True)

        self.app.push_screen(
            ModelPicker(
                on_select,
                provider=provider,
                provider_name=str(getattr(delta.cfg, "llm_provider", "") or ""),
            )
        )

    def show_plugin(self, name: str) -> None:
        plugin = self.delta.plugins.get(name)
        if plugin is None:
            return
        enabled = bool(getattr(plugin, "enabled", False))
        table = dict(getattr(self.delta.cfg, "plugins", {}) or {}).get(name, {})
        settings = ", ".join(f"{k}={v}" for k, v in sorted(table.items()) if k != "enabled")
        detail = f"{name}: {'enabled' if enabled else 'disabled'} · {type(plugin).__name__}"
        if settings:
            detail += f" · {settings}"
        self.notify(detail, title="plugin")

    # ------------------------------------------------------------ actions

    def action_toggle_diagnostics(self) -> None:
        self._diag_open = not self.diag_open
        self.layout_views()
        if self.diag_open:
            self.query_one("#cfg-diag-body", VerticalScroll).focus()
        elif self.narrow:
            self.query_one("#cfg-model", DeltaTable).focus()

    def action_close_diagnostics(self) -> None:
        if self.narrow and self.diag_open:
            self._diag_open = False
            self.layout_views()
            self.query_one("#cfg-model", DeltaTable).focus()

    async def action_refresh(self) -> None:
        await self.refresh_view()

    def action_focus_plugins(self) -> None:
        self.query_one("#cfg-plugins", DeltaTable).focus()

    def action_focus_targets(self) -> None:
        if self.narrow:
            self.query_one("#cfg-targets-line", FocusLine).focus()
        else:
            self.query_one("#cfg-targets", DeltaTable).focus()


def _stamp(stamp: datetime) -> str:
    if stamp.tzinfo is None:
        stamp = stamp.replace(tzinfo=UTC)
    return stamp.astimezone(UTC).strftime("%d %b %H:%M UTC")


def _human_size(size: int) -> str:
    value = float(size)
    for unit in ("B", "KB", "MB", "GB"):
        if value < 1024 or unit == "GB":
            return f"{value:.0f} {unit}" if unit in ("B", "KB") else f"{value:.1f} {unit}"
        value /= 1024
    return f"{value:.1f} GB"
