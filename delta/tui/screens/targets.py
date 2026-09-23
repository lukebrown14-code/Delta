"""Keyboard-first watchlist ledger with ephemeral streaming quotes."""

from __future__ import annotations

import asyncio
from contextlib import suppress
from datetime import UTC, datetime
from functools import partial
from typing import Any

from rich.table import Table
from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.widgets import Button, Input, Label, OptionList, Select, Static
from textual.widgets.option_list import Option

from delta import services
from delta.asset_metrics import (
    METRIC_HELP,
    AssetMetrics,
    chart_window,
    fetch_asset_metrics,
    groups_for,
    profile_for,
)
from delta.core.models import Instrument
from delta.core.plugin import MarketPlugin, discover_plugins
from delta.plugins.data.yfinance import DEFAULT_SUFFIXES
from delta.quotes import SearchResult, canonical_symbol, yahoo_search
from delta.targets import DEFAULT_KIND, KNOWN_KINDS
from delta.tui.axes import format_price
from delta.tui.components import EmptyState, QuoteFeedMixin, SuggestionList, require_selection
from delta.tui.shell import DeltaScreen, age_text
from delta.tui.widgets import (
    MODAL_WIDTH,
    MODAL_WIDTH_WIDE,
    Dialog,
    Pane,
    PaneRow,
    PriceChart,
    hint_markup,
    token_color,
)


class WatchlistList(OptionList):
    """Keyboard list with the old table row-count compatibility surface."""

    @property
    def row_count(self) -> int:
        return sum(
            1 for option in self.options if option.id and str(option.id).startswith("target:")
        )


ASSET_CLASS_ORDER = ("equity", "etf", "bond", "commodity", "fx", "crypto", "cash", "other")

#: The inspector's visible range strip (K8), left to right, plus the display
#: label and the trailing-window size each range shows on the chart.
RANGES = ("1d", "5d", "1m", "6m", "ytd", "1y", "all")
RANGE_LABELS = {
    "1d": "1D",
    "5d": "5D",
    "1m": "1M",
    "6m": "6M",
    "ytd": "YTD",
    "1y": "1Y",
    "all": "ALL",
}
RANGE_WINDOW: dict[str, int | None] = {
    "1d": None,
    "5d": 5,
    "1m": 30,
    "6m": 130,
    "ytd": 250,
    "1y": 252,
    "all": None,
}
DEFAULT_RANGE = "1m"

#: Block-eighth glyphs for the 52-week position bar (K7), from 1/8 to 7/8.
_BAR_EIGHTHS = "▏▎▍▌▋▊▉"


def _fraction_bar(fraction: float, width: int = 16) -> str:
    """A ``▕██▊▏`` bar showing ``fraction`` (0..1) filled over ``width`` cells."""
    fraction = max(0.0, min(1.0, fraction))
    filled = fraction * width
    whole = int(filled)
    bar = "█" * whole
    if whole < width:
        eighth = int((filled - whole) * 8)
        if eighth > 0:
            bar += _BAR_EIGHTHS[min(eighth - 1, len(_BAR_EIGHTHS) - 1)]
            whole += 1
    return "▕" + bar + " " * (width - whole) + "▏"


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
    """Centred search-first form for adding one target to the watchlist.

    Type in the name field and suggestions browse *without leaving the
    input*: arrows move the highlight, typing keeps filtering, enter picks
    and enter again saves. Kind, asset class, market and tickers are derived
    from the picked result; ctrl+t unfolds the advanced fields to override
    them or add more tickers and tags.
    """

    dialog_title = "add to watchlist"
    dialog_hint = hint_markup(
        ("↑↓", "choose"), ("enter", "pick · save"), ("ctrl+t", "more"), ("esc", "cancel")
    )
    dialog_width = MODAL_WIDTH

    BINDINGS = [("ctrl+t", "toggle_more", "More fields")]

    #: Seconds to let the user finish a thought before asking Yahoo.
    SEARCH_DEBOUNCE = 0.3
    #: Suggestion rows kept in reserve even while empty, so the form never
    #: reflows under the cursor when the dropdown appears or closes.
    SUGGESTION_ROWS = 4
    #: Suggestions shown at most; local matches are pinned ahead of remote.
    RESULT_CAP = 8

    DEFAULT_CSS = f"""
    TargetAddModal #tg-form {{ height: auto; }}
    TargetAddModal .tg-field {{ height: 3; }}
    TargetAddModal .tg-field Label {{ width: 10; padding: 1 0; color: $text-muted; }}
    TargetAddModal .tg-field Input {{ width: 1fr; margin: 0; }}
    TargetAddModal .tg-field Select {{ width: 1fr; margin: 0; }}
    TargetAddModal #tg-network {{ height: 1; color: $text-muted; content-align-horizontal: center; }}
    TargetAddModal #tg-network.-online {{ color: $text-success; }}
    TargetAddModal #tg-network.-offline {{ color: $text-warning; }}
    TargetAddModal #tg-suggestions {{
        height: {SUGGESTION_ROWS};
        margin: 0 0 0 10;
        border: none;
        background: $panel;
        scrollbar-size-horizontal: 0;
    }}
    TargetAddModal #tg-summary {{ height: 1; margin: 0 0 0 10; color: $text-muted; }}
    TargetAddModal #tg-advanced {{ display: none; }}
    TargetAddModal.-more #tg-advanced {{ display: block; }}
    TargetAddModal #tg-modal-actions {{ height: 1; margin-top: 1; }}
    TargetAddModal #tg-modal-actions Button {{
        height: 1; min-width: 0; border: none; padding: 0 1; margin: 0 1 0 0;
    }}
    """

    def __init__(self, delta: Any) -> None:
        super().__init__()
        self.delta = delta
        self._search_task: asyncio.Task | None = None
        self._search_timer: Any = None
        self._search_generation = 0
        self._results_by_key: dict[str, SearchResult] = {}
        self._suppress_name_search = False
        self._suffixes = DEFAULT_SUFFIXES | getattr(self.delta.cfg, "plugins", {}).get(
            "yfinance", {}
        ).get("suffixes", {})
        self._suffixes.update(
            {
                name: profile.yahoo_suffix
                for name, profile in getattr(self.delta.cfg, "markets", {}).items()
            }
        )
        self._watched = self._watched_pairs()
        # Configured market profiles and discovered market plugins are both
        # real markets; a target may name either.
        configured = set(getattr(self.delta.cfg, "markets", {}) or {})
        plugin_markets = {
            name for name, plugin in discover_plugins().items() if isinstance(plugin, MarketPlugin)
        }
        self._markets = sorted(plugin_markets | configured) or ["us"]

    def _currency(self, market: str) -> str:
        profile = getattr(self.delta.cfg, "markets", {}).get(market.lower())
        return profile.currency if profile else ""

    def compose_dialog(self) -> ComposeResult:
        yield Static("◌ Yahoo lookup ready", id="tg-network", markup=False)
        yield Vertical(
            Horizontal(
                Label("Name"),
                Input(placeholder="company or ticker — type to search", id="tg-name"),
                classes="tg-field",
            ),
            SuggestionList(id="tg-suggestions"),
            Static("", id="tg-summary", markup=False),
            Vertical(
                Horizontal(
                    Label("Kind"),
                    Select(
                        [(kind.title(), kind) for kind in KNOWN_KINDS],
                        value=DEFAULT_KIND,
                        allow_blank=False,
                        id="tg-kind",
                    ),
                    classes="tg-field",
                ),
                Horizontal(
                    Label("Asset"),
                    Select(
                        [(asset.title(), asset) for asset in ASSET_CLASS_ORDER],
                        value="equity",
                        allow_blank=False,
                        id="tg-asset-class",
                    ),
                    classes="tg-field",
                ),
                Horizontal(
                    Label("Market"),
                    Select(
                        [(market.upper(), market) for market in self._markets],
                        value="us" if "us" in self._markets else self._markets[0],
                        allow_blank=False,
                        id="tg-market",
                    ),
                    classes="tg-field",
                ),
                Horizontal(
                    Label("Tickers"),
                    Input(placeholder="BHP,RIO", id="tg-tickers"),
                    classes="tg-field",
                ),
                Horizontal(
                    Label("Tags"),
                    Input(placeholder="resources,income", id="tg-tags"),
                    classes="tg-field",
                ),
                id="tg-advanced",
            ),
            id="tg-form",
        )
        yield Horizontal(
            Button("Save", id="tg-add", variant="primary"),
            Button("Cancel", id="tg-close"),
            id="tg-modal-actions",
        )

    # ----- lookups --------------------------------------------------------

    def _value(self, field: str) -> str:
        return self.query_one(f"#tg-{field}", Input).value.strip()

    def _select_value(self, field: str) -> str:
        value = self.query_one(f"#tg-{field}", Select).value
        return str(value) if value is not None else ""

    def _set_select(self, field: str, value: str) -> None:
        """Adopt a searched value when the select offers it; never guess."""
        with suppress(Exception):
            self.query_one(f"#tg-{field}", Select).value = value

    def _watched_pairs(self) -> set[tuple[str, str]]:
        """``(market, canonical ticker)`` for every ticker already watched."""
        pairs: set[tuple[str, str]] = set()
        with suppress(Exception):
            for target in services.target_specs().values():
                for market in target.markets:
                    for symbol in target.tickers:
                        pairs.add((market.casefold(), symbol.upper()))
        return pairs

    def _result_key(self, result: SearchResult) -> str:
        return f"{result.market.casefold()}:{result.symbol.casefold()}"

    def _is_watched(self, result: SearchResult) -> bool:
        canonical = canonical_symbol(result.symbol, result.market, self._suffixes)
        return (result.market.casefold(), canonical) in self._watched

    def _local_results(self, query: str) -> list[SearchResult]:
        needle = query.casefold()
        results: list[SearchResult] = []
        seen: set[tuple[str, str]] = set()
        instruments = list(self.delta.universe())
        with suppress(Exception):
            for target in services.target_specs().values():
                for market in target.markets:
                    for symbol in target.tickers:
                        instruments.append(
                            Instrument(
                                id=f"{market.upper()}:{symbol}",
                                market=market,
                                symbol=symbol,
                                currency=self._currency(market),
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
        return results[: self.RESULT_CAP]

    def _merged_results(self, remote: list[SearchResult], query: str) -> list[SearchResult]:
        """Local matches pinned first, remote appended, de-duped by listing."""
        seen: set[str] = set()
        merged: list[SearchResult] = []
        for result in (*self._local_results(query), *remote):
            key = self._result_key(result)
            if key in seen:
                continue
            seen.add(key)
            merged.append(result)
        return merged[: self.RESULT_CAP]

    def _show_results(self, results: list[SearchResult]) -> None:
        self._results_by_key = {self._result_key(result): result for result in results}
        options = []
        for result in results:
            exchange = f" · {result.exchange}" if result.exchange else ""
            prompt = Text(f"{result.symbol} — {result.name} · {result.market.upper()}{exchange}")
            if self._is_watched(result):
                prompt.append("  ✓ watched", style=token_color(self.app, "text-success"))
            options.append(Option(prompt, id=self._result_key(result)))
        self.query_one("#tg-suggestions", SuggestionList).show(options)

    def _set_network(self, text: str, state: str) -> None:
        indicator = self.query_one("#tg-network", Static)
        indicator.remove_class("-online", "-offline", "-pending")
        indicator.add_class(f"-{state}")
        indicator.update(text)

    async def _search(self, query: str, generation: int) -> None:
        try:
            results = await yahoo_search(query)
        except Exception:
            if generation == self._search_generation:
                self._set_network("○ offline · local search", "offline")
            return
        if generation != self._search_generation:
            return
        merged = self._merged_results(results, query)
        self._set_network(f"● online · Yahoo · {len(merged)} matches", "online")
        self._show_results(merged)

    def on_mount(self) -> None:
        self.query_one("#tg-name", Input).focus()
        self._update_summary()

    # ----- reactions ------------------------------------------------------

    def on_input_changed(self, event: Input.Changed) -> None:
        if event.input.id == "tg-tickers":
            self._update_summary()
            return
        if event.input.id != "tg-name":
            return
        if self._suppress_name_search:
            self._suppress_name_search = False
            return
        self._search_generation += 1
        generation = self._search_generation
        if self._search_timer:
            self._search_timer.stop()
            self._search_timer = None
        query = event.value.strip()
        if len(query) < 2:
            self._show_results([])
            self._set_network("◌ type 2+ characters", "pending")
            return
        # Local matches paint instantly; the debounced Yahoo lookup follows.
        self._show_results(self._merged_results([], query))
        self._set_network("◌ searching Yahoo…", "pending")
        self._search_timer = self.set_timer(
            self.SEARCH_DEBOUNCE, lambda: self._kick_search(query, generation)
        )

    def _kick_search(self, query: str, generation: int) -> None:
        if self._search_task:
            self._search_task.cancel()
        self._search_task = asyncio.create_task(self._search(query, generation))

    def on_select_changed(self, event: Select.Changed) -> None:
        self._update_summary()

    def on_key(self, event: Any) -> None:
        """Arrows browse the suggestions while the name input keeps focus."""
        if getattr(self.focused, "id", None) == "tg-name":
            self.query_one("#tg-suggestions", SuggestionList).browse(event)

    def on_option_list_option_selected(self, event: OptionList.OptionSelected) -> None:
        if getattr(event.option_list, "id", None) != "tg-suggestions":
            return
        if not event.option.id:
            return
        event.stop()
        self._pick(str(event.option.id))

    def _pick(self, key: str) -> None:
        """Adopt a suggestion: name, tickers, market and asset class fill in."""
        result = self._results_by_key.get(key)
        if result is None:
            # Results refreshed mid-flight and the object is gone; recover the
            # market from the option key rather than silently defaulting to US.
            market, _, symbol = key.partition(":")
            result = SearchResult(symbol.upper(), symbol.upper(), market or "us", "")
        self._suppress_name_search = True
        self.query_one("#tg-name", Input).value = result.name or result.symbol
        self.query_one("#tg-tickers", Input).value = canonical_symbol(
            result.symbol, result.market, self._suffixes
        )
        self._set_select("market", result.market)
        self._set_select("asset-class", result.asset_class)
        self._close_suggestions()
        self.query_one("#tg-name", Input).focus()

    def _close_suggestions(self) -> None:
        self._show_results([])
        self._set_network("◌ Yahoo lookup ready", "pending")

    def _update_summary(self) -> None:
        """One honest line under the field: exactly what save will write."""
        market = self._select_value("market")
        tickers = ", ".join(
            canonical_symbol(token, market, self._suffixes)
            for token in self._value("tickers").split(",")
            if token.strip()
        )
        title = self._value("name") or "untitled"
        self.query_one("#tg-summary", Static).update(
            f"→ {title} · {self._select_value('kind') or DEFAULT_KIND}"
            f" · {market.upper() if market else '—'}:{tickers or '—'}"
            f" · {self._select_value('asset-class') or 'equity'}"
        )

    # ----- keys and lifecycle ---------------------------------------------

    def on_input_submitted(self, event: Input.Submitted) -> None:
        event.stop()
        # Pick, then confirm: enter with suggestions open adopts one; the
        # next enter (or enter on an empty dropdown) saves.
        if event.input.id == "tg-name" and self._results_by_key:
            options = self.query_one("#tg-suggestions", SuggestionList)
            option = options.get_option_at_index(options.highlighted_index)
            if option.id:
                self._pick(str(option.id))
                return
        self._save()

    def action_toggle_more(self) -> None:
        self.set_class(not self.has_class("-more"), "-more")

    async def on_unmount(self) -> None:
        if self._search_timer:
            self._search_timer.stop()
        if self._search_task:
            self._search_task.cancel()

    def _save(self) -> None:
        name = self._value("name")
        market = self._select_value("market")
        asset_class = self._select_value("asset-class") or "equity"
        if not name or not market:
            self.notify("name and market are required", severity="error")
            return
        try:
            tickers = [
                canonical_symbol(s.strip(), market, self._suffixes)
                for s in self._value("tickers").split(",")
                if s.strip()
            ]
            duplicates = sorted({t for t in tickers if (market.casefold(), t) in self._watched})
            services.add_target(
                name,
                kind=self._select_value("kind") or DEFAULT_KIND,
                market=market,
                tickers=tickers,
                tags=[s.strip() for s in self._value("tags").split(",") if s.strip()],
                asset_class=asset_class,
            )
        except (ValueError, KeyError) as exc:
            self.notify(str(exc), severity="error")
            return
        if duplicates:
            self.notify(
                f"{' , '.join(duplicates)} already watched elsewhere",
                severity="warning",
            )
        self.dismiss(name)

    def on_button_pressed(self, event: Button.Pressed) -> None:
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
        """Escape: close the suggestions first, the dialog second."""
        if self._results_by_key:
            self._close_suggestions()
            self.query_one("#tg-name", Input).focus()
            return
        self.dismiss(None)


class MetricHelpModal(Dialog):
    """Plain-English glossary for the selected instrument's metric cards."""

    dialog_title = "what these metrics mean"
    dialog_hint = hint_markup(("esc", "close"))
    dialog_width = MODAL_WIDTH_WIDE

    DEFAULT_CSS = """
    MetricHelpModal #glossary-body {
        max-height: 22;
        padding: 0 1;
    }
    MetricHelpModal .glossary-group {
        color: $text-primary;
        text-style: bold;
        margin-top: 1;
    }
    MetricHelpModal .glossary-entry {
        color: $text-muted;
        margin: 0 0 0 1;
    }
    """

    def __init__(self, profile: str) -> None:
        super().__init__()
        self.profile = profile

    def compose_dialog(self) -> ComposeResult:
        with VerticalScroll(id="glossary-body"):
            for title, labels in groups_for(self.profile):
                if not labels:
                    continue
                yield Static(title.casefold(), classes="glossary-group", markup=False)
                for label in labels:
                    entry = Text(label, style="bold")
                    help_text = METRIC_HELP.get(label)
                    if help_text:
                        entry.append(f" — {help_text}")
                    yield Static(entry, classes="glossary-entry")


class Targets(QuoteFeedMixin, DeltaScreen):
    name = "targets"
    BINDINGS = [
        ("enter", "inspect", "refresh metrics"),
        ("r", "cycle_range(1)", "range"),
        ("R", "cycle_range(-1)", "range"),
        ("i", "metric_help", "glossary"),
        ("a", "add", "add"),
        ("d", "remove", "remove"),
        ("slash", "filter", "filter"),
        ("space", "toggle_group", "fold"),
        ("left", "member(-1)", "member"),
        ("right", "member(1)", "member"),
        ("escape", "cancel", "back"),
    ]
    #: Column widths of a watchlist row: name, last, change, age.
    COLUMNS = (12, 10, 8, 4)
    CSS = """
    #target-list-pane { width: 46; min-width: 46; }
    #target-inspector-pane { width: 1fr; min-width: 0; }
    Targets.-narrow #target-list-pane, Targets.-narrow #target-inspector-pane { width: 1fr; }
    #target-table { height: 1fr; margin: 0; border: none; background: transparent; }
    #target-table > .option-list--option { padding: 0 1; }
    #target-table > .option-list--option-disabled { color: $text-muted; text-style: none; }
    #target-table .tg-group { color: $text-primary; text-style: bold; }
    #tg-filter { margin: 0 0 0 0; }
    #tg-empty { height: auto; padding: 0 1; color: $text-muted; }
    #target-inspector-content { width: 1fr; height: 1fr; padding: 0 1; overflow-y: auto; }
    #target-inspector-empty { width: 1fr; height: auto; color: $text-muted; }
    #target-inspector-title { width: 1fr; height: 1; color: $text-primary; text-style: bold; }
    #target-inspector-hero { width: 1fr; height: 1; }
    #target-range-strip { width: 1fr; height: 1; margin: 0; }
    #target-range-tabs { width: auto; color: $text-muted; }
    #target-range-summary { width: 1fr; content-align-horizontal: right; color: $text-muted; }
    #target-chart { width: 1fr; height: 1fr; min-height: 6; max-height: 16; padding: 0 1;
                   background: transparent; }
    #target-metric-grid { width: 1fr; height: auto; }
    #target-inspector-source { width: 1fr; height: auto; margin: 1 0 0 0; color: $text-muted; }
    """

    def __init__(self, delta: Any) -> None:
        super().__init__(delta)
        self.rows: dict[str, tuple[str, str | None]] = {}
        self.active = False
        self.specs = {}
        #: Cached metrics per (instrument id, range): range switches must not
        #: refetch what the provider already answered for that window.
        self._metrics: dict[tuple[str, str], AssetMetrics] = {}
        self._selected_instrument: Instrument | None = None
        self._collapsed_groups: set[str] = set()
        self._option_indices: dict[str, int] = {}
        self._collect_task: asyncio.Task | None = None
        self._range = DEFAULT_RANGE
        self._members_by_target: dict[str, list[str]] = {}
        self._selected_member: dict[str, str] = {}
        self.detail_open = False

    def compose_content(self) -> ComposeResult:
        with PaneRow(id="target-split"):
            with Pane(
                title="watchlist",
                hints=hint_markup(
                    ("a", "add"), ("d", "remove"), ("/", "filter"), ("space", "fold")
                ),
                id="target-list-pane",
            ):
                yield Input(
                    placeholder="/ filter names, tickers, markets, kinds or tags", id="tg-filter"
                )
                yield WatchlistList(id="target-table")
                yield EmptyState("no targets yet", key="a", action="add one", id="tg-empty")
            with Pane(title="metrics", hints=self._metric_hints(), id="target-inspector-pane"):
                with Vertical(id="target-inspector-content"):
                    yield Static("", id="target-inspector-empty", markup=False)
                    yield Static("", id="target-inspector-title", markup=False)
                    yield Static("", id="target-inspector-hero", markup=False)
                    with Horizontal(id="target-range-strip"):
                        yield Static("", id="target-range-tabs", markup=False)
                        yield Static("", id="target-range-summary", markup=False)
                    yield PriceChart([], id="target-chart")
                    yield Static("", id="target-metric-grid", markup=False)
                    yield Static("", id="target-inspector-source", markup=False)

    def _colours(self) -> dict[str, str]:
        """Theme tokens as colours Rich can parse — never a raw theme value."""
        return {
            token: token_color(self.app, token)
            for token in (
                "foreground",
                "text-muted",
                "text-primary",
                "text-success",
                "text-error",
                "text-warning",
            )
        }

    def _metric_hints(self) -> str:
        pairs = [
            ("enter", "refresh"),
            ("r/R", f"range: {self._range_label()}"),
            ("i", "glossary"),
        ]
        target_key = self._selected() if self.is_mounted else None
        if target_key and len(self._members_by_target.get(self.rows[target_key][0], [])) > 1:
            pairs.append(("←→", "member"))
        if self.has_class("-narrow"):
            pairs.append(("esc", "back"))
        return hint_markup(*pairs)

    def layout_views(self) -> None:
        """Wide: both panes. Narrow: the list, or the metrics after enter."""
        narrow = self.apply_breakpoint()
        self.query_one("#target-list-pane").display = not (narrow and self.detail_open)
        self.query_one("#target-inspector-pane").display = not narrow or self.detail_open
        self.query_one("#target-inspector-pane", Pane).set_hints(self._metric_hints())

    def on_resize(self) -> None:
        if self.is_mounted:
            self.layout_views()

    def on_mount(self) -> None:
        self.query_one("#tg-filter").display = False
        self.layout_views()
        self.refresh_view()
        self.query_one("#target-table").focus()
        self.set_interval(0.5, self._paint_quotes)

    def _selected(self) -> str | None:
        key = self._highlighted_key()
        return key if key in self.rows else None

    def refresh_view(self) -> None:
        table = self.query_one("#target-table", WatchlistList)
        selected = self._selected()
        table.clear_options()
        self.rows.clear()
        self._option_indices.clear()
        name_w, last_w, chg_w, age_w = self.COLUMNS
        table.add_option(
            Option(
                f"  {'Name':<{name_w}}{'Last':>{last_w}}  {'Chg%':>{chg_w}}  {'Age':>{age_w}}",
                disabled=True,
            )
        )
        self.specs = services.target_specs()
        specs = sorted(self.specs.values(), key=lambda t: t.id)
        known = {inst.id: inst for inst in self.delta.universe()}
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
                        currency=getattr(self.delta.cfg, "markets", {}).get(market).currency
                        if market in getattr(self.delta.cfg, "markets", {})
                        else "",
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
            table.add_option(
                Option(self._group_prompt(asset_class, entries, expanded), id=group_key)
            )
            if not expanded:
                continue
            for target, members in entries:
                key = f"target:{target.id}"
                inst = members[0] if len(members) == 1 else None
                self.rows[key] = (target.id, inst)
                table.add_option(Option(self._row_prompt(target, members), id=key))
        empty = self.query_one("#tg-empty", EmptyState)
        empty.display = not table.row_count
        if specs:
            empty.set_message("no targets match the filter", key="esc", action="clear it")
        else:
            empty.set_message("no targets yet", key="a", action="add one")
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
        instrument = (
            next((item for item in self.delta.universe() if item.id == ident), None)
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
        # Paint the loading state first so the pane never shows a stale card.
        self._render_metrics()
        metric = await asyncio.to_thread(
            fetch_asset_metrics, instrument, self._range, self.delta.engine
        )
        key = (instrument.id, self._range)
        self._metrics[key] = metric
        self._cap_metrics(key)
        if self._selected_instrument and self._selected_instrument.id == instrument.id:
            self._render_metrics()

    def _cap_metrics(self, keep: tuple[str, str]) -> None:
        """Bound the cache; eviction is insertion-oldest, never the current key."""
        while len(self._metrics) > 12:
            for key in self._metrics:
                if key != keep:
                    del self._metrics[key]
                    break
            else:
                break

    def _render_metrics(self) -> None:
        instrument = self._selected_instrument
        metric = self._metrics.get((instrument.id, self._range)) if instrument else None
        empty = self.query_one("#target-inspector-empty", Static)
        title = self.query_one("#target-inspector-title", Static)
        hero = self.query_one("#target-inspector-hero", Static)
        source = self.query_one("#target-inspector-source", Static)
        grid = self.query_one("#target-metric-grid", Static)
        self.query_one("#target-inspector-pane", Pane).set_hints(self._metric_hints())

        def clear_chart() -> None:
            chart = self.query_one("#target-chart", PriceChart)
            chart.times = []
            chart.data = []
            source.update("")

        def clear_cards() -> None:
            grid.update("")

        selected = self._selected()
        target = self.specs.get(self.rows[selected][0]) if selected in self.rows else None
        members = self._members_by_target.get(target.id, []) if target else []
        if not instrument:
            empty.display = True
            if target and not target.tickers:
                message = "no tickers on this target — metrics need at least one ticker"
            elif not self.rows:
                message = "no targets yet — press a to add one"
            else:
                message = "no target selected — ↑↓ picks one"
            empty.update(message)
            title.update(f"{target.id} · {target.kind}" if target else "")
            hero.update("")
            self._render_range_strip(metric=None)
            clear_chart()
            clear_cards()
            return
        empty.display = False
        name = getattr(instrument, "name", "") or instrument.symbol
        member_note = (
            f" · member {members.index(instrument.id) + 1}/{len(members)}"
            if instrument.id in members and len(members) > 1
            else ""
        )
        tokens = self._colours()
        # K7 row 1: name left, exchange · class · currency right.
        meta = " · ".join(
            part
            for part in (
                instrument.market.upper(),
                instrument.asset_class,
                instrument.currency,
            )
            if part
        )
        title.update(
            Text.assemble(
                (name, f"bold {tokens['foreground']}"),
                (f"  {meta}{member_note}", tokens["text-muted"]),
            )
        )
        if metric is None:
            hero.update(Text("loading metrics…", style=tokens["text-muted"]))
            self._render_range_strip(metric=None)
            clear_chart()
            clear_cards()
            return
        current_label = "Current yield" if metric.profile == "bond" else "Current price"
        current = metric.values.get(current_label, "—")
        quote = self.quote_for(instrument.id)
        up, down, flat = tokens["text-success"], tokens["text-error"], tokens["text-muted"]
        hero_text = Text()
        hero_text.append(current, style=f"bold {tokens['foreground']}")
        hero_text.append(f" {instrument.currency}" if instrument.currency else "", style=flat)
        hero_text.append("   ")
        if quote is not None and quote.change_pct is not None:
            pct = quote.change_pct
            arrow = "▲" if pct > 0 else "▼" if pct < 0 else "─"
            style = up if pct > 0 else down if pct < 0 else flat
            hero_text.append(f"{arrow} {pct:+.2f}%", style=style)
            hero_text.append("  today", style=flat)
        else:
            if metric.history_end:
                closed = _friendly_date_range(metric.history_end, metric.history_end)
                hero_text.append(f"closed · last {closed}", style=flat)
            else:
                hero_text.append("— today", style=flat)
        hero_text.append("   ")
        # K7: a 52-week position bar sits on the same row.
        if (
            metric.week_52_high is not None
            and metric.week_52_low is not None
            and metric.week_52_high > metric.week_52_low
            and metric.series
        ):
            price = metric.series[-1]
            fraction = (price - metric.week_52_low) / (metric.week_52_high - metric.week_52_low)
            hero_text.append("52w ", style=flat)
            hero_text.append(_fraction_bar(fraction), style=tokens["text-primary"])
            hero_text.append(f" {fraction * 100:.0f}% of high", style=flat)
        hero.update(hero_text)
        if metric.error:
            source.update(f"metrics unavailable: {metric.error} — press enter to retry")
        else:
            history = _friendly_date_range(metric.history_start, metric.history_end)
            quote_stamp = quote.timestamp.strftime("%H:%M:%S UTC") if quote else "—"
            source.update(f"{metric.source} · live {quote_stamp} · history {history}")
        self._render_range_strip(metric=metric)
        chart = self.query_one("#target-chart", PriceChart)
        window, window_times = chart_window(
            metric.series, self._range_window(), metric.series_times
        )
        chart.times = window_times
        chart.y_format = partial(
            format_price, kind="yield" if metric.profile == "bond" else "price"
        )
        chart.data = window
        grid.update(self._metric_grid(metric, tokens))

    def _render_range_strip(self, metric: AssetMetrics | None = None) -> None:
        """The visible range tabs plus the range's change and hi/lo (K8, J10)."""
        tokens = self._colours()
        active = self._range
        tabs = Text()
        for index, label in enumerate(RANGES):
            if index:
                tabs.append("  ")
            display = RANGE_LABELS[label]
            if label == active:
                tabs.append(display, style=f"bold {tokens['text-primary']}")
            else:
                tabs.append(display, style="")
        self.query_one("#target-range-tabs", Static).update(tabs)
        summary = Text()
        if metric is not None and metric.change_label:
            label = metric.change_label
            arrow = "▲" if label.startswith("+") else "▼" if label.startswith("-") else "─"
            style = (
                tokens["text-success"]
                if label.startswith("+")
                else (tokens["text-error"] if label.startswith("-") else tokens["text-muted"])
            )
            summary.append(f"{arrow} {label}", style=style)
            if metric.period_high is not None and metric.period_low is not None:
                summary.append(
                    f"   hi {metric.period_high:,.2f}   lo {metric.period_low:,.2f}",
                    style=tokens["text-muted"],
                )
        self.query_one("#target-range-summary", Static).update(summary)

    def _metric_grid(self, metric: AssetMetrics, tokens: dict[str, str]) -> Table:
        """A borderless two-column key/value grid under muted headings (J8)."""
        groups = metric.groups or ({"Available Metrics": metric.values} if metric.values else {})
        table = Table.grid(expand=True, padding=(0, 1))
        table.add_column(ratio=1, no_wrap=True, overflow="ellipsis")
        table.add_column(justify="right", no_wrap=True)
        table.add_column(ratio=1, no_wrap=True, overflow="ellipsis")
        table.add_column(justify="right", no_wrap=True)
        for title, values in groups.items():
            if not values:
                continue
            heading = Text(title, style=f"bold {tokens['text-muted']}")
            table.add_row(heading, "", "", "")
            pairs = list(values.items())
            for index in range(0, max(len(pairs), 1), 2):
                left_label, left_raw = pairs[index]
                left_value = Text(left_raw, style=tokens["foreground"])
                if index + 1 < len(pairs):
                    right_label, right_raw = pairs[index + 1]
                    right_value = Text(right_raw, style=tokens["foreground"])
                else:
                    right_label, right_value = "", Text("")
                table.add_row(left_label, left_value, right_label, right_value)
        return table

    def _sync_feed(self) -> None:
        self.sync_quotes(self._instruments)

    def _row_prompt(self, target: Any, members: list[str]) -> Text:
        """One watchlist row: name, last, change, quote age — columns, not prose.

        Only the change cell carries colour, so the cursor row stays readable
        and the eye scans one column for what moved.
        """
        name_w, last_w, chg_w, age_w = self.COLUMNS
        tokens = self._colours()
        ident = members[0] if len(members) == 1 else self._selected_member.get(target.id)
        if len(members) == 1:
            name = members[0].split(":", 1)[1]
        elif members:
            name = f"{target.id} ▸{len(members)}"
        else:
            name = target.id
        quote = self.quote_for(ident)
        row = Text(f"  {name[:name_w]:<{name_w}}")
        if quote is None:
            row.append(
                f"{'—':>{last_w}}  {'—':>{chg_w}}  {'':>{age_w}}", style=tokens["text-muted"]
            )
            return row
        pct = quote.change_pct
        row.append(f"{quote.price:,.2f}"[:last_w].rjust(last_w), style=tokens["foreground"])
        row.append("  ")
        row.append(
            f"{'—' if pct is None else f'{pct:+.2f}%':>{chg_w}}",
            style=tokens["text-success"]
            if pct and pct > 0
            else tokens["text-error"]
            if pct and pct < 0
            else tokens["text-muted"],
        )
        age, state = age_text(datetime.now(UTC) - quote.received_at)
        row.append("  ")
        row.append(
            f"{age:>{age_w}}",
            style=tokens["text-muted"] if state == "ok" else tokens["text-warning"],
        )
        return row

    def _group_prompt(self, asset_class: str, entries: list[Any], expanded: bool) -> Text:
        """Group header with the mean move of its members that have a quote."""
        name_w, last_w, chg_w, _age_w = self.COLUMNS
        tokens = self._colours()
        arrow = "▾" if expanded else "▸"
        row = Text(f"{arrow} {asset_class} ", style=f"bold {tokens['text-primary']}")
        row.append(f"({len(entries)})", style=tokens["text-muted"])
        moves = [
            quote.change_pct
            for _target, members in entries
            for quote in (self.quote_for(members[0] if len(members) == 1 else None),)
            if quote is not None and quote.change_pct is not None
        ]
        if moves:
            mean = sum(moves) / len(moves)
            pad = 2 + name_w + last_w + 2 - len(row.plain)
            row.append(" " * max(1, pad))
            row.append(
                f"{mean:+.2f}%".rjust(chg_w),
                style=tokens["text-success"]
                if mean > 0
                else tokens["text-error"]
                if mean < 0
                else tokens["text-muted"],
            )
        return row

    def _paint_quotes(self) -> None:
        if not self.is_mounted:
            return
        try:
            table = self.query_one("#target-table", WatchlistList)
        except Exception:
            return
        grouped: dict[str, list[Any]] = {}
        for key, (target_id, _ident) in self.rows.items():
            target = self.specs.get(target_id)
            if target is None:
                continue
            members = self._members_by_target.get(target_id, [])
            asset_class = str(getattr(target, "asset_class", "equity")).casefold()
            grouped.setdefault(
                asset_class if asset_class in ASSET_CLASS_ORDER else "other", []
            ).append((target, members))
            with suppress(Exception):
                table.replace_option_prompt(key, self._row_prompt(target, members))
        for asset_class, entries in grouped.items():
            group_key = f"group:{asset_class}"
            with suppress(Exception):
                table.replace_option_prompt(
                    group_key,
                    self._group_prompt(
                        asset_class, entries, group_key not in self._collapsed_groups
                    ),
                )
        live = self.feed is not None and any(
            self.quote_for(ident) for _t, ident in self.rows.values() if ident
        )
        self.query_one("#target-list-pane", Pane).set_badge(
            f"{'● live' if live else '○ idle'} · {len(self.rows)}"
        )

    def on_option_list_option_highlighted(self, event: Any) -> None:
        if getattr(event.option_list, "id", None) != "target-table":
            return
        self._select_instrument()

    def on_option_list_option_selected(self, event: Any) -> None:
        self.action_inspect()

    def action_toggle_group(self) -> None:
        self._toggle_group()

    def action_member(self, delta: int) -> None:
        """Step through the tickers of a multi-ticker target."""
        key = self._selected()
        if key is None:
            return
        target_id = self.rows[key][0]
        members = self._members_by_target.get(target_id, [])
        if len(members) < 2:
            return
        current = self._selected_member.get(target_id, members[0])
        index = members.index(current) if current in members else 0
        self._selected_member[target_id] = members[(index + delta) % len(members)]
        self._select_instrument(force=True)
        self._paint_quotes()

    def _highlighted_key(self) -> str:
        """The highlighted option's id, group headers included.

        ``_selected`` answers "which target row?" and so drops the group
        headers; folding is the one action that wants them.
        """
        option = self.query_one("#target-table", WatchlistList).highlighted_option
        return str(option.id) if option and option.id else ""

    def _toggle_group(self, key: str | None = None) -> None:
        key = key or self._highlighted_key()
        if not key or not key.startswith("group:"):
            return
        if key in self._collapsed_groups:
            self._collapsed_groups.remove(key)
        else:
            self._collapsed_groups.add(key)
        self.refresh_view()
        # ``refresh_view`` restores the highlight from ``_selected``, which only
        # knows target rows — leave it to that and a collapsed group could never
        # be reopened, because the cursor would have jumped off its header.
        with suppress(Exception):
            table = self.query_one("#target-table", WatchlistList)
            table.highlighted = table.get_option_index(key)

    def action_inspect(self) -> None:
        """Refresh live metrics for the highlighted instrument; narrow: open them."""
        if self.has_class("-narrow") and not self.detail_open:
            self.detail_open = True
            self.layout_views()
        self._select_instrument(force=True)

    def action_metric_help(self) -> None:
        """Open the plain-English glossary for the selected instrument's profile."""
        instrument = self._selected_instrument
        if not require_selection(self, instrument, "a target"):
            return
        self.app.push_screen(MetricHelpModal(profile_for(instrument)))

    def _range_label(self) -> str:
        return RANGE_LABELS.get(self._range, RANGE_LABELS[DEFAULT_RANGE])

    def _range_window(self) -> int | None:
        return RANGE_WINDOW.get(self._range, RANGE_WINDOW[DEFAULT_RANGE])

    def action_cycle_range(self, delta: int = 1) -> None:
        index = RANGES.index(self._range) if self._range in RANGES else RANGES.index(DEFAULT_RANGE)
        self._range = RANGES[(index + delta) % len(RANGES)]
        self.query_one("#target-inspector-pane", Pane).set_hints(self._metric_hints())
        self._render_range_strip()
        self._select_instrument(force=True)

    def action_filter(self) -> None:
        if self.detail_open:
            self.detail_open = False
            self.layout_views()
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
        """Escape: close the filter, or back out of the narrow metrics view."""
        self.query_one("#tg-filter", Input).value = ""
        self.query_one("#tg-filter").display = False
        if self.detail_open:
            self.detail_open = False
            self.layout_views()
        self.query_one("#target-table").focus()

    def action_add(self) -> None:
        self.app.push_screen(TargetAddModal(self.delta), self._target_added)

    def _target_added(self, name: str | None) -> None:
        if not name:
            return
        self.refresh_view()
        key = f"target:{name}"
        if key in self.rows:
            with suppress(Exception):
                table = self.query_one("#target-table", WatchlistList)
                table.highlighted = table.get_option_index(key)
        self.notify(f"added {name} to the watchlist")
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
        self.notify(f"gathering evidence for {name}…")
        try:
            result = await services.ingest(self.delta, instruments=instruments)
            self.notify(f"gathered {sum(result.counts.values())} records for {name}")
            self._metrics.clear()
            self._select_instrument(force=True)
        except asyncio.CancelledError:
            raise
        except Exception as exc:
            self.notify(
                f"could not gather evidence for {name}: {exc} — press c to check the provider",
                severity="error",
            )

    def action_remove(self) -> None:
        key = self._selected()
        if key is None or not require_selection(self, not key.startswith("child:"), "a target"):
            return
        name = self.rows[key][0]
        try:
            services.remove_target(name)
        except (ValueError, KeyError) as exc:
            self.notify(str(exc), severity="error")
            return
        self.refresh_view()
        self.notify(f"removed {name} from the watchlist")

    async def on_screen_resume(self) -> None:
        self.active = True
        await super().on_screen_resume()

    async def _stop_feed(self) -> None:
        self.active = False
        await self.stop_quotes()

    async def on_screen_suspend(self) -> None:
        await self._stop_feed()

    async def on_unmount(self) -> None:
        await self._stop_feed()
        if self._collect_task:
            self._collect_task.cancel()
