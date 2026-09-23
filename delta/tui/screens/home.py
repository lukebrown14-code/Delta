"""Home: the desk overview — a watchlist, an activity pulse and a to-do agenda.

A ``DeltaScreen`` like every other panel, so the status bar is there where
a new user lands and ``h`` round-trips cleanly. One header row (an inked
``DELTA`` chip, "overview", the live clock) sits above three ``PaneRow``s:

* top — ``watchlist`` (live quotes when the feed is up, last closes
  otherwise, 40-close sparks) and ``since you last looked`` (new evidence,
  the pulse histogram, stale bars and stale reports);
* middle — ``upcoming`` (calendar events) and ``theses`` (fleet health);
* bottom — ``needs you today`` (the agenda: reviews due, falsifier hits,
  earnings in the next week, stale sources — each line a jump key).

Below 40 rows the middle row goes first and its content folds into two
summary lines under the watchlist. Below 100 columns only the watchlist and
the agenda survive. On a first run with no targets, the whole grid is
replaced by the setup checklist (one key per step).

Every letter key on Home is an app-level binding; the screen only adds the
arrows, enter and tab, so nothing here can shadow navigation.
"""

from __future__ import annotations

from contextlib import suppress
from datetime import UTC, datetime, timedelta
from typing import Any

from rich.text import Text
from textual import work
from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical
from textual.markup import escape
from textual.message import Message
from textual.widgets import DataTable, OptionList, Static

from delta import decisions, review, services
from delta.core.models import Instrument
from delta.core.state import read_last_seen
from delta.core.time import to_utc
from delta.quotes import Quote
from delta.tui.components import QuoteFeedMixin, goto
from delta.tui.shell import NARROW_WIDTH, DeltaScreen, age_text
from delta.tui.widgets import (
    BrailleGraph,
    DeltaTable,
    Pane,
    PaneRow,
    hint_markup,
    token_color,
)

PULSE_DAYS = 30
WATCH_ROWS = 7
UPCOMING_ROWS = 5
THESES_ROWS = 6
SPARK_CLOSES = 40
#: A report older than this is called out beside the stale-bar warnings.
REPORT_STALE = timedelta(days=7)
#: How often the watchlist repaints from the quote feed.
QUOTE_PAINT_SECONDS = 0.5

#: Below this height the middle row folds into the watchlist summary lines.
TALL_HEIGHT = 40

#: Health state -> theme text token for its dot.
STATE_TOKEN: dict[str, str] = {
    "building": "text-success",
    "mixed": "text-warning",
    "weakening": "text-warning",
    "challenged": "text-error",
    "emerging": "text-muted",
    "idle": "text-muted",
}

#: How far ahead the agenda looks for earnings.
EARNINGS_WINDOW = timedelta(days=7)


def _setup_link(check: services.Check) -> tuple[str, str]:
    """The key and app action that resolves a setup-check step."""
    name = check.name
    if name.startswith("LLM provider"):
        return "p", "show_provider_picker"
    if name == "Price history":
        return "2", "switch_screen('targets')"
    return "c", "switch_screen('config')"


def _plural(count: int, noun: str) -> str:
    return f"{count} {noun}" if count == 1 else f"{count} {noun}s"


def _theses(count: int) -> str:
    return "1 thesis" if count == 1 else f"{count} theses"


def _when(ts: datetime, now: datetime) -> str:
    """Relative distance: what you react to, beside the date you diarise."""
    days = (ts.date() - now.date()).days
    if days <= 0:
        return "today"
    if days == 1:
        return "tomorrow"
    if days < 14:
        return f"in {days}d"
    return f"in {days // 7}w"


def _price(value: float) -> str:
    return f"{value:,.2f}" if value >= 10 else f"{value:,.4f}"


def _watch_head(*, live: bool) -> str:
    """The watchlist column header, which names the price column it describes.

    ``last`` plus a live dot once a quote has arrived; ``close`` and no dot
    while the rows are still showing the newest stored bar.
    """
    price = "last" if live else "close"
    head = f"{' Symbol':<11}{price:>11}{'chg%':>9}{'age':>6}  {SPARK_CLOSES} closes"
    return escape(head) + ("  [$text-success]● live[/]" if live else "")


def _symbol(watched: list[tuple[str, Instrument]], instrument_id: str) -> str:
    for _target, instrument in watched:
        if instrument.id == instrument_id:
            return str(instrument.symbol)
    return instrument_id


class FocusBox(Vertical):
    """A pane body that takes focus so tab can land on it (no keys of its own)."""

    can_focus = True


class WatchRow(Horizontal):
    """One instrument: symbol, price, change, quote age, spark.

    The price cell is the live quote when one has arrived and the last stored
    close otherwise — the age cell says which, so a price is never passed off
    as fresher than it is. Cells are updated in place by ``paint`` rather than
    remounted, because the quote feed repaints twice a second.
    """

    def __init__(self, index: int, target_id: str, instrument: Instrument, closes: list[float]):
        super().__init__(classes="w-row")
        self.index = index
        self.target_id = target_id
        self.instrument = instrument
        self.closes = closes
        self.quote: Quote | None = None

    def compose(self) -> ComposeResult:
        yield Static(self.instrument.symbol, classes="w-sym", markup=False)
        yield Static("", classes="w-close", markup=False)
        yield Static("", classes="w-chg", markup=False)
        yield Static("", classes="w-age", markup=False)
        if self.closes:
            yield BrailleGraph(self.closes, fill=True, classes="w-spark")
        else:
            yield Static("no bars", classes="w-none", markup=False)

    def on_mount(self) -> None:
        self.paint()

    def set_quote(self, quote: Quote | None) -> None:
        """Adopt the newest quote (or its absence) and repaint the three cells."""
        self.quote = quote
        self.paint()

    def paint(self) -> None:
        price, pct, age, state = self._cells()
        with suppress(Exception):
            self.query_one(".w-close", Static).update("—" if price is None else _price(price))
            chg = self.query_one(".w-chg", Static)
            if price is None or pct is None:
                glyph, value = "", "—"
            elif pct > 0:
                glyph, value = "▲ ", f"{pct:+.2f}%"
            elif pct < 0:
                glyph, value = "▼ ", f"{pct:+.2f}%"
            else:
                glyph, value = "", f"{pct:+.2f}%"
            chg.update(f"{glyph}{value}")
            chg.set_class(bool(pct and pct > 0), "-up")
            chg.set_class(bool(pct and pct < 0), "-down")
            cell = self.query_one(".w-age", Static)
            cell.update(age)
            cell.set_class(state == "warn" or state == "error", "-stale")

    def _cells(self) -> tuple[float | None, float | None, str, str]:
        """``(price, change %, age label, age state)`` for whatever evidence exists."""
        if self.quote is not None:
            pct = self.quote.change_pct
            if pct is None and self.closes and self.closes[-1]:
                pct = (self.quote.price - self.closes[-1]) / self.closes[-1] * 100
            age, state = age_text(datetime.now(UTC) - self.quote.received_at)
            return self.quote.price, pct, age, state
        if self.closes:
            last = self.closes[-1]
            prior = self.closes[-2] if len(self.closes) > 1 else None
            return last, ((last - prior) / prior * 100) if prior else None, "", "ok"
        return None, None, "", "ok"

    def on_click(self) -> None:
        rows = self.parent
        if isinstance(rows, WatchRows):
            rows.select(self.index)
            rows.focus()


class WatchRows(Vertical):
    """The watchlist body: a cursor over ``WatchRow``s, arrows and enter."""

    can_focus = True

    BINDINGS = [
        Binding("up", "cursor(-1)", "Up", show=False),
        Binding("down", "cursor(1)", "Down", show=False),
        Binding("enter", "open", "Open", show=False),
    ]

    class Open(Message):
        """Enter on a row: open that target on the Watchlist screen."""

        def __init__(self, target_id: str, instrument: Instrument) -> None:
            super().__init__()
            self.target_id = target_id
            self.instrument = instrument

    def __init__(self, id: str | None = None) -> None:
        super().__init__(id=id)
        self.cursor = 0

    @property
    def rows(self) -> list[WatchRow]:
        return list(self.query(WatchRow))

    async def set_rows(self, rows: list[WatchRow]) -> None:
        await self.remove_children()
        if rows:
            await self.mount(*rows)
        self.cursor = min(self.cursor, max(len(rows) - 1, 0))
        self._paint()

    def select(self, index: int) -> None:
        rows = self.rows
        if rows:
            self.cursor = max(0, min(index, len(rows) - 1))
        self._paint()

    def action_cursor(self, delta: int) -> None:
        self.select(self.cursor + delta)

    def action_open(self) -> None:
        rows = self.rows
        if rows:
            row = rows[self.cursor]
            self.post_message(self.Open(row.target_id, row.instrument))

    def _paint(self) -> None:
        for row in self.rows:
            row.set_class(row.index == self.cursor, "-cursor")


class HomeLink(Horizontal):
    """One ``▸ key label`` row in the agenda or the setup checklist.

    Clickable: runs the app action, so an agenda line or a setup step is a
    jump, not a paragraph. The label is updated in place by the screen so
    the rows never remount and keep their ids across refreshes.
    """

    def __init__(self, key: str, action: str, id: str | None = None) -> None:
        super().__init__(id=id, classes="home-link")
        self.key = key
        self.action = action

    def compose(self) -> ComposeResult:
        yield Static("▸", classes="home-link-glyph", markup=False)
        yield Static(self.key, classes="home-link-key", markup=False)
        yield Static("", classes="home-link-label")

    def set_label(self, text: str) -> None:
        self.query_one(".home-link-label", Static).update(text)

    async def on_click(self) -> None:
        await self.app.run_action(self.action)


class Home(QuoteFeedMixin, DeltaScreen):
    name = "home"

    DEFAULT_CSS = """
    Home #home-header { height: 1; margin: 0 0 0 0; }
    Home #home-brand {
        width: auto;
        background: $primary;
        color: $block-cursor-foreground;
        text-style: bold;
    }
    Home #home-sub { width: auto; margin-left: 2; color: $text-muted; }
    Home #home-clock { width: 1fr; text-align: right; color: $text-muted; }
    Home #home-grid { height: 1fr; }

    /* Wide and tall: the top two rows share what a taller terminal adds. The
       agenda row is fixed at four lines plus its border. Folded (short or
       narrow): the middle row is gone. */
    Home #home-top { height: 1fr; }
    Home #home-mid { height: 1fr; }
    Home #home-bottom { height: 7; }

    /* watchlist */
    Home #watch-head { height: 1; color: $text-muted; }
    Home #watch-rows { height: auto; }
    Home .w-row { height: 1; }
    Home .w-sym { width: 11; padding-left: 1; color: $foreground; text-style: bold; }
    Home .w-close { width: 11; text-align: right; color: $foreground; }
    Home .w-chg { width: 9; text-align: right; color: $text-muted; }
    Home .w-chg.-up { color: $text-success; }
    Home .w-chg.-down { color: $text-error; }
    Home .w-age { width: 6; text-align: right; color: $text-muted; }
    Home .w-age.-stale { color: $text-warning; }
    Home .w-none { width: 1fr; margin-left: 2; color: $text-muted; }
    Home .w-spark { width: 1fr; height: 1; margin: 0 1 0 2; }
    Home .w-spark > .braille-graph--low-color { color: $text-primary 45%; }
    Home .w-spark > .braille-graph--high-color { color: $text-primary; }
    Home .w-row.-cursor { background: $block-cursor-blurred-background; }
    Home WatchRows:focus .w-row.-cursor { background: $primary; }
    Home .w-row.-cursor Static { color: $block-cursor-foreground; }
    Home .w-row.-cursor .w-spark > .braille-graph--low-color { color: $block-cursor-foreground 60%; }
    Home .w-row.-cursor .w-spark > .braille-graph--high-color { color: $block-cursor-foreground; }
    Home #watch-note, Home #watch-since, Home #watch-next {
        height: 1;
        padding-left: 1;
        color: $text-muted;
        text-wrap: nowrap;
        text-overflow: ellipsis;
    }
    Home #watch-note, Home #watch-since { margin-top: 1; }

    /* since you last looked */
    Home #since-body { height: 1fr; padding: 0 1; }
    Home .since-line { height: 1; }
    Home .since-left { width: 1fr; text-wrap: nowrap; text-overflow: ellipsis; }
    Home .since-right { width: auto; margin-left: 2; color: $text-muted; }
    Home #since-head { margin-bottom: 1; }
    Home #since-spark { width: 1fr; height: 2; }
    Home #since-spark > .braille-graph--low-color { color: $text-primary 45%; }
    Home #since-spark > .braille-graph--high-color { color: $text-primary; }
    Home #since-axis { margin-bottom: 1; color: $text-muted; }
    Home #since-newest { margin-bottom: 1; }
    Home #since-stale { height: 1; text-wrap: nowrap; text-overflow: ellipsis; }
    Home #since-review { height: 1; color: $text-warning; text-wrap: nowrap; text-overflow: ellipsis; }

    /* upcoming / theses */
    Home #upcoming-table, Home #theses-table { height: 1fr; overflow-x: hidden; }
    Home #upcoming-empty, Home #theses-empty {
        height: 1;
        padding: 0 1;
        color: $text-muted;
        text-wrap: nowrap;
        text-overflow: ellipsis;
    }
    Home #upcoming-note { height: 1; padding: 0 1; color: $text-muted; }
    Home #theses-summary { height: 1; padding: 0 1; margin-bottom: 1; text-wrap: nowrap; text-overflow: ellipsis; }

    /* agenda */
    Home #agenda-rows { height: auto; padding: 0 1; }
    Home .home-link { height: 1; }
    Home .home-link-glyph { width: 2; color: $text-primary; }
    Home .home-link-key { width: 3; color: $text-primary; text-style: bold; }
    Home .home-link-label { width: 1fr; color: $text-muted; }
    Home .home-link:hover .home-link-label { color: $foreground; }

    /* first-run setup checklist */
    Home #home-setup { height: 1fr; padding: 1 1; }
    Home #home-setup.-hidden { display: none; }
    Home #setup-title { height: 1; color: $foreground; text-style: bold; }
    Home #setup-sub { height: 1; margin-bottom: 1; color: $text-muted; }
    Home #setup-rows { height: auto; }
    """

    def __init__(self, delta: Any, last_seen: datetime | None = None) -> None:
        super().__init__(delta)
        # Captured once for the session. Mounted standalone (no app), fall back
        # to the stored value so the panel still has a reference point.
        self.last_seen = last_seen or read_last_seen(delta.cfg)
        # Cached between refreshes so a resize can re-lay the tables.
        self._watched_rows: list[tuple[str, Instrument]] = []
        self._events: list[services.Upcoming] | None = None
        self._fleet: list[services.ThesisHealth] = []
        # True while the no-targets setup checklist replaces the grid.
        self._first_run = False
        # Ephemeral quotes (QuoteFeedMixin), started on resume and cancelled
        # on suspend/unmount.
        self.active = False

    # ----- layout ---------------------------------------------------------

    def compose_content(self) -> ComposeResult:
        with Horizontal(id="home-header"):
            yield Static(" DELTA ", id="home-brand", markup=False)
            yield Static("overview", id="home-sub", markup=False)
            yield Static("", id="home-clock", markup=False)
        with Vertical(id="home-grid"):
            with PaneRow(id="home-top"):
                with Pane(
                    title="watchlist",
                    hints=hint_markup(("↑↓", "select"), ("enter", "open"), ("tab", "next box")),
                    id="watch-pane",
                ):
                    yield Static(_watch_head(live=False), id="watch-head")
                    yield WatchRows(id="watch-rows")
                    yield Static("", id="watch-note", markup=False)
                    yield Static("", id="watch-since")
                    yield Static("", id="watch-next")
                with Pane(
                    title="since you last looked",
                    hints=hint_markup(("3", "research")),
                    id="since-pane",
                ):
                    with FocusBox(id="since-body"):
                        with Horizontal(id="since-head", classes="since-line"):
                            yield Static("", classes="since-left")
                            yield Static("", classes="since-right", markup=False)
                        yield BrailleGraph([], fill=True, id="since-spark")
                        with Horizontal(id="since-axis", classes="since-line"):
                            yield Static(
                                f"{PULSE_DAYS} days ago", classes="since-left", markup=False
                            )
                            yield Static("today", classes="since-right", markup=False)
                        yield Static("", id="since-rank", classes="since-line")
                        with Horizontal(id="since-newest", classes="since-line"):
                            yield Static("", classes="since-left")
                            yield Static("", classes="since-right", markup=False)
                        yield Static("", id="since-stale")
                        yield Static("", id="since-review", markup=False)
            with PaneRow(id="home-mid"):
                with Pane(
                    title="upcoming",
                    hints=hint_markup(("enter", "open evidence")),
                    id="upcoming-pane",
                ):
                    yield DeltaTable(id="upcoming-table")
                    yield Static("", id="upcoming-empty", markup=False)
                    yield Static(
                        "calendar plugin: earnings & dividends only",
                        id="upcoming-note",
                        markup=False,
                    )
                with Pane(
                    title="theses",
                    hints=hint_markup(("enter", "open thesis"), ("4", "all")),
                    id="theses-pane",
                ):
                    yield Static("", id="theses-summary")
                    yield DeltaTable(id="theses-table")
                    yield Static("", id="theses-empty", markup=False)
            with PaneRow(id="home-bottom"):
                with Pane(
                    title="needs you today",
                    hints=hint_markup(("enter", "open"), ("tab", "next box")),
                    id="agenda-pane",
                ):
                    with Vertical(id="agenda-rows"):
                        yield HomeLink("6", "switch_screen('decisions')", id="agenda-reviews")
                        yield HomeLink("4", "switch_screen('theses')", id="agenda-falsifier")
                        yield HomeLink("3", "switch_screen('data')", id="agenda-earnings")
                        yield HomeLink("2", "switch_screen('targets')", id="agenda-stale")
            with Vertical(id="home-setup", classes="-hidden"):
                yield Static("setup", id="setup-title", markup=False)
                yield Static("", id="setup-sub", markup=False)
                with Vertical(id="setup-rows"):
                    for index in range(5):
                        yield HomeLink("", "", id=f"setup-{index}")

    async def on_mount(self) -> None:
        self.layout_views()
        await self.refresh_view()
        self.set_interval(1, self._tick)
        self.set_interval(QUOTE_PAINT_SECONDS, self._paint_quotes)

    def on_resize(self) -> None:
        if self.is_mounted:
            self.layout_views()

    def layout_views(self) -> None:
        """Wide+tall: watchlist + pulse. Short: no middle row. Narrow: watchlist and agenda."""
        narrow = self.apply_breakpoint()
        short = self.size.height < TALL_HEIGHT
        folded = narrow or short
        self.set_class(short, "-short")
        self.set_class(folded, "-folded")
        if not self._first_run:
            self.query_one("#home-mid").display = not folded
            self.query_one("#since-pane").display = not narrow
            self.query_one("#watch-note").display = not folded
            self.query_one("#watch-since").display = folded
            self.query_one("#watch-next").display = folded
        # Table columns are sized to the box, so a resize re-lays them.
        if self._events is not None and not folded and not self._first_run:
            self._refresh_upcoming(self._watched_rows, self._events)
            self._refresh_theses(self._fleet)

    # ----- data -----------------------------------------------------------

    def _tick(self) -> None:
        now = datetime.now().astimezone()
        # The clock is cosmetic: a tick landing while the screen is mid-rebuild
        # must never raise, or the app stores the exception and dies later.
        clock = self.query("#home-clock")
        if clock:
            clock.first().update(
                f"{now:%A %d %B %Y} · {now:%H:%M:%S} {now:%Z}".rstrip()
            )

    def _token(self, name: str) -> str:
        """A theme colour Rich can parse; empty (default colour) when unknown.

        Never the raw ``theme_variables`` value: Textual writes ``auto 87%``
        for background-dependent tokens and Rich raises ``MissingStyle`` on it.
        """
        return token_color(self.app, name)

    # ----- quotes ---------------------------------------------------------

    def _sync_feed(self) -> None:
        """Point the quote feed at the watched instruments, restarting if they changed."""
        self.sync_quotes([instrument for _target, instrument in self._watched_rows[:WATCH_ROWS]])

    def _paint_quotes(self) -> None:
        """Repaint the watchlist from the feed. Reads cached quotes only — never the network."""
        if not self.is_mounted:
            return
        try:
            rows = self.query_one("#watch-rows", WatchRows).rows
            head = self.query_one("#watch-head", Static)
        except Exception:
            return
        live = False
        for row in rows:
            quote = self.quote_for(row.instrument.id)
            row.set_quote(quote)
            live = live or quote is not None
        head.update(_watch_head(live=live))

    async def _stop_feed(self) -> None:
        self.active = False
        await self.stop_quotes()

    async def on_screen_resume(self) -> None:
        self.active = True
        await super().on_screen_resume()
        self._sync_feed()

    async def on_screen_suspend(self) -> None:
        await self._stop_feed()

    async def on_unmount(self) -> None:
        await self._stop_feed()

    def _watched(self) -> list[tuple[str, Instrument]]:
        """Every ticker of every watch target, keyed by the target it belongs to.

        Prefers the universe instrument (it carries the bars); a ticker the
        universe does not know is synthesised so the row still shows, as
        "no bars" — the honest reading, and the same rule the Watchlist uses.
        """
        known = {instrument.id: instrument for instrument in self.delta.universe()}
        by_symbol = {instrument.symbol.upper(): instrument for instrument in known.values()}
        watched: list[tuple[str, Instrument]] = []
        seen: set[str] = set()
        for target in sorted(services.target_specs().values(), key=lambda t: t.id):
            for market in target.markets:
                for symbol in target.tickers:
                    ident = f"{market.upper()}:{symbol}"
                    instrument = known.get(ident) or by_symbol.get(symbol.upper())
                    if instrument is None:
                        instrument = Instrument(
                            id=ident,
                            market=market,
                            symbol=symbol,
                            currency={"us": "USD", "asx": "AUD"}.get(market, ""),
                            asset_class=getattr(target, "asset_class", "equity"),
                        )
                    if instrument.id in seen:
                        continue
                    seen.add(instrument.id)
                    watched.append((target.id, instrument))
        return watched

    async def refresh_view(self) -> None:
        """Reload the desk, running the blocking reads off the event loop.

        The SQL work runs in a ``@work(thread=True)`` worker; ``wait()`` keeps
        this awaitable so ``on_mount`` / ``on_screen_resume`` and the tests keep
        their synchronous refresh semantics.
        """
        self._tick()
        self._set_shimmer(True)
        await self._reload().wait()

    @work(exclusive=True, group="home-refresh", thread=True)
    async def _reload(self) -> None:
        data = self._gather()  # blocking reads, in the worker thread
        self.app.call_from_thread(self._apply, data)  # blocks until _apply finishes

    def _gather(self) -> dict[str, Any]:
        """All blocking reads (SQL and the report dir) in one place, off the loop."""
        engine = self.delta.engine
        watched = self._watched()
        ids = [instrument.id for _target, instrument in watched]
        health = services.data_health(self.delta)
        pulse = services.pulse(engine, instrument_ids=ids, since=self.last_seen, days=PULSE_DAYS)
        events = services.upcoming_events(engine, instrument_ids=ids, limit=UPCOMING_ROWS)
        earnings = self._earnings_soon(ids)
        try:
            fleet = services.thesis_fleet(engine)
        except Exception:
            fleet = []
        checks = services.setup_checks(self.delta)
        closes = {
            instrument.id: services.recent_closes(engine, instrument.id, limit=SPARK_CLOSES)
            for _target, instrument in watched
        }
        stale = self._stale(watched, closes, health)
        try:
            prompts = review.review_queue(
                self.delta, instrument_ids=ids, since=self.last_seen
            )
            due = decisions.due_reviews(engine)
        except Exception:
            prompts, due = [], []
        return {
            "watched": watched,
            "health": health,
            "pulse": pulse,
            "events": events,
            "earnings": earnings,
            "fleet": fleet,
            "checks": checks,
            "closes": closes,
            "stale": stale,
            "prompts": prompts,
            "due": due,
        }

    def _earnings_soon(self, ids: list[str]) -> list[services.Upcoming]:
        """Earnings events in the next ``EARNINGS_WINDOW``, reusing the calendar read."""
        now = datetime.now(UTC)
        horizon = now + EARNINGS_WINDOW
        return [
            event
            for event in services.upcoming_events(self.delta.engine, instrument_ids=ids, limit=30)
            if event.kind == "earnings" and event.ts <= horizon
        ]

    async def _apply(self, data: dict[str, Any]) -> None:
        watched = data["watched"]
        self._watched_rows = watched
        self._events = data["events"]
        self._fleet = data["fleet"]
        self._set_shimmer(False)

        if not watched:
            self._show_setup(data["checks"])
            return
        self._show_setup([])

        await self._refresh_watchlist(watched, data["closes"])
        self._refresh_since(watched, data["pulse"], data["stale"])
        self._refresh_review(data["due"], data["prompts"])
        self._refresh_upcoming(watched, data["events"])
        self._refresh_theses(data["fleet"])
        self._refresh_agenda(data)
        self._refresh_summary(watched, data["pulse"], data["events"], data["fleet"], data["stale"])

    def _set_shimmer(self, on: bool) -> None:
        """A lightweight loading signal while the threaded reload runs.

        A shared shimmer widget does not exist yet (see the phase-0 backlog):
        the watchlist badge stands in until one lands in ``components.py``.
        """
        if not self.is_mounted:
            return
        with suppress(Exception):
            self.query_one("#watch-pane", Pane).set_badge("…" if on else "")

    def _stale(
        self,
        watched: list[tuple[str, Instrument]],
        closes: dict[str, list[float]],
        health: Any,
    ) -> list[str]:
        """Warnings worth a ⚠: watched instruments with old or missing bars, plugins off.

        Bar age is known only for universe instruments (``data_health`` walks
        the universe); a ticker outside it with bars is left alone rather than
        called stale on no evidence. A company with no report at all is not a
        warning — nothing is out of date until something has been written.
        """
        now = datetime.now(UTC)
        reports_dir = str(getattr(self.delta.cfg, "reports_dir", "reports") or "reports")
        notes: list[str] = []
        for _target, instrument in watched:
            newest = health.latest_bar.get(instrument.id)
            if newest is None:
                if not closes.get(instrument.id):
                    notes.append(f"{instrument.symbol} no bars")
            else:
                age = now - to_utc(newest)
                if age > timedelta(days=1):
                    notes.append(f"{instrument.symbol} {age_text(age)[0]} old")
            as_of = services.latest_report_age(reports_dir, instrument.id)
            if as_of is not None:
                report_age = now - to_utc(as_of)
                if report_age > REPORT_STALE:
                    notes.append(f"{instrument.symbol} report {age_text(report_age)[0]} old")
        plugins = getattr(self.delta, "plugins", {}) or {}
        notes.extend(
            f"{name} off"
            for name, plugin in plugins.items()
            if not getattr(plugin, "enabled", False)
        )
        return notes

    async def _refresh_watchlist(
        self, watched: list[tuple[str, Instrument]], closes: dict[str, list[float]]
    ) -> None:
        rows = [
            WatchRow(index, target_id, instrument, closes.get(instrument.id, []))
            for index, (target_id, instrument) in enumerate(watched[:WATCH_ROWS])
        ]
        await self.query_one("#watch-rows", WatchRows).set_rows(rows)
        if self.active:
            self._sync_feed()
        self._paint_quotes()
        self.query_one("#watch-pane", Pane).set_badge(str(len(watched)) if watched else "")
        hidden = len(watched) - WATCH_ROWS
        note = self.query_one("#watch-note", Static)
        if not watched:
            note.update("nothing yet — 2 builds the watchlist")
        elif hidden > 0:
            note.update(f"+{hidden} more · 2 watchlist")
        else:
            note.update(f"spark: {SPARK_CLOSES} daily closes · chg%: move on the day")

    def _refresh_since(
        self, watched: list[tuple[str, Instrument]], pulse: services.Pulse, stale: list[str]
    ) -> None:
        counts = [
            f"[b]{_plural(n, noun)}[/b]"
            for n, noun in (
                (pulse.articles, "article"),
                (pulse.filings, "filing"),
                (pulse.events, "event"),
            )
            if n
        ]
        head = self.query_one("#since-head")
        head.query_one(".since-left", Static).update(
            "   ".join(counts) if counts else "[$text-muted]nothing new since your last visit[/]"
        )
        head.query_one(".since-right", Static).update(
            f"since {self.last_seen.astimezone():%a %H:%M}"
        )
        self.query_one("#since-pane", Pane).set_badge(f"{pulse.total} new" if pulse.total else "")

        spark = self.query_one("#since-spark", BrailleGraph)
        spark.data = pulse.daily
        spark.display = any(pulse.daily)
        self.query_one("#since-axis").display = any(pulse.daily)

        if pulse.busiest:
            quiet = (
                f"   [$text-muted]quietest[/]  {escape(_symbol(watched, pulse.quietest[0]))}"
                f" [$text-muted]({pulse.quietest[1]})[/]"
                if pulse.quietest
                else ""
            )
            rank = (
                f"[$text-muted]busiest[/]  {escape(_symbol(watched, pulse.busiest[0]))}"
                f" [$text-muted]({pulse.busiest[1]})[/]{quiet}"
            )
        else:
            rank = f"[$text-muted]no activity in the last {PULSE_DAYS} days[/]"
        self.query_one("#since-rank", Static).update(rank)

        newest = self.query_one("#since-newest")
        ids = [instrument.id for _target, instrument in watched]
        headline = services.latest_headline(self.delta.engine, instrument_ids=ids)
        if headline:
            newest.query_one(".since-left", Static).update(
                f"[$text-muted]newest[/]   {escape(headline.title)}"
            )
            newest.query_one(".since-right", Static).update(
                age_text(datetime.now(UTC) - headline.ts)[0]
            )
        else:
            newest.query_one(".since-left", Static).update(
                "[$text-muted]newest   no articles yet[/]"
            )
            newest.query_one(".since-right", Static).update("")

        self.query_one("#since-stale", Static).update(
            f"[$text-warning]⚠ {escape(' · '.join(stale))}[/]"
            if stale
            else "[$text-muted]✓ bars and reports fresh · all plugins on[/]"
        )

    def _refresh_review(self, due: list[Any], prompts: list[Any]) -> None:
        """Expose evidence and journal prompts without making an investment call."""
        line = self.query_one("#since-review", Static)
        if due:
            line.update(f"⚠ {len(due)} decision review{'s' if len(due) != 1 else ''} due · 6 decisions")
        elif prompts:
            line.update(f"⚠ {len(prompts)} evidence prompt{'s' if len(prompts) != 1 else ''} · 3 evidence")
        else:
            line.update("✓ no decision or evidence reviews due")

    def _refresh_upcoming(
        self, watched: list[tuple[str, Instrument]], events: list[services.Upcoming]
    ) -> None:
        table = self.query_one("#upcoming-table", DataTable)
        table.clear(columns=True)
        widths = (7, 11, 6, 7)
        detail = self._table_width("#upcoming-pane") - sum(widths) - 2 * (len(widths) + 1)
        for label, width in zip(("Symbol", "Event", "Date", "When"), widths, strict=True):
            table.add_column(label, width=width)
        table.add_column("Detail", width=max(detail, 4))
        now = datetime.now(UTC)
        muted = self._token("text-muted")
        accent = self._token("text-primary")
        for item in events:
            table.add_row(
                Text(_symbol(watched, item.instrument_id), style="bold", no_wrap=True),
                Text(item.kind, style=muted, no_wrap=True, overflow="ellipsis"),
                f"{item.ts:%d %b}",
                Text(_when(item.ts, now), style=accent),
                Text(item.summary, style=muted, no_wrap=True, overflow="ellipsis"),
            )
        table.display = bool(events)
        empty = self.query_one("#upcoming-empty", Static)
        empty.display = not events
        empty.update("nothing scheduled — press 3, then U to gather evidence")
        self.query_one("#upcoming-pane", Pane).set_badge(str(len(events)) if events else "")

    def _refresh_theses(self, fleet: list[services.ThesisHealth]) -> None:
        summary = self.query_one("#theses-summary", Static)
        table = self.query_one("#theses-table", DataTable)
        empty = self.query_one("#theses-empty", Static)
        table.clear(columns=True)
        ratio_w = len("for/against")
        table.add_column("", width=1)
        table.add_column(
            "Most at risk", width=max(self._table_width("#theses-pane") - ratio_w - 7, 8)
        )
        table.add_column("for/against", width=ratio_w)
        if not fleet:
            summary.update("[$text-muted]no theses yet[/]")
            table.display = False
            empty.display = True
            empty.update("4 opens the theses desk — n tracks a claim")
            self.query_one("#theses-pane", Pane).set_badge("")
            return
        counts: dict[str, int] = {}
        for entry in fleet:
            counts[entry.state] = counts.get(entry.state, 0) + 1
        summary.update(
            "   ".join(
                f"[${STATE_TOKEN.get(state, 'text-muted')}]● {n} {state}[/]"
                for state, n in sorted(
                    counts.items(), key=lambda kv: services.RISK_ORDER.get(kv[0], 99)
                )
            )
        )
        for entry in fleet[:THESES_ROWS]:
            result = entry.result
            ratio = f"{result.support} / {result.against}" if result else "—"
            table.add_row(
                Text("●", style=self._token(STATE_TOKEN.get(entry.state, "text-muted"))),
                Text(entry.thesis.claim, no_wrap=True, overflow="ellipsis"),
                Text(ratio, style=self._token("text-muted"), justify="right"),
            )
        table.display = True
        empty.display = False
        challenged = counts.get("challenged", 0)
        badge = str(len(fleet)) + (f" · {challenged} challenged" if challenged else "")
        self.query_one("#theses-pane", Pane).set_badge(badge)

    def _table_width(self, pane_id: str) -> int:
        """Columns a table inside ``pane_id`` may use: the box interior, or a guess before layout."""
        width = self.query_one(pane_id).content_size.width
        if width <= 0:
            width = max(self.size.width - 2, NARROW_WIDTH) // 2 - 2
        return width

    def _refresh_agenda(self, data: dict[str, Any]) -> None:
        """The "needs you today" agenda: four lines, each a jump key."""
        due = data["due"]
        fleet = data["fleet"]
        earnings = data["earnings"]
        stale = data["stale"]

        challenged = [entry for entry in fleet if entry.state == "challenged"]

        def set_row(link_id: str, text: str) -> None:
            self.query_one(f"#{link_id}", HomeLink).set_label(text)

        set_row(
            "agenda-reviews",
            f"[b]{_plural(len(due), 'decision review')} due[/b]"
            if due
            else "[$text-muted]✓ no decision reviews due[/]",
        )
        set_row(
            "agenda-falsifier",
            f"[b]{_plural(len(challenged), 'falsifier hit')}[/b]"
            if challenged
            else "[$text-muted]✓ no falsifier hits[/]",
        )
        set_row(
            "agenda-earnings",
            f"[b]{_plural(len(earnings), 'earnings')}[/b] in the next 7 days"
            if earnings
            else "[$text-muted]✓ no earnings in the next 7 days[/]",
        )
        set_row(
            "agenda-stale",
            f"[$text-warning]⚠ {_plural(len(stale), 'stale source')}[/]"
            if stale
            else "[$text-muted]✓ bars and reports fresh[/]",
        )
        needs = len(due) + len(challenged) + len(earnings) + len(stale)
        self.query_one("#agenda-pane", Pane).set_badge(str(needs) if needs else "clear")

    def _show_setup(self, checks: list[services.Check]) -> None:
        """First run: no targets, so the whole grid becomes the setup checklist."""
        first_run = bool(checks)
        self._first_run = first_run
        setup = self.query_one("#home-setup")
        setup.set_class(not first_run, "-hidden")
        for row_id in ("home-top", "home-mid", "home-bottom"):
            self.query_one(f"#{row_id}").display = not first_run
        if not first_run:
            return
        self.query_one("#setup-title", Static).update("setup")
        self.query_one("#setup-sub", Static).update(
            "a few things before Delta can gather anything — each jumps to its fix"
        )
        for index, check in enumerate(checks[:5]):
            key, action = _setup_link(check)
            link = self.query_one(f"#setup-{index}", HomeLink)
            link.key = key
            link.action = action
            link.query_one(".home-link-key", Static).update(key)
            state = "[$text-success]✓[/]" if check.ok else "[$text-error]✗[/]"
            link.set_label(f"{escape(check.name)} [$text-muted]— {escape(check.fix)}[/]  {state}")

    def _refresh_summary(
        self,
        watched: list[tuple[str, Instrument]],
        pulse: services.Pulse,
        events: list[services.Upcoming],
        fleet: list[services.ThesisHealth],
        stale: list[str],
    ) -> None:
        """The two lines that stand in for the middle row when it is folded away."""
        counts = (
            " ".join(
                f"[b]{_plural(n, noun)}[/b]"
                for n, noun in (
                    (pulse.articles, "article"),
                    (pulse.filings, "filing"),
                    (pulse.events, "event"),
                )
                if n
            )
            or "nothing new"
        )
        since = f"[$text-muted]since {self.last_seen.astimezone():%a %H:%M}[/]  {counts}"
        if stale:
            since += f"   [$text-warning]⚠ {escape(stale[0])}[/]"
            if len(stale) > 1:
                since += f"[$text-muted] +{len(stale) - 1}[/]"
        self.query_one("#watch-since", Static).update(since)

        upcoming = (
            " · ".join(
                f"{escape(_symbol(watched, item.instrument_id))} {escape(item.kind)} {item.ts:%d %b}"
                for item in events[:2]
            )
            or "nothing scheduled"
        )
        line = f"[$text-muted]next[/]  {upcoming}"
        challenged = sum(1 for entry in fleet if entry.state == "challenged")
        weakening = sum(1 for entry in fleet if entry.state == "weakening")
        if challenged:
            line += f"   [$text-error]● {_theses(challenged)} challenged[/]"
        elif weakening:
            line += f"   [$text-warning]● {_theses(weakening)} weakening[/]"
        elif fleet:
            line += f"   [$text-muted]● {_theses(len(fleet))} tracked[/]"
        self.query_one("#watch-next", Static).update(line)

    # ----- keys -----------------------------------------------------------

    def on_watch_rows_open(self, event: WatchRows.Open) -> None:
        """Enter on the watchlist: open that target on the Watchlist screen."""
        if not goto(self.app, "targets"):
            return
        self.app.call_later(self._highlight_target, f"target:{event.target_id}")

    def _highlight_target(self, option_id: str, attempts: int = 5) -> None:
        """Move the Watchlist cursor to the row once that screen has built it."""
        targets = getattr(self.app, "screens_by_name", {}).get("targets")
        if targets is None:
            return
        try:
            table = targets.query_one("#target-table", OptionList)
            table.highlighted = table.get_option_index(option_id)
        except Exception:
            if attempts > 0:
                self.app.set_timer(0.05, lambda: self._highlight_target(option_id, attempts - 1))

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        """Enter on the upcoming or theses table: open the matching desk."""
        screen = {"upcoming-table": "data", "theses-table": "theses"}.get(event.data_table.id or "")
        if screen:
            goto(self.app, screen)
