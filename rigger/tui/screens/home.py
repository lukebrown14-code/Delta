"""Home: a full-bleed splash dashboard — wordmark, data health, ticker strip, menu.

Deliberately *not* a ``RiggerScreen``: this screen has no nav rail, no top bar
and no footer. It is the intro page, so the menu on the right is the
navigation, and every row shows the key that triggers it.
"""

from __future__ import annotations

from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal, Vertical
from textual.screen import Screen
from textual.widgets import Sparkline, Static

from rigger import services
from rigger.core.state import read_last_seen
from rigger.core.time import to_utc
from rigger.tui.shell import age_text

LOGO = r"""
██████╗ ██╗ ██████╗  ██████╗ ███████╗██████╗
██╔══██╗██║██╔════╝ ██╔════╝ ██╔════╝██╔══██╗
██████╔╝██║██║  ███╗██║  ███╗█████╗  ██████╔╝
██╔══██╗██║██║   ██║██║   ██║██╔══╝  ██╔══██╗
██║  ██║██║╚██████╔╝╚██████╔╝███████╗██║  ██║
╚═╝  ╚═╝╚═╝ ╚═════╝  ╚═════╝ ╚══════╝╚═╝  ╚═╝
""".strip("\n")
# Pad every row to the same width: ``text-align: center`` centres each line
# independently, so ragged rows make the wordmark lean.
LOGO = "\n".join(
    line.ljust(max(len(row) for row in LOGO.splitlines())) for line in LOGO.splitlines()
)

#: One glyph per category, not one per row: the marker says what kind of thing
#: this is. All three are verified single-cell so the columns stay aligned.
GROUP_GLYPH = {"do": "▸", "look": "◦", "app": "·"}


@dataclass(frozen=True)
class MenuItem:
    key: str
    label: str  # full phrase, what the prompt matches on
    hint: str  # short form for the hint grid
    group: str
    screen: str | None = None
    action: str | None = None


MENU: list[MenuItem] = [
    MenuItem("w", "Add a watch target", "add target", "do", screen="targets"),
    MenuItem("4", "Read a report", "report", "do", screen="reports"),
    MenuItem("6", "Ask a question", "ask", "do", screen="chat"),
    MenuItem("5", "Track a thesis", "thesis", "do", screen="theses"),
    MenuItem("2", "Evidence & spend", "evidence", "look", screen="data"),
    MenuItem("c", "Command console", "console", "look", screen="console"),
    MenuItem("3", "Settings", "settings", "look", screen="config"),
    MenuItem("?", "Help", "help", "app", action="show_help"),
    MenuItem("q", "Quit", "quit", "app", action="quit"),
]

HINT_COLUMNS = 3

PULSE_DAYS = 30


def _plural(count: int, noun: str) -> str:
    return f"{count} {noun}" if count == 1 else f"{count} {noun}s"


def _headline(pulse: services.Pulse) -> str:
    """Only the categories that actually have something to report."""
    parts = [
        _plural(pulse.articles, "article"),
        _plural(pulse.events, "event"),
        _plural(pulse.filings, "filing"),
    ]
    live = [
        part
        for part, count in zip(parts, (pulse.articles, pulse.events, pulse.filings), strict=True)
        if count
    ]
    return "   ".join(live) if live else "nothing new since your last visit"


def _symbol(watched: list[Any], instrument_id: str) -> str:
    """Prefer the ticker; fall back to the raw id if it is not in the universe."""
    for instrument in watched:
        if instrument.id == instrument_id:
            return str(instrument.symbol)
    return instrument_id


class HintCell(Horizontal):
    """One key hint: category glyph, key, short label. Clickable."""

    def __init__(self, item: MenuItem) -> None:
        super().__init__(classes=f"hint-cell -{item.group}")
        self.item = item
        self.tooltip = item.label

    def compose(self) -> ComposeResult:
        yield Static(GROUP_GLYPH[self.item.group], classes="hint-glyph", markup=False)
        yield Static(self.item.key, classes="hint-key", markup=False)
        yield Static(self.item.hint, classes="hint-label", markup=False)

    def on_click(self) -> None:
        run_menu_item(self.app, self.item)


def run_menu_item(app: Any, item: MenuItem) -> None:
    """Dispatch a menu item, whether it names a screen or an app action."""
    if item.screen:
        switch = getattr(app, "action_switch_screen", None)
        if callable(switch):
            switch(item.screen)
        return
    if item.action:
        handler = getattr(app, f"action_{item.action}", None)
        if callable(handler):
            handler()


class Home(Screen):
    name = "home"
    # Nothing is focused on arrival: #home-prompt is the first focusable
    # widget, and while it holds focus it swallows the single-letter nav
    # keys (1..6, w, c, ?) before they reach the app bindings.
    AUTO_FOCUS = ""  # "" disables; None would inherit the app default of "*"

    DEFAULT_CSS = """
    Home {
        align: center top;
        background: $background;
    }
    Home #dash {
        width: 118;
        max-width: 100%;
        height: auto;
        padding-top: 2;
    }
    Home #dash-body {
        height: auto;
        padding-top: 1;
    }
    /* Under ~100 cells the two columns cannot sit side by side without
       clipping the 45-cell wordmark and the menu labels, so stack them. */
    Home #dash-body.-narrow {
        layout: vertical;
    }
    Home #dash-body.-narrow #dash-left,
    Home #dash-body.-narrow #dash-right {
        width: 100%;
    }
    Home #dash-body.-narrow #dash-right {
        padding-left: 0;
        padding-top: 1;
    }
    Home #dash-left {
        width: 56;
        height: auto;
    }
    Home #dash-right {
        width: 1fr;
        height: auto;
        padding-left: 6;
    }

    /* left column */
    Home #logo {
        width: 100%;
        height: 6;
        text-align: center;
        color: $secondary;
        text-style: bold;
    }
    Home #clock {
        width: 100%;
        text-align: center;
        color: $text-muted;
        margin: 1 0;
    }
    Home #statbox {
        width: 50;
        height: auto;
        border: round $secondary;
        border-title-color: $secondary;
        border-title-align: left;
        padding: 0 1;
        margin: 0 3;
    }
    Home .pulse-headline {
        width: 100%;
        height: 1;
        color: $foreground;
        text-style: bold;
        margin-bottom: 1;
    }
    Home .pulse-spark { width: 100%; height: 3; }
    Home .pulse-spark > .sparkline--min-color { color: $secondary 45%; }
    Home .pulse-spark > .sparkline--max-color { color: $secondary; }
    Home .pulse-axis { height: 1; margin-bottom: 1; }
    Home .pulse-axis-left { width: 1fr; color: $text-muted; }
    Home .pulse-axis-right { width: auto; color: $text-muted; }
    Home .pulse-rank { height: 1; }
    Home .pulse-busy { width: 1fr; color: $text-muted; }
    Home .pulse-quiet { width: auto; color: $text-muted; }
    Home #page-footer {
        dock: bottom;
        height: auto;
        padding-bottom: 1;
    }
    Home #tickers { height: auto; }
    Home #menu-block { height: auto; margin: 1 3 0 3; }
    Home #setup-line {
        width: 100%;
        text-align: center;
        color: $text-muted;
        text-wrap: nowrap;
        text-overflow: ellipsis;
    }
    Home #setup-line.-ok { color: $success; }
    Home #setup-line.-bad { color: $error; }
    Home .tick-row { height: 1; }
    Home .tick-sym { width: 9; color: $foreground; text-style: bold; }
    Home .tick-px { width: 11; text-align: right; color: $foreground; }
    Home .tick-chg { width: 9; text-align: right; color: $text-muted; }
    Home .tick-chg.-up { color: $success; }
    Home .tick-chg.-down { color: $error; }
    Home .tick-row Sparkline { width: 1fr; height: 1; margin-left: 2; }
    Home .tick-row Sparkline > .sparkline--min-color { color: $secondary 45%; }
    Home .tick-row Sparkline > .sparkline--max-color { color: $secondary; }
    Home #boot {
        width: 100%;
        text-align: center;
        color: $text-muted;
    }

    /* right column */
    Home .hint-row { height: 1; }
    Home .hint-cell { width: 1fr; height: 1; }
    Home .hint-glyph { width: 2; }
    Home .hint-key { width: 2; color: $primary; text-style: bold; }
    Home .hint-label { width: 1fr; color: $text-muted; }
    Home .hint-cell.-do .hint-glyph { color: $primary; }
    Home .hint-cell.-look .hint-glyph { color: $secondary; }
    Home .hint-cell.-app .hint-glyph { color: $text-muted; }
    Home .hint-cell:hover .hint-label { color: $foreground; }
    Home .sec-head {
        height: 1;
        margin: 1 0 0 0;
        color: $secondary;
        text-style: bold;
    }
    Home .sec-row { height: 1; }
    Home .sec-text {
        width: 1fr;
        color: $text-muted;
        text-wrap: nowrap;
        text-overflow: ellipsis;
    }
    Home .sec-num { width: 4; text-align: right; color: $primary; }
    """

    def __init__(self, rig: Any, last_seen: datetime | None = None) -> None:
        super().__init__()
        self.rig = rig
        # Captured once for the session. Mounted standalone (no app), fall back
        # to the stored value so the panel still has a reference point.
        self.last_seen = last_seen or read_last_seen(rig.cfg)

    def compose(self) -> ComposeResult:
        with Vertical(id="dash"):
            yield Static(LOGO, id="logo", markup=False)
            yield Static("", id="clock", markup=False)
            with Horizontal(id="dash-body"):
                with Vertical(id="dash-left"):
                    yield Vertical(id="statbox")
                    with Vertical(id="menu-block"):
                        for row in range(0, len(MENU), HINT_COLUMNS):
                            yield Horizontal(
                                *(HintCell(item) for item in MENU[row : row + HINT_COLUMNS]),
                                classes="hint-row",
                            )
                with Vertical(id="dash-right"):
                    yield Static("MARKET", classes="sec-head", markup=False)
                    yield Vertical(id="tickers")
        with Vertical(id="page-footer"):
            yield Static("", id="setup-line", markup=False)
            yield Static("", id="boot", markup=False)

    async def on_mount(self) -> None:
        self.query_one("#statbox", Vertical).border_title = "SINCE YOU LAST LOOKED"
        await self.refresh_view()
        self.set_interval(1, self._tick)

    async def on_screen_resume(self) -> None:
        await self.refresh_view()

    def on_resize(self, event: Any) -> None:
        self.query_one("#dash-body").set_class(event.size.width < 100, "-narrow")

    def _tick(self) -> None:
        self.query_one("#clock", Static).update(datetime.now().strftime("%A %d %B %Y  ·  %H:%M:%S"))

    async def refresh_view(self) -> None:
        self._tick()
        self._refresh_status()
        await self._refresh_statbox()
        await self._refresh_tickers()
        self._refresh_setup()

    def _refresh_status(self) -> None:
        plugins = getattr(self.rig, "plugins", {}) or {}
        ready = sum(1 for p in plugins.values() if getattr(p, "enabled", False))
        spend = sum(row.cost_usd for row in services.llm_costs(self.rig.engine))
        self.query_one("#boot", Static).update(
            f"⚡ {ready}/{len(plugins)} plugins  ·  {self._freshness()}  ·  ${spend:.2f} spent"
        )

    def _freshness(self) -> str:
        """Age of the newest price bar — the figure that decides if a report is stale."""
        health = services.data_health(self.rig)
        if not health.latest_bar:
            return "no price data"
        label, _state = age_text(datetime.now(UTC) - to_utc(max(health.latest_bar.values())))
        return f"data {label} old" if label != "live" else "data live"

    async def _refresh_statbox(self) -> None:
        """The pulse panel: what was published since the last visit."""
        box = self.query_one("#statbox", Vertical)
        await box.remove_children()

        watched = self._watched()
        pulse = services.pulse(
            self.rig.engine,
            instrument_ids=[instrument.id for instrument in watched],
            since=self.last_seen,
            days=PULSE_DAYS,
        )

        rows: list[Any] = [Static(_headline(pulse), classes="pulse-headline", markup=False)]
        if any(pulse.daily):
            rows.append(Sparkline(pulse.daily, classes="pulse-spark"))
            rows.append(
                Horizontal(
                    Static(f"{PULSE_DAYS} days ago", classes="pulse-axis-left", markup=False),
                    Static("today", classes="pulse-axis-right", markup=False),
                    classes="pulse-axis",
                )
            )
        if pulse.busiest:
            busiest = f"Busiest: {_symbol(watched, pulse.busiest[0])} ({pulse.busiest[1]})"
            quietest = (
                f"Quietest: {_symbol(watched, pulse.quietest[0])} ({pulse.quietest[1]})"
                if pulse.quietest
                else ""
            )
            rows.append(
                Horizontal(
                    Static(busiest, classes="pulse-busy", markup=False),
                    Static(quietest, classes="pulse-quiet", markup=False),
                    classes="pulse-rank",
                )
            )
        await box.mount(*rows)

    def _watched(self) -> list[Any]:
        """Universe instruments attached to a watch target.

        Matches on ``Instrument.watchlists`` (which holds target ids), and
        falls back to the target's tickers so a target still shows up when the
        universe was built without the back-reference.
        """
        specs = services.target_specs()
        target_ids = set(specs)
        symbols = {ticker.upper() for target in specs.values() for ticker in target.tickers}
        watched = []
        for instrument in self.rig.universe():
            linked = set(getattr(instrument, "watchlists", ()) or ()) & target_ids
            if linked or instrument.symbol.upper() in symbols:
                watched.append(instrument)
        return watched

    async def _refresh_tickers(self) -> None:
        holder = self.query_one("#tickers", Vertical)
        await holder.remove_children()
        watched = self._watched()
        if not watched:
            await holder.mount(self._row("nothing yet — press w to add a target", ""))
            return
        rows: list[Horizontal] = []
        for instrument in watched[:5]:
            closes = services.recent_closes(self.rig.engine, instrument.id)
            if not closes:
                continue
            last = closes[-1]
            first = closes[0]
            pct = ((last - first) / first * 100) if first else 0.0
            direction = "-up" if pct >= 0 else "-down"
            rows.append(
                Horizontal(
                    Static(instrument.symbol, classes="tick-sym", markup=False),
                    Static(f"{last:,.2f}", classes="tick-px", markup=False),
                    Static(f"{pct:+.1f}%", classes=f"tick-chg {direction}", markup=False),
                    Sparkline(closes),
                    classes="tick-row",
                )
            )
        await holder.mount(*rows)

    def _refresh_setup(self) -> None:
        """One status line, like the boot line: stay quiet unless something is wrong."""
        checks = services.setup_checks(self.rig)
        failing = [check.name for check in checks if not check.ok]
        line = self.query_one("#setup-line", Static)
        if failing:
            noun = "issue" if len(failing) == 1 else "issues"
            line.update(f"⚠ {len(failing)} setup {noun} — {', '.join(failing)}")
        else:
            line.update(f"✓ Setup ok — {len(checks)} checks passed")
        line.set_class(not failing, "-ok")
        line.set_class(bool(failing), "-bad")

    def _row(self, text: str, right: str) -> Horizontal:
        return Horizontal(
            Static(text, classes="sec-text", markup=False),
            Static(right, classes="sec-num", markup=False),
            classes="sec-row",
        )
