"""Keyboard-first watchlist ledger with ephemeral streaming quotes."""

from __future__ import annotations

import asyncio
from contextlib import suppress
from datetime import datetime
from typing import Any

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.widgets import Button, Input, Label, OptionList, Sparkline, Static
from textual.widgets.option_list import Option

from rigger import services
from rigger.asset_metrics import AssetMetrics, chart_window, fetch_asset_metrics
from rigger.core.models import Instrument
from rigger.plugins.data.yfinance import DEFAULT_SUFFIXES
from rigger.quotes import SearchResult, YahooQuotes, canonical_symbol, yahoo_search
from rigger.tui.shell import RiggerScreen
from rigger.tui.widgets import Dialog, Pane, PaneRow


class WatchlistList(OptionList):
    """Keyboard list with the old table row-count compatibility surface."""

    @property
    def row_count(self) -> int:
        return sum(
            1 for option in self.options if option.id and str(option.id).startswith("target:")
        )


ASSET_CLASS_ORDER = ("equity", "etf", "bond", "commodity", "fx", "crypto", "cash", "other")


def _friendly_date_range(start: str | None, end: str | None) -> str:
    """Format provider timestamps as a compact date range for the inspector."""
    if not start or not end:
        return "no historical dates"
    try:
        first = datetime.fromisoformat(start.replace("Z", "+00:00"))
        last = datetime.fromisoformat(end.replace("Z", "+00:00"))
    except ValueError:
        return f"{start} → {end}"
    if first.date() == last.date():
        return first.strftime("%-d %b %Y")
    if first.year == last.year:
        return f"{first.day} {first.strftime('%b')} – {last.day} {last.strftime('%b')} {last.year}"
    return f"{first.day} {first.strftime('%b %Y')} – {last.day} {last.strftime('%b %Y')}"


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
    TargetAddModal #tg-network.-online { color: $text-success; }
    TargetAddModal #tg-network.-offline { color: $text-warning; }
    TargetAddModal #tg-suggestions { display: none; height: auto; max-height: 6; margin: 0 0 1 10; background: $panel; }
    TargetAddModal #tg-modal-actions { height: 1; margin-top: 1; }
    TargetAddModal #tg-modal-actions Button { height: 1; min-width: 0; border: none; padding: 0 1; margin: 0 1 0 0; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__()
        self.rig = rig
        self._search_task: asyncio.Task | None = None
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
            ("asset-class", "Asset", "equity / crypto / etf"),
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
                                asset_class=getattr(target, "asset_class", "equity"),
                            )
                        )
        for inst in instruments:
            haystack = " ".join((inst.symbol, inst.name or "", inst.market)).casefold()
            key = (inst.market.casefold(), inst.symbol.casefold())
            if needle in haystack and key not in seen:
                seen.add(key)
                results.append(
                    SearchResult(
                        inst.symbol,
                        inst.name or inst.symbol,
                        inst.market,
                        inst.currency,
                        asset_class=getattr(inst, "asset_class", "equity"),
                    )
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
        supported = [result for result in results if result.market in self._suffixes]
        self._show_results(supported or self._local_results(query))

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id != "tg-name":
            return
        if self._suppress_name_search:
            self._suppress_name_search = False
            return
        self._search_generation += 1
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
            self.query_one("#tg-asset-class", Input).value = "equity"
        else:
            self._suppress_name_search = True
            self.query_one("#tg-name", Input).value = selected.name
            self.query_one("#tg-tickers", Input).value = selected.symbol
            self.query_one("#tg-market", Input).value = selected.market
            self.query_one("#tg-asset-class", Input).value = selected.asset_class
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
        if self._search_task:
            self._search_task.cancel()

    def _save(self) -> None:
        name = self._value("name")
        market = self._value("market")
        asset_class = self._value("asset-class").casefold() or "equity"
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
                asset_class=asset_class,
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
        ("r", "cycle_range", "Range"),
        ("a", "add", "Add"),
        ("d", "remove", "Remove"),
        ("slash", "filter", "Filter"),
        ("escape", "cancel", "Close"),
    ]
    CSS = """
    #target-list-pane { height: 1fr; }
    #target-table { height: 1fr; margin: 0; border: none; }
    #target-table > .option-list--option { padding: 0 1; }
    #target-table .tg-group { color: $text-primary; text-style: bold; }
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
    #tg-actions Button:focus { color: $text-primary; text-style: bold; }
    #tg-action-spacer { width: 1fr; height: 1; background: $panel; }
    #tg-empty, #tg-state { height: auto; color: $text-muted; }
    #target-inspector-pane { width: 2fr; min-width: 0; padding: 0 1; }
    #target-inspector-content { width: 1fr; height: 1fr; padding: 0 1; overflow-y: auto; }
    #target-inspector-empty { width: 1fr; height: 1; content-align-vertical: middle; }
    #target-inspector-title { width: 1fr; height: 2; content-align-vertical: middle; }
    #target-inspector-members-label { width: 1fr; height: 1; color: $text-muted; }
    #target-members { width: 1fr; height: auto; max-height: 4; margin-bottom: 1; background: $panel; }
    #target-inspector-hero { width: 1fr; height: 3; margin-bottom: 1; }
    #target-chart-header { width: 1fr; height: 1; }
    #target-chart-label { width: 1fr; color: $text-muted; text-style: bold; }
    #target-chart-change { width: auto; text-style: bold; }
    #target-chart-change.-up { color: $text-success; }
    #target-chart-change.-down { color: $text-error; }
    #target-chart { width: 1fr; height: 7; margin-bottom: 1; padding: 0 1; background: $panel; }
    #target-metric-grid { width: 1fr; height: auto; layout: grid; grid-size: 2; grid-columns: 1fr 1fr; grid-gutter: 1 1; }
    .pane-row.-narrow #target-metric-grid { grid-size: 1; grid-columns: 1fr; }
    .metric-card { width: 1fr; height: auto; min-height: 5; padding: 1; border: solid $border-blurred; background: $surface; }
    .metric-card-title { width: 1fr; height: 1; color: $text-primary; text-style: bold; }
    .metric-card-body { width: 1fr; height: auto; color: $foreground; }
    #target-inspector-title { color: $text-primary; text-style: bold; }
    #target-inspector-status, #target-inspector-empty { color: $text-muted; height: auto; }
    """

    def __init__(self, rig: Any) -> None:
        super().__init__(rig)
        self.rows: dict[str, tuple[str, str | None]] = {}
        self.feed: YahooQuotes | None = None
        self.feed_task: asyncio.Task | None = None
        self.active = False
        self.signature: tuple = ()
        self.specs = {}
        self._metrics: dict[str, AssetMetrics] = {}
        self._selected_instrument: Instrument | None = None
        self._collapsed_groups: set[str] = set()
        self._option_indices: dict[str, int] = {}
        self._collect_task: asyncio.Task | None = None
        self._range = "month"
        self._members_by_target: dict[str, list[str]] = {}
        self._selected_member: dict[str, str] = {}

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="target-split"):
            with Pane(title="watchlist", id="target-list-pane"):
                yield Static("quotes: idle", id="tg-state", markup=False)
                yield Input(
                    placeholder="filter names, tickers, markets, kinds or tags", id="tg-filter"
                )
                yield WatchlistList(id="target-table")
                yield Static("No targets yet — a add your first target.", id="tg-empty")
                yield Static("", id="tg-form")
                with Horizontal(id="tg-actions"):
                    yield Button("enter refresh", id="tg-refresh-hint")
                    yield Button("a add", id="tg-add")
                    yield Button("d remove", id="tg-remove")
                    yield Button("/ filter", id="tg-search")
                    yield Button("r range: month", id="tg-range-hint")
                    yield Button("space groups", id="tg-space-hint")
                    yield Button("esc close", id="tg-close")
                    yield Static("", id="tg-action-spacer")
            with Pane(title="metrics", icon="", id="target-inspector-pane"):
                with Vertical(id="target-inspector-content"):
                    yield Static(
                        "Select a Watchlist item",
                        id="target-inspector-empty",
                        classes="muted",
                        markup=False,
                    )
                    yield Static("", id="target-inspector-title", markup=False)
                    yield Static("", id="target-inspector-members-label", markup=False)
                    yield OptionList(id="target-members")
                    yield Static("", id="target-inspector-hero", markup=False)
                    yield Static("", id="target-inspector-status", classes="muted", markup=False)
                    with Horizontal(id="target-chart-header"):
                        yield Static(
                            "PRICE PERFORMANCE · 1 MONTH", id="target-chart-label", markup=False
                        )
                        yield Static("", id="target-chart-change", markup=False)
                    yield Sparkline([], id="target-chart")
                    with Vertical(id="target-metric-grid"):
                        for index in range(4):
                            with Vertical(classes="metric-card", id=f"metric-card-{index}"):
                                yield Static(
                                    "",
                                    classes="metric-card-title",
                                    id=f"metric-card-title-{index}",
                                    markup=False,
                                )
                                yield Static(
                                    "",
                                    classes="metric-card-body",
                                    id=f"metric-card-body-{index}",
                                    markup=False,
                                )

    def on_mount(self) -> None:
        self.query_one("#tg-filter").display = False
        self.query_one("#tg-form").display = False
        self.refresh_view()
        self.query_one("#target-table").focus()
        self.set_interval(0.5, self._paint_quotes)

    def _selected(self) -> str | None:
        option = self.query_one("#target-table", WatchlistList).highlighted_option
        key = str(option.id) if option and option.id else ""
        return key if key in self.rows else None

    def refresh_view(self) -> None:
        table = self.query_one("#target-table", WatchlistList)
        selected = self._selected()
        table.clear_options()
        self.rows.clear()
        self._option_indices.clear()
        self.specs = services.target_specs()
        specs = sorted(self.specs.values(), key=lambda t: t.id)
        known = {inst.id: inst for inst in self.rig.universe()}
        self.query_one("#target-list-pane", Pane).set_badge(str(len(specs)))
        query = self.query_one("#tg-filter", Input).value.casefold().strip()
        instruments: dict[str, Instrument] = {}
        grouped: dict[str, list[Any]] = {}
        self._members_by_target.clear()
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
                        asset_class=getattr(target, "asset_class", "equity"),
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
            asset_class = str(getattr(target, "asset_class", "equity")).casefold()
            if asset_class not in ASSET_CLASS_ORDER:
                asset_class = "other"
            grouped.setdefault(asset_class, []).append((target, members))
            self._members_by_target[target.id] = members
        order = ASSET_CLASS_ORDER
        for asset_class in order + tuple(sorted(set(grouped) - set(order))):
            entries = grouped.get(asset_class)
            if not entries:
                continue
            group_key = f"group:{asset_class}"
            expanded = group_key not in self._collapsed_groups
            header = (
                f"▾ {asset_class.upper()}  ({len(entries)})"
                if expanded
                else f"▸ {asset_class.upper()}  ({len(entries)})"
            )
            table.add_option(Option(Text(header, style="bold"), id=group_key))
            if not expanded:
                continue
            for target, members in entries:
                key = f"target:{target.id}"
                inst = members[0] if len(members) == 1 else None
                self.rows[key] = (target.id, inst)
                tickers = ",".join(target.tickers) or "market"
                market = ",".join(target.markets)
                tags = ", ".join(sorted(target.tags)) or "—"
                table.add_option(
                    Option(f"  {target.id}  ·  {tickers}  ·  {market}  ·  {tags}", id=key)
                )
        empty = self.query_one("#tg-empty", Static)
        empty.display = not table.row_count
        empty.update(
            "No matching targets." if specs else "No targets yet — a add your first target."
        )
        if selected:
            with suppress(Exception):
                table.highlighted = table.get_option_index(selected)
        elif self.rows:
            table.highlighted = table.get_option_index(next(iter(self.rows)))
        self._instruments = list(instruments.values())
        if self.active:
            self._sync_feed()
        self._paint_quotes()
        self._select_instrument()

    def _select_instrument(self, force: bool = False) -> None:
        key = self._selected()
        if key is None or key not in self.rows:
            self._selected_instrument = None
            self._render_metrics()
            return
        target_id, ident = self.rows[key]
        target = self.specs.get(target_id)
        members = self._members_by_target.get(target_id, []) if target else []
        if members:
            ident = self._selected_member.get(target_id, ident or members[0])
            if ident not in members:
                ident = members[0]
            self._selected_member[target_id] = ident
        self._render_member_selector(target_id, members, ident)
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
        metric = await asyncio.to_thread(
            fetch_asset_metrics, instrument, self._range, self.rig.engine
        )
        self._metrics[instrument.id] = metric
        if self._selected_instrument and self._selected_instrument.id == instrument.id:
            self._render_metrics()

    def _render_metrics(self) -> None:
        instrument = self._selected_instrument
        metric = self._metrics.get(instrument.id) if instrument else None
        empty = self.query_one("#target-inspector-empty", Static)
        title = self.query_one("#target-inspector-title", Static)
        status = self.query_one("#target-inspector-status", Static)
        hero = self.query_one("#target-inspector-hero", Static)
        chart_change = self.query_one("#target-chart-change", Static)
        members_label = self.query_one("#target-inspector-members-label", Static)
        members_widget = self.query_one("#target-members", OptionList)
        cards = [
            (
                self.query_one(f"#metric-card-title-{i}", Static),
                self.query_one(f"#metric-card-body-{i}", Static),
                self.query_one(f"#metric-card-{i}", Vertical),
            )
            for i in range(4)
        ]

        def clear_cards() -> None:
            for card_title, card_body, card in cards:
                card.display = False
                card_title.update("")
                card_body.update("")

        if not instrument:
            selected = self._selected()
            target = self.specs.get(self.rows[selected][0]) if selected in self.rows else None
            empty.update(
                "No ticker proxy configured"
                if target and not target.tickers
                else "Select a Watchlist item"
            )
            title.update(f"{target.id} · {target.kind}" if target else "")
            members_label.update("")
            members_widget.display = False
            status.update(
                "This target has no ticker proxy. Add a representative instrument to view live metrics."
                if target and not target.tickers
                else ""
            )
            hero.update("")
            chart_change.update("")
            chart_change.set_classes("")
            self.query_one("#target-chart", Sparkline).data = []
            clear_cards()
            return
        empty.update("")
        title.update(f"{instrument.symbol} · {instrument.id} · {instrument.asset_class}")
        if metric is None:
            status.update("Loading live metrics…")
            hero.update("")
            chart_change.update("—")
            chart_change.set_classes("")
            self.query_one("#target-chart", Sparkline).data = []
            clear_cards()
            return
        current_label = "Current yield" if metric.profile == "bond" else "Current price"
        current = metric.values.get(current_label, "—")
        quote = self.feed.quotes.get(instrument.id) if self.feed else None
        daily = (
            "—" if quote is None or quote.change_pct is None else f"{quote.change_pct:+.1f}% today"
        )
        tokens = self.app.theme_variables
        up, down, flat = tokens["text-success"], tokens["text-error"], tokens["text-muted"]
        hero_text = Text()
        hero_text.append(current, style=f"bold {tokens['foreground']}")
        hero_text.append("   ")
        hero_text.append(
            daily,
            style=up
            if quote and quote.change_pct and quote.change_pct > 0
            else down
            if quote and quote.change_pct and quote.change_pct < 0
            else flat,
        )
        hero_text.append("   ")
        hero_text.append(
            f"{metric.change_label or '—'} {self._range_label().casefold()}",
            style=up
            if metric.change_label.startswith("+")
            else down
            if metric.change_label.startswith("-")
            else flat,
        )
        hero.update(hero_text)
        if metric.error:
            status.update(f"Metrics unavailable: {metric.error} · press Enter to retry")
        else:
            quote_stamp = quote.timestamp.strftime("%H:%M:%S UTC") if quote else "—"
            history_stamp = _friendly_date_range(metric.history_start, metric.history_end)
            range_context = (
                f"high {metric.period_high:,.2f} · low {metric.period_low:,.2f}"
                if metric.period_high is not None and metric.period_low is not None
                else "range unavailable"
            )
            volatility = (
                f"vol {metric.volatility * 100:.1f}%"
                if metric.volatility is not None
                else "vol unavailable"
            )
            status.update(
                f"{metric.source} · live {quote_stamp} · history {history_stamp}\n"
                f"{range_context} · {volatility}"
            )
        chart_change.update(metric.change_label or "—")
        chart_change.set_class(metric.change_label.startswith("+"), "-up")
        chart_change.set_class(metric.change_label.startswith("-"), "-down")
        self.query_one("#target-chart", Sparkline).data = chart_window(
            metric.series, None if self._range == "all" else 30
        )
        groups = metric.groups or ({"Available Metrics": metric.values} if metric.values else {})
        clear_cards()
        for index, (card_title, card_body, card) in enumerate(cards):
            if index >= len(groups):
                continue
            card.display = True
            card_title.display = True
            card_body.display = True
            group, values = list(groups.items())[index]
            card_title.update(group.upper())
            card_body.update(
                "\n".join(f"{label:<22} {value:>12}" for label, value in values.items())
            )

    def _render_member_selector(
        self, target_id: str, members: list[str], selected: str | None
    ) -> None:
        label = self.query_one("#target-inspector-members-label", Static)
        options = self.query_one("#target-members", OptionList)
        if len(members) < 2:
            label.update("")
            options.display = False
            return
        label.update(f"TICKERS · {len(members)} members")
        options.clear_options()
        for ident in members:
            market, symbol = ident.split(":", 1)
            quote = self.feed.quotes.get(ident) if self.feed else None
            quote_text = ""
            if quote:
                change = "—" if quote.change_pct is None else f"{quote.change_pct:+.1f}%"
                quote_text = f" · {quote.price:,.2f} {change}"
            options.add_option(Option(f"{symbol} · {market}{quote_text}", id=ident))
        options.display = True
        with suppress(Exception):
            options.highlighted = members.index(selected) if selected in members else 0

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
        try:
            table = self.query_one("#target-table", WatchlistList)
        except Exception:
            return
        for key, (_, ident) in self.rows.items():
            target = self.specs.get(self.rows[key][0])
            if target is None:
                continue
            quote = self.feed.quotes.get(ident) if self.feed and ident else None
            if quote is None:
                prompt = f"  {target.id}  ·  {','.join(target.tickers) or 'market'}  ·  {','.join(target.markets)}  ·  {', '.join(sorted(target.tags)) or '—'}"
            else:
                pct = quote.change_pct
                trend = "─" if pct is None or pct == 0 else "▲" if pct > 0 else "▼"
                tokens = self.app.theme_variables
                color = (
                    tokens["text-success"]
                    if pct and pct > 0
                    else tokens["text-error"]
                    if pct and pct < 0
                    else tokens["foreground"]
                )
                prompt = Text(
                    f"  {target.id}  ·  {','.join(target.tickers) or 'market'}  ·  {','.join(target.markets)}  ·  {', '.join(sorted(target.tags)) or '—'}  {quote.price:,.2f} {trend}",
                    style=color,
                )
            with suppress(Exception):
                table.replace_option_prompt(key, prompt)
        selected = self._selected()
        if selected in self.rows:
            target_id, ident = self.rows[selected]
            members = self._members_by_target.get(target_id, [])
            self._render_member_selector(
                target_id, members, ident or self._selected_member.get(target_id)
            )

    def on_option_list_option_highlighted(self, event: Any) -> None:
        if getattr(event.option_list, "id", None) != "target-table":
            return
        self._select_instrument()

    def on_option_list_option_selected(self, event: Any) -> None:
        if getattr(event.option_list, "id", None) == "target-members":
            selected = str(event.option.id)
            target_key = self._selected()
            if target_key in self.rows:
                self._selected_member[self.rows[target_key][0]] = selected
                self._select_instrument(force=True)
            return
        self._select_instrument(force=True)

    def on_key(self, event: Any) -> None:
        if self.focused is not self.query_one("#target-table", WatchlistList):
            return
        option = self.query_one("#target-table", WatchlistList).highlighted_option
        key = str(option.id) if option and option.id else ""
        if key.startswith("group:") and event.key == "space":
            event.stop()
            self._toggle_group(key)

    def _toggle_group(self, key: str | None = None) -> None:
        key = key or self._selected()
        if not key or not key.startswith("group:"):
            return
        if key in self._collapsed_groups:
            self._collapsed_groups.remove(key)
        else:
            self._collapsed_groups.add(key)
        self.refresh_view()

    def action_inspect(self) -> None:
        """Refresh live metrics for the highlighted instrument."""
        self._select_instrument(force=True)

    def _range_label(self) -> str:
        return {"day": "Day", "month": "Month", "all": "All time"}[self._range]

    def action_cycle_range(self) -> None:
        self._range = {"month": "all", "all": "day", "day": "month"}[self._range]
        self.query_one(
            "#tg-range-hint", Button
        ).label = f"r range: {self._range_label().casefold()}"
        self.query_one("#target-chart-label", Static).update(
            f"PRICE PERFORMANCE · {self._range_label().upper()}"
        )
        self._metrics.clear()
        self._select_instrument(force=True)

    def action_filter(self) -> None:
        self.query_one("#tg-filter").display = True
        self.query_one("#tg-filter").focus()

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "tg-filter":
            self.refresh_view()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id in {
            "tg-name",
            "tg-kind",
            "tg-asset-class",
            "tg-market",
            "tg-tickers",
            "tg-tags",
        }:
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
            with suppress(Exception):
                table = self.query_one("#target-table", WatchlistList)
                table.highlighted = table.get_option_index(key)
        self.notify(f"Added {name} to the watchlist")
        target = self.specs.get(name)
        if target and target.tickers and target.markets:
            instruments = [
                Instrument(
                    id=f"{target.markets[0].upper()}:{symbol}",
                    market=target.markets[0],
                    symbol=symbol,
                    currency={"us": "USD", "asx": "AUD"}.get(target.markets[0], ""),
                    asset_class=target.asset_class,
                )
                for symbol in target.tickers
            ]
            if self._collect_task and not self._collect_task.done():
                self._collect_task.cancel()
            self._collect_task = asyncio.create_task(self._collect_target(name, instruments))

    async def _collect_target(self, name: str, instruments: list[Instrument]) -> None:
        self.notify(f"Collecting data for {name}…")
        try:
            result = await services.ingest(self.rig, instruments=instruments)
            self.notify(f"Collected data for {name}: {sum(result.counts.values())} records")
            self._metrics.clear()
            self._select_instrument(force=True)
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            self.notify(f"Data collection failed for {name}: {exc}", severity="error")

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
            "tg-refresh-hint": self.action_inspect,
            "tg-range-hint": self.action_cycle_range,
            "tg-space-hint": self._toggle_group,
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
        if self._collect_task:
            self._collect_task.cancel()
