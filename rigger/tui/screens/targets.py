"""Keyboard-first watchlist ledger with ephemeral streaming quotes."""

from __future__ import annotations

import asyncio
from contextlib import suppress
from datetime import UTC, datetime
from typing import Any

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Button, DataTable, Input, Label, OptionList, Sparkline, Static
from textual.widgets.option_list import Option

from rigger import services
from rigger.asset_metrics import AssetMetrics, chart_window, fetch_asset_metrics
from rigger.core.models import Instrument
from rigger.plugins.data.yfinance import DEFAULT_SUFFIXES
from rigger.quotes import SearchResult, YahooQuotes, canonical_symbol, yahoo_search
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Dialog, Pane, PaneRow, RiggerTable


class TargetAddModal(Dialog):
    """Centered terminal form for adding one watch target."""

    dialog_title = "add to watchlist"
    dialog_hint = "<Enter>: save   <Esc>: cancel"
    dialog_width = 64

    DEFAULT_CSS = """
    TargetAddModal #tg-form { height: auto; }
    TargetAddModal .tg-field { height: 3; }
    TargetAddModal .tg-field Label { width: 10; padding: 1 0; color: $text-muted; }
    TargetAddModal .tg-field Input { width: 1fr; margin: 0; }
    TargetAddModal #tg-network { height: 1; color: $text-muted; content-align-horizontal: center; }
    TargetAddModal #tg-network.-online { color: $success; }
    TargetAddModal #tg-network.-offline { color: $warning; }
    TargetAddModal #tg-suggestions { display: none; height: auto; max-height: 6; margin: 0 0 1 10; background: $panel; }
    TargetAddModal #tg-modal-actions { height: 1; margin-top: 1; }
    TargetAddModal #tg-modal-actions Button { height: 1; min-width: 0; border: none; padding: 0 1; margin: 0 1 0 0; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__()
        self.rig = rig
        self._search_task: asyncio.Task | None = None
        self._search_debounce_task: asyncio.Task | None = None
        self._search_generation = 0
        self._results_by_symbol: dict[str, SearchResult] = {}
        self._suppress_name_search = False
        self._suffixes = DEFAULT_SUFFIXES | getattr(self.rig.cfg, "plugins", {}).get(
            "yfinance", {}
        ).get("suffixes", {})

    def compose_dialog(self) -> ComposeResult:
        fields = []
        for field, label, placeholder in (
            ("name", "Name", "company or ticker"),
            ("kind", "Kind", "company"),
            ("market", "Market", "us / asx"),
            ("tickers", "Tickers", "BHP,RIO"),
            ("tags", "Tags", "resources,income"),
        ):
            fields.append(
                Horizontal(
                    Label(label),
                    Input(placeholder=placeholder, id=f"tg-{field}"),
                    classes="tg-field",
                )
            )
            if field == "name":
                fields.append(OptionList(id="tg-suggestions"))
        yield Static("◌ Yahoo lookup ready", id="tg-network", markup=False)
        yield Vertical(*fields, id="tg-form")
        yield Horizontal(
            Button("Save", id="tg-add", variant="primary"),
            Button("Cancel", id="tg-close"),
            id="tg-modal-actions",
        )

    def _value(self, field: str) -> str:
        return self.query_one(f"#tg-{field}", Input).value.strip()

    def on_mount(self) -> None:
        self.query_one("#tg-name", Input).focus()

    def _set_network(self, text: str, state: str) -> None:
        indicator = self.query_one("#tg-network", Static)
        indicator.remove_class("-online", "-offline", "-pending")
        indicator.add_class(f"-{state}")
        indicator.update(text)

    def _local_results(self, query: str) -> list[SearchResult]:
        needle = query.casefold()
        results: list[SearchResult] = []
        seen: set[tuple[str, str]] = set()
        instruments = list(self.rig.universe())
        with suppress(Exception):
            for target in services.target_specs().values():
                for market in target.markets:
                    for symbol in target.tickers:
                        instruments.append(
                            Instrument(
                                id=f"{market.upper()}:{symbol}",
                                market=market,
                                symbol=symbol,
                                currency={"us": "USD", "asx": "AUD"}.get(market, ""),
                            )
                        )
        for inst in instruments:
            haystack = " ".join((inst.symbol, inst.name or "", inst.market)).casefold()
            key = (inst.market.casefold(), inst.symbol.casefold())
            if needle in haystack and key not in seen:
                seen.add(key)
                results.append(
                    SearchResult(inst.symbol, inst.name or inst.symbol, inst.market, inst.currency)
                )
        return results[:8]

    def _show_results(self, results: list[SearchResult]) -> None:
        options = self.query_one("#tg-suggestions", OptionList)
        options.clear_options()
        self._results_by_symbol = {result.symbol: result for result in results}
        for result in results:
            exchange = f" · {result.exchange}" if result.exchange else ""
            options.add_option(
                Option(
                    f"{result.symbol} — {result.name} · {result.market.upper()}{exchange}",
                    id=result.symbol,
                )
            )
        options.display = bool(results)

    async def _search(self, query: str, generation: int) -> None:
        self._show_results(self._local_results(query))
        try:
            results = await yahoo_search(query)
        except Exception:
            if generation == self._search_generation:
                self._set_network("○ offline · local search", "offline")
            return
        if generation != self._search_generation:
            return
        self._set_network("● online · Yahoo", "online")
        self._show_results(results or self._local_results(query))

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id != "tg-name":
            return
        if self._suppress_name_search:
            self._suppress_name_search = False
            return
        self._search_generation += 1
        if self._search_debounce_task:
            self._search_debounce_task.cancel()
        if self._search_task:
            self._search_task.cancel()
        query = event.value.strip()
        if not query:
            self.query_one("#tg-suggestions", OptionList).display = False
            self._set_network("◌ Yahoo lookup ready", "pending")
            return
        if len(query) < 2:
            self.query_one("#tg-suggestions", OptionList).display = False
            self._set_network("◌ type 2+ characters", "pending")
            return
        # Show local matches immediately while the debounced Yahoo lookup runs.
        self._show_results(self._local_results(query))
        self._set_network("◌ searching Yahoo…", "pending")
        self._search_task = asyncio.create_task(self._search(query, self._search_generation))

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        symbol = event.option.id
        if not symbol:
            return
        selected = self._results_by_symbol.get(str(symbol))
        if selected is None:
            # A remote-only result is represented by its symbol; the market
            # defaults to US until the user changes it.
            self.query_one("#tg-tickers", Input).value = str(symbol)
            self.query_one("#tg-market", Input).value = "us"
        else:
            self._suppress_name_search = True
            self.query_one("#tg-name", Input).value = selected.name
            self.query_one("#tg-tickers", Input).value = selected.symbol
            self.query_one("#tg-market", Input).value = selected.market
        self.query_one("#tg-suggestions", OptionList).display = False
        self.query_one("#tg-kind", Input).focus()

    def on_key(self, event: Any) -> None:
        control = getattr(event, "control", None) or self.focused
        if (
            event.key in {"down", "up"}
            and getattr(control, "id", None) == "tg-name"
            and self.query_one("#tg-suggestions", OptionList).display
        ):
            event.stop()
            options = self.query_one("#tg-suggestions", OptionList)
            options.highlighted = 0 if event.key == "down" else max(0, len(options.options) - 1)
            options.focus()

    async def on_unmount(self) -> None:
        if self._search_debounce_task:
            self._search_debounce_task.cancel()
        if self._search_task:
            self._search_task.cancel()

    def _save(self) -> None:
        name = self._value("name")
        market = self._value("market")
        if not name or not market:
            self.notify("name and market are required", severity="error")
            return
        try:
            tickers = [
                canonical_symbol(s.strip(), market, self._suffixes)
                for s in self._value("tickers").split(",")
                if s.strip()
            ]
            services.add_target(
                name,
                kind=self._value("kind") or "company",
                market=market,
                tickers=tickers,
                tags=[s.strip() for s in self._value("tags").split(",") if s.strip()],
            )
        except (ValueError, KeyError) as exc:
            self.notify(str(exc), severity="error")
            return
        self.dismiss(name)

    def on_input_submitted(self, event: Input.Submitted) -> None:
        event.stop()
        self._save()

    async def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "tg-add":
            self._save()
        elif event.button.id == "tg-close":
            self.dismiss(None)

    def on_click(self, event: Any) -> None:
        control = getattr(event, "control", None)
        if getattr(control, "id", None) == "tg-add":
            self._save()
        elif getattr(control, "id", None) == "tg-close":
            self.dismiss(None)

    def action_dismiss_dialog(self) -> None:
        suggestions = self.query_one("#tg-suggestions", OptionList)
        focused = self.focused
        if suggestions.display or getattr(focused, "id", None) == "tg-name":
            suggestions.display = False
            self.query_one("#tg-name", Input).focus()
            return
        self.dismiss(None)


class Targets(RiggerScreen):
    name = "targets"
    BINDINGS = [
        ("enter", "inspect", "Refresh metrics"),
        ("a", "add", "Add"),
        ("d", "remove", "Remove"),
        ("slash", "filter", "Filter"),
        ("escape", "cancel", "Close"),
    ]
    CSS = """
    #target-list-pane { height: 1fr; }
    #target-table { height: 1fr; margin: 0; }
    #tg-filter { height: 3; }
    #tg-form { height: auto; max-height: 18; overflow-y: auto; }
    .tg-field { height: 3; }
    .tg-field Label { width: 10; padding: 1 0; color: $text-muted; }
    .tg-field Input { width: 1fr; margin: 0; }
    #tg-actions {
        height: 1;
        margin-bottom: 0;
        padding: 0;
    }
    #tg-actions Button { height: 1; min-width: 0; border: none; padding: 0 1; margin: 0; background: $panel; color: $text-muted; }
    #tg-actions Button:focus { color: $primary; text-style: bold; }
    #tg-action-spacer { width: 1fr; height: 1; background: $panel; }
    #tg-empty, #tg-details, #tg-state { height: auto; color: $text-muted; }
    #tg-details { padding: 0 1; }
    #target-inspector-pane { width: 2fr; }
    #target-inspector-title, #target-inspector-status, #target-metrics { height: auto; }
    #target-inspector-title { color: $primary; text-style: bold; }
    #target-inspector-status, #target-inspector-empty { color: $text-muted; }
    #target-chart { height: 5; width: 1fr; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self.rows: dict[str, tuple[str, str | None]] = {}
        self.feed: YahooQuotes | None = None
        self.feed_task: asyncio.Task | None = None
        self.active = False
        self.signature: tuple = ()
        self.narrow = False
        self.specs = {}
        self._metrics: dict[str, AssetMetrics] = {}
        self._selected_instrument: Instrument | None = None

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="target-split"):
            with Pane(title="watchlist", id="target-list-pane"):
                yield Static("quotes: idle", id="tg-state", markup=False)
                yield Input(
                    placeholder="filter names, tickers, markets, kinds or tags", id="tg-filter"
                )
                yield RiggerTable(id="target-table")
                yield Static("No targets yet — a add your first target.", id="tg-empty")
                yield Static("", id="tg-details", markup=False)
                yield Static("", id="tg-form")
                with Horizontal(id="tg-actions"):
                    yield Button("/ filter", id="tg-search")
                    yield Button("a add", id="tg-add")
                    yield Button("d remove", id="tg-remove")
                    yield Button("esc close", id="tg-close")
                    yield Static("", id="tg-action-spacer")
            with Pane(title="metrics", icon="", id="target-inspector-pane"):
                yield Static("Select a Watchlist item", id="target-inspector-empty", markup=False)
                yield Static("", id="target-inspector-title", markup=False)
                yield Static("", id="target-inspector-status", markup=False)
                yield Sparkline([], id="target-chart")
                yield Static("", id="target-metrics", markup=False)

    def on_mount(self) -> None:
        self.query_one("#tg-filter").display = False
        self.query_one("#tg-form").display = False
        self.query_one("#target-table", RiggerTable).zebra_stripes = False
        self._columns()
        self.refresh_view()
        self.query_one("#target-table").focus()
        self.set_interval(0.5, self._paint_quotes)

    def _columns(self) -> None:
        table = self.query_one("#target-table", RiggerTable)
        table.clear(columns=True)
        for key, label, width in [
            ("name", "NAME / TICKER", 25 if not self.narrow else 20),
            ("market", "MARKET", 7),
            ("price", "PRICE", 12),
            ("currency", "CCY", 4),
            ("change", "DAY %", 11),
        ]:
            table.add_column(label, key=key, width=width)
        if not self.narrow:
            table.add_column("AGE", key="age", width=8)

    def on_resize(self, event: Any) -> None:
        narrow = event.size.width < 100
        if narrow != self.narrow and self.is_mounted:
            self.narrow = narrow
            self._columns()
            self.refresh_view()

    def _selected(self) -> str | None:
        table = self.query_one("#target-table", RiggerTable)
        if not table.row_count:
            return None
        return str(table.coordinate_to_cell_key(table.cursor_coordinate).row_key.value)

    def refresh_view(self) -> None:
        table = self.query_one("#target-table", RiggerTable)
        selected, cursor = self._selected(), table.cursor_row
        scroll = table.scroll_offset
        table.clear()
        self.rows.clear()
        self.specs = services.target_specs()
        specs = sorted(self.specs.values(), key=lambda t: t.id)
        known = {inst.id: inst for inst in self.rig.universe()}
        self.query_one("#target-list-pane", Pane).set_badge(str(len(specs)))
        query = self.query_one("#tg-filter", Input).value.casefold().strip()
        instruments: dict[str, Instrument] = {}
        for target in specs:
            members = []
            for market in target.markets:
                for symbol in target.tickers:
                    ident = f"{market.upper()}:{symbol}"
                    instruments[ident] = Instrument(
                        id=ident,
                        market=market,
                        symbol=symbol,
                        currency={"us": "USD", "asx": "AUD"}.get(market, ""),
                    )
                    if ident in known:
                        instruments[ident] = known[ident]
                    members.append(ident)
            if (
                query
                and query
                not in " ".join(
                    [
                        target.id,
                        target.name,
                        target.kind,
                        *target.markets,
                        *target.tickers,
                        *target.tags,
                    ]
                ).casefold()
            ):
                continue
            key = f"target:{target.id}"
            inst = members[0] if len(members) == 1 else None
            self.rows[key] = (target.id, inst)
            values = [
                target.id,
                ",".join(target.markets),
                f"{len(members)} tickers" if len(members) > 1 else "—",
                "",
                "—",
            ]
            table.add_row(*(values + ([] if self.narrow else ["—"])), key=key)
        empty = self.query_one("#tg-empty", Static)
        empty.display = not table.row_count
        empty.update(
            "No matching targets." if specs else "No targets yet — a add your first target."
        )
        keys = list(self.rows)
        table.move_cursor(
            row=keys.index(selected) if selected in keys else min(cursor, max(0, len(keys) - 1))
        )
        table.scroll_to(scroll.x, scroll.y, animate=False, force=True)
        self._instruments = list(instruments.values())
        if self.active:
            self._sync_feed()
        self._paint_quotes()
        self._details()
        self._select_instrument()

    def _select_instrument(self, force: bool = False) -> None:
        key = self._selected()
        if key is None or key not in self.rows:
            self._selected_instrument = None
            self._render_metrics()
            return
        target_id, ident = self.rows[key]
        target = self.specs.get(target_id)
        if ident is None and target is not None and target.tickers and target.markets:
            ident = f"{target.markets[0].upper()}:{target.tickers[0]}"
        instrument = (
            next((item for item in self.rig.universe() if item.id == ident), None)
            if ident
            else None
        )
        if instrument is None and ident:
            market, symbol = ident.split(":", 1)
            instrument = Instrument(
                id=ident,
                market=market.lower(),
                symbol=symbol,
                currency="",
                asset_class=getattr(target, "asset_class", "equity"),
            )
        self._selected_instrument = instrument
        if instrument:
            if force or instrument.id not in self._metrics:
                self.fetch_metrics(instrument)
            else:
                self._render_metrics()

    @work(exclusive=True, thread=False)
    async def fetch_metrics(self, instrument: Instrument) -> None:
        self.query_one("#target-inspector-status", Static).update("Loading live metrics…")
        metric = await asyncio.to_thread(fetch_asset_metrics, instrument)
        self._metrics[instrument.id] = metric
        if self._selected_instrument and self._selected_instrument.id == instrument.id:
            self._render_metrics()

    def _render_metrics(self) -> None:
        instrument = self._selected_instrument
        metric = self._metrics.get(instrument.id) if instrument else None
        empty = self.query_one("#target-inspector-empty", Static)
        title = self.query_one("#target-inspector-title", Static)
        status = self.query_one("#target-inspector-status", Static)
        if not instrument:
            empty.update("Select a Watchlist item")
            title.update("")
            status.update("")
            self.query_one("#target-chart", Sparkline).data = []
            self.query_one("#target-metrics", Static).update("")
            return
        empty.update("")
        title.update(f"{instrument.symbol} · {instrument.id} · {instrument.asset_class}")
        if metric is None:
            status.update("Loading live metrics…")
            return
        status.update(metric.error or f"1 month: {metric.change_label or 'insufficient history'}")
        self.query_one("#target-chart", Sparkline).data = chart_window(metric.series)
        self.query_one("#target-metrics", Static).update(
            "\n".join(f"{label:<24} {value}" for label, value in metric.values.items())
            or "No additional metrics available"
        )

    def _sync_feed(self) -> None:
        suffixes = DEFAULT_SUFFIXES | getattr(self.rig.cfg, "plugins", {}).get("yfinance", {}).get(
            "suffixes", {}
        )
        signature = (
            tuple(sorted(i.id for i in self._instruments)),
            tuple(sorted(suffixes.items())),
        )
        if self.feed_task and not self.feed_task.done() and signature == self.signature:
            return
        old_task = self.feed_task
        if old_task:
            old_task.cancel()
        old_quotes = self.feed.quotes if self.feed else {}
        self.signature = signature
        self.feed = YahooQuotes(self._instruments, suffixes, self._quote_state)
        self.feed.quotes.update({k: v for k, v in old_quotes.items() if k in signature[0]})
        feed = self.feed

        async def start() -> None:
            if old_task:
                with suppress(asyncio.CancelledError):
                    await old_task
            await feed.run()

        self.feed_task = asyncio.create_task(start())

    def _quote_state(self, state: str) -> None:
        if self.is_mounted:
            self.query_one("#tg-state", Static).update(
                f"quotes: {state} · Yahoo · age is quote age"
            )

    def _paint_quotes(self) -> None:
        if not self.is_mounted:
            return
        table = self.query_one("#target-table", RiggerTable)
        for key, (_, ident) in self.rows.items():
            if ident is None:
                continue
            quote = self.feed.quotes.get(ident) if self.feed else None
            if quote is None:
                continue
            pct = quote.change_pct
            color = (
                self.app.current_theme.success
                if pct and pct > 0
                else self.app.current_theme.error
                if pct and pct < 0
                else self.app.current_theme.foreground
            )
            label = (
                "—" if pct is None else f"{'▲' if pct > 0 else '▼' if pct < 0 else '─'} {pct:+.2f}%"
            )
            table.update_cell(key, "price", Text(f"{quote.price:,.2f}", justify="right"))
            table.update_cell(key, "currency", quote.currency)
            table.update_cell(key, "change", Text(label, style=color, justify="right"))
            if not self.narrow:
                age = max(0, int((datetime.now(UTC) - quote.timestamp).total_seconds()))
                label = (
                    f"{age}s" if age < 60 else f"{age // 60}m" if age < 3600 else f"{age // 3600}h"
                )
                table.update_cell(key, "age", label)
        self._details()

    def _details(self) -> None:
        selected = self._selected()
        detail = self.query_one("#tg-details", Static)
        if selected not in self.rows:
            detail.update("")
            return
        name, ident = self.rows[selected]
        target = self.specs.get(name)
        if target is None:
            detail.update("")
            return
        quote = self.feed.quotes.get(ident) if self.feed and ident else None
        detail.update(
            f"{target.id} · {target.kind} · {','.join(target.tickers) or 'no tickers'}\n"
            f"tags: {', '.join(sorted(target.tags)) or '—'}"
            + (f" · quote: {quote.timestamp:%Y-%m-%d %H:%M:%S} UTC" if quote else "")
        )

    def on_data_table_row_highlighted(self, event: DataTable.RowHighlighted) -> None:
        self._details()
        self._select_instrument()

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        self._select_instrument(force=True)

    def action_inspect(self) -> None:
        """Refresh live metrics for the highlighted instrument."""
        self._select_instrument(force=True)

    def action_filter(self) -> None:
        self.query_one("#tg-filter").display = True
        self.query_one("#tg-filter").focus()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "tg-filter":
            self.refresh_view()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id in {"tg-name", "tg-kind", "tg-market", "tg-tickers", "tg-tags"}:
            event.stop()
            self.action_add()

    def action_cancel(self) -> None:
        self.query_one("#tg-filter", Input).value = ""
        self.query_one("#tg-filter").display = False
        self.query_one("#tg-form").display = False
        self.query_one("#tg-add", Button).label = "a add"
        self.query_one("#target-table").focus()

    def action_add(self) -> None:
        self.app.push_screen(TargetAddModal(self.rig), self._target_added)

    def _target_added(self, name: str | None) -> None:
        if not name:
            return
        self.refresh_view()
        key = f"target:{name}"
        if key in self.rows:
            self.query_one("#target-table", RiggerTable).move_cursor(row=list(self.rows).index(key))
        self.notify(f"Added {name} to the watchlist")

    def action_remove(self) -> None:
        key = self._selected()
        if key is None or key.startswith("child:"):
            self.notify("Select a watchlist target first", severity="error")
            return
        name = self.rows[key][0]
        try:
            services.remove_target(name)
        except (ValueError, KeyError) as exc:
            self.notify(str(exc), severity="error")
            return
        self.refresh_view()
        self.notify(f"Removed {name} from the watchlist")

    def on_button_pressed(self, event: Button.Pressed) -> None:
        actions = {
            "tg-add": self.action_add,
            "tg-remove": self.action_remove,
            "tg-search": self.action_filter,
            "tg-close": self.action_cancel,
        }
        if event.button.id in actions:
            actions[event.button.id]()

    async def on_screen_resume(self) -> None:
        self.active = True
        await super().on_screen_resume()

    async def _stop_feed(self) -> None:
        self.active = False
        if self.feed_task:
            self.feed_task.cancel()
            with suppress(asyncio.CancelledError):
                await self.feed_task
            self.feed_task = None

    async def on_screen_suspend(self) -> None:
        await self._stop_feed()

    async def on_unmount(self) -> None:
        await self._stop_feed()
