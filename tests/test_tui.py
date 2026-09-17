"""TUI smoke tests: mount the app offline against a seeded engine."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

import pytest

from rigger import services
from rigger.core.models import Instrument
from rigger.tui.app import RiggerApp
from rigger.tui.shell import ALL_ITEMS, OFF_BAR_ITEMS, ScreenFooter, StatusBar
from rigger.tui.widgets import (
    MODAL_WIDTH,
    MODAL_WIDTH_WIDE,
    Dialog,
    KeyHint,
    binding_key,
    shown_bindings,
)
from tests.conftest import seed_bars

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Tech")


class FakeRig:
    def __init__(self, engine, universe):
        self.engine = engine
        self.llm = None
        self._universe = universe
        self.settings = SimpleNamespace(
            openrouter_api_key="", openai_api_key="", anthropic_api_key=""
        )
        self.cfg = SimpleNamespace(
            base_currency="AUD",
            llm_provider="openrouter",
            llm_routing={},
            plugins={"sec_edgar": {}},
            universe={},
            targets={},
        )
        self.plugins = {"sec_edgar": SimpleNamespace(enabled=True)}

    def universe(self):
        return self._universe

    def context(self, universe):
        from rigger.core.plugin import Context

        return Context(
            engine=self.engine,
            settings=self.settings,
            config=self.cfg,
            llm=self.llm,
            universe=universe,
            plugins=self.plugins,
        )


@pytest.fixture
def rig(tmp_engine, tmp_path):
    seed_bars(tmp_engine, AAPL.id, price_fn=lambda i: 100.0)
    fake = FakeRig(tmp_engine, [AAPL])
    # Keep the last-seen state file out of the repo: the app stamps a visit on start.
    fake.cfg.db_path = str(tmp_path / "rigger.db")
    return fake


def test_app_mounts_and_navigates(rig):
    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            assert app.screen.name == "home"
            for key, name, _label in ALL_ITEMS:
                await pilot.press(key)
                assert app.screen.name == name

    asyncio.run(run())


def test_status_bar_is_one_row(rig):
    async def run():
        app = RiggerApp(rig)
        async with app.run_test(size=(120, 24)) as pilot:
            # Home is a RiggerScreen like every panel: the bar is there where a
            # new user lands, so the 1/2/4/5 rail never disappears.
            assert app.screen.name == "home"
            bar = app.screen.query_one(StatusBar)
            assert not app.screen.query("#nav-console")
            # Off-bar screens stay reachable by key but earn no columns.
            for _key, name, _label in OFF_BAR_ITEMS:
                assert not app.screen.query(f"#nav-{name}")
            # Panels, status cells and chrome all share the one row.
            for sel in ("#nav-targets", "#nav-config", "#sl-dot", "#sl-keys"):
                assert bar.query(sel), sel
            assert app.screen.query_one(ScreenFooter).styles.height.value == 1
            await pilot.press("c")
            assert app.screen.name == "config"

    asyncio.run(run())


def test_status_bar_sheds_cells_when_narrow(rig):
    async def run():
        app = RiggerApp(rig)
        async with app.run_test(size=(70, 24)) as pilot:
            await pilot.press("2")
            bar = app.screen.query_one(StatusBar)
            # Below MINIMAL_WIDTH the hint and provider give up their columns;
            # freshness and spend are the cells worth keeping.
            assert not bar.query_one("#sl-keys").display
            assert bar.query_one("#sl-dot").display

    asyncio.run(run())


def test_targets_panel_adds_target(rig, monkeypatch, tmp_path):
    import tomli_w

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps({"universe": {"us": ["AAPL"]}}), encoding="utf-8"
    )

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            await pilot.press("1")
            assert app.screen.name == "targets"
            await pilot.press("a")
            assert app.screen.query_one("#tg-form").display
            app.screen.query_one("#tg-name").value = "mining"
            app.screen.query_one("#tg-kind").value = "industry"
            app.screen.query_one("#tg-market").value = "asx"
            app.screen.query_one("#tg-tickers").value = "BHP,RIO"
            await pilot.press("enter")
            assert "mining" in services.target_specs()
            assert app.screen.query_one("#target-table").row_count == 1

    asyncio.run(run())


def test_question_mark_opens_help_then_closes(rig):
    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            assert app.screen.name == "home"
            await pilot.press("?")
            assert app.screen.name == "help"
            await pilot.press("?")
            assert app.screen.name == "home"

    asyncio.run(run())


def test_targets_panel_remove_with_no_targets_notifies(rig, monkeypatch, tmp_path):
    """An empty DataTable reports cursor_row == 0, so row_count is the real guard."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            await pilot.press("1")
            assert app.screen.query_one("#target-table").row_count == 0
            await pilot.press("d")
            await pilot.pause()

    asyncio.run(run())


def test_home_pulse_reference_point_is_stable_across_refreshes(rig):
    """last_seen is captured once per session; re-reading it would zero the panel."""

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            home = app.screen
            captured = home.last_seen
            await home.refresh_view()
            await pilot.pause()
            await home.refresh_view()
            await pilot.pause()
            assert home.last_seen == captured

    asyncio.run(run())


@pytest.fixture(autouse=True)
def offline_quotes(monkeypatch):
    from rigger.quotes import YahooQuotes

    async def run(self):
        self.on_state("offline test")
        await asyncio.Event().wait()

    monkeypatch.setattr(YahooQuotes, "run", run)


@pytest.mark.parametrize("size", [(80, 24), (120, 40)])
@pytest.mark.parametrize("theme", ["rigger-dark", "rigger-light"])
def test_ledger_groups_quotes_and_focus(rig, monkeypatch, tmp_path, size, theme):
    import tomli_w

    from rigger.quotes import parse_quote

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {
                "targets": {
                    "mining": {
                        "kind": "sector",
                        "market": "asx",
                        "tickers": ["BHP", "RIO"],
                        "tags": ["resources"],
                    },
                    "whole-market": {"kind": "market", "market": "us"},
                }
            }
        )
    )

    async def run():
        app = RiggerApp(rig)
        async with app.run_test(size=size) as pilot:
            app.theme = theme
            await pilot.press("1")
            screen = app.screen
            table = screen.query_one("#target-table")
            assert table.row_count == 2
            await pilot.press("enter")
            # Enter refreshes the always-open inspector; grouped assets are
            # no longer expanded into child rows.
            assert table.row_count == 2
            # A multi-ticker target steps through its members with the arrows.
            assert screen._members_by_target["mining"] == ["ASX:BHP", "ASX:RIO"]
            assert screen._selected_instrument.id == "ASX:BHP"
            await pilot.press("right")
            assert screen._selected_instrument.id == "ASX:RIO"
            await pilot.press("left")
            assert screen._selected_instrument.id == "ASX:BHP"
            assert len(screen.feed.symbols) == 2
            screen.feed.quotes["ASX:BHP"] = parse_quote(
                {"price": 42.18, "time": 1789516800000, "change_percent": -0.4}, "AUD"
            )
            selected = screen._selected()
            screen._paint_quotes()
            assert screen._selected() == selected
            assert "child:mining:ASX:BHP" not in screen.rows
            await pilot.press("d")
            assert "mining" not in services.target_specs()
            await pilot.press("/", "r", "e", "s")
            assert table.row_count == 0
            await pilot.press("escape")
            assert table.row_count == 1
            assert table.region.right <= size[0]
            task = screen.feed_task
            await pilot.press("2")
            assert task.done()
            assert screen.feed_task is None

    asyncio.run(run())


@pytest.mark.parametrize("field", ["name", "kind", "market", "tickers", "tags"])
def test_company_form_enter_submits(rig, monkeypatch, tmp_path, field):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            await pilot.press("1", "a")
            screen = app.screen
            screen.query_one("#tg-name").value = "apple"
            screen.query_one("#tg-market").value = "us"
            screen.query_one("#tg-tickers").value = "AAPL"
            screen.query_one(f"#tg-{field}").focus()
            await pilot.press("enter")
            assert services.target_specs()["apple"].tickers == ("AAPL",)
            assert app.screen.name == "targets"
            assert not app.screen.query("#tg-form")
            assert app.screen.query_one("#target-table").has_focus

    asyncio.run(run())


def test_company_form_enter_keeps_invalid_form_open(rig, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            await pilot.press("1", "a")
            app.screen.query_one("#tg-name").value = "apple"
            await pilot.press("enter")
            assert not services.target_specs()
            assert app.screen.query_one("#tg-form").display
            assert app.screen.query_one("#tg-name").value == "apple"

    asyncio.run(run())


def test_add_modal_yahoo_dropdown_keyboard_and_online_state(rig, monkeypatch, tmp_path):
    from rigger import tui
    from rigger.quotes import SearchResult
    from rigger.tui.screens.targets import TargetAddModal

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def yahoo(query):
        assert query == "bhp"
        return [SearchResult("BHP.AX", "BHP Group Limited", "asx", "AUD", "ASX")]

    monkeypatch.setattr(tui.screens.targets, "yahoo_search", yahoo)

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            await pilot.press("1", "a")
            assert isinstance(app.screen, TargetAddModal)
            app.screen.query_one("#tg-name").value = "bhp"
            await pilot.pause()
            suggestions = app.screen.query_one("#tg-suggestions")
            assert suggestions.display
            assert app.screen.query_one("#tg-network").has_class("-online")
            await pilot.press("down")
            assert suggestions.has_focus
            await pilot.press("enter")
            assert app.screen.query_one("#tg-tickers").value == "BHP.AX"
            assert app.screen.query_one("#tg-market").value == "asx"
            app.screen.query_one("#tg-name").value = "manual"
            app.screen.query_one("#tg-name").focus()
            await pilot.pause(0.2)
            await pilot.press("escape")
            assert isinstance(app.screen, TargetAddModal)
            assert not app.screen.query_one("#tg-suggestions").display

    asyncio.run(run())


def test_theme_tokens_are_readable_on_black():
    """Base tokens are ink; anything with ``color:`` must use a ``text-`` token."""
    import re
    from pathlib import Path

    from textual.color import Color

    from rigger.tui.theme import RIGGER_DARK

    tui = Path(__file__).resolve().parents[1] / "rigger" / "tui"
    pattern = re.compile(r"color: \$(primary|secondary|accent|success|error|warning)\b")
    # A graphic is ink, not text: the braille graph's low end is the brand
    # blue on purpose, and reading it never depends on that contrast — the
    # bright end and the figures above the chart carry the meaning.
    graphic = re.compile(r"--(low|high)-color")
    offenders = [
        f"{path.relative_to(tui)}:{n}"
        for path in list(tui.rglob("*.py")) + list(tui.rglob("*.tcss"))
        if path.name != "theme.py"
        for n, line in enumerate(path.read_text().splitlines(), 1)
        if pattern.search(line) and not graphic.search(line)
    ]
    assert not offenders, offenders

    def luminance(color: Color) -> float:
        def channel(value: int) -> float:
            c = value / 255
            return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

        return 0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)

    black = luminance(Color.parse(RIGGER_DARK.background))
    for token in ("text-primary", "text-error", "text-success", "text-warning", "text-muted"):
        ratio = (luminance(Color.parse(RIGGER_DARK.variables[token])) + 0.05) / (black + 0.05)
        assert ratio >= 4.5, (token, ratio)


def _routed_rig(rig):
    rig.cfg.llm_routing = {"chat": "x/sonnet", "extract": "x/haiku", "report": "x/sonnet"}
    rig.cfg.targets = {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}
    return rig


def test_config_two_columns_fold_and_focus_keys(rig):
    """Wide: diagnostics open beside the stack; d folds; l/t move focus; enter opens the picker."""
    from rigger.tui.screens.model_picker import ModelPicker

    async def run():
        app = RiggerApp(_routed_rig(rig))
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("c")
            screen = app.screen
            assert screen.name == "config"
            assert not screen.query("Collapsible")
            assert screen.query_one("#cfg-diag-body").display
            assert not screen.query_one("#cfg-diag-summary").display
            assert screen.query_one("#cfg-left").display
            assert screen.focused is screen.query_one("#cfg-routing")
            await pilot.press("d")
            assert screen.query_one("#cfg-diag-summary").display
            assert not screen.query_one("#cfg-diag-body").display
            await pilot.press("d")
            assert screen.query_one("#cfg-diag-body").display
            await pilot.press("l")
            assert screen.focused is screen.query_one("#cfg-plugins")
            await pilot.press("t")
            assert screen.focused is screen.query_one("#cfg-targets")
            # The targets pane is a signpost: no phantom add key.
            empty = str(screen.query_one("#cfg-targets-empty").render())
            assert "press w" not in empty and "1 Watchlist" in empty
            # Enter on a routing row opens the model picker for that task.
            screen.query_one("#cfg-routing").focus()
            await pilot.press("enter")
            await pilot.pause()
            assert isinstance(app.screen, ModelPicker)

    asyncio.run(run())


def test_config_narrow_folds_diagnostics_until_d(rig):
    """80x24: one column, diagnostics folded; d expands full-height, esc closes."""

    async def run():
        app = RiggerApp(_routed_rig(rig))
        async with app.run_test(size=(80, 24)) as pilot:
            await pilot.press("c")
            screen = app.screen
            assert screen.has_class("-narrow")
            assert screen.query_one("#cfg-diag-summary").display
            assert not screen.query_one("#cfg-diag-body").display
            assert screen.query_one("#cfg-left").display
            # Nothing may fall off the bottom: the body ends above the footer.
            body = screen.query_one("#cfg-body")
            assert body.region.bottom <= screen.size.height - 1
            await pilot.press("d")
            assert not screen.query_one("#cfg-left").display
            assert screen.query_one("#cfg-diag-body").display
            await pilot.press("escape")
            assert screen.query_one("#cfg-left").display
            assert not screen.query_one("#cfg-diag-body").display

    asyncio.run(run())


def test_home_refreshes_twice_without_duplicate_ids(rig, monkeypatch, tmp_path):
    """Home's boxes update in place.

    A remove-then-mount rebuild raced Textual's async ``remove_children`` and
    crashed with DuplicateIds on the second refresh — the first repaint after
    the screen had already been drawn once.
    """
    import tomli_w

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {"targets": {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}}
        ),
        encoding="utf-8",
    )

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            home = app.screen
            assert home.name == "home"
            for _ in range(3):
                await home.refresh_view()
                await pilot.pause()
            assert len(app.screen.query("#system-db")) == 1
            assert len(app.screen.query("#system-plugins")) == 1

    asyncio.run(run())


def test_braille_graph_paints_in_a_running_app():
    """The graph reaches the screen, not just the renderable.

    Asserting on ``rows()`` alone passes for a widget Textual never paints;
    this drives a real app and looks for braille in the exported frame.
    """
    from textual.app import App, ComposeResult

    from rigger.tui.widgets import BrailleGraph

    class GraphApp(App):
        CSS = "BrailleGraph { width: 40; height: 6; }"

        def compose(self) -> ComposeResult:
            yield BrailleGraph([float(i) for i in range(120)], id="g")

    async def run():
        app = GraphApp()
        async with app.run_test(size=(60, 12)) as pilot:
            await pilot.pause()
            frame = app.export_screenshot()
            assert sum(1 for ch in frame if 0x2800 <= ord(ch) <= 0x28FF) > 20

    asyncio.run(run())


def test_braille_graph_resolution_and_shape():
    """The graph uses the whole box: 4 dot rows per cell, 2 sample columns."""
    from rigger.tui.widgets import BrailleGraph

    rising = BrailleGraph(list(range(100)))
    rows = rising.rows(20, 4)
    assert len(rows) == 4
    assert all(len(row) == 20 for row in rows)
    # A rising series ends high and starts low: the first cell of the top row
    # is blank, the last is not, and the bottom row is the other way round.
    assert rows[0][0] == BrailleGraph.EMPTY and rows[0][-1] != BrailleGraph.EMPTY
    assert rows[-1][0] != BrailleGraph.EMPTY and rows[-1][-1] == BrailleGraph.EMPTY

    # Filled draws every dot below the line, so a full box is denser than a line.
    line = BrailleGraph(list(range(100)))
    area = BrailleGraph(list(range(100)), fill=True)
    dots = lambda g: sum(  # noqa: E731
        bin(ord(ch) - 0x2800).count("1") for row in g.rows(20, 4) for ch in row
    )
    assert dots(area) > dots(line) * 3

    # The widget must actually paint. Textual's Widget.BLANK means "render
    # nothing", so a constant of that name on the subclass silently blanks it
    # before render() is ever called — which is how this shipped broken once.
    assert BrailleGraph.BLANK is False

    # Degenerate input must not raise or divide by zero.
    assert BrailleGraph([]).rows(10, 2) == [BrailleGraph.EMPTY * 10] * 2
    assert BrailleGraph([5.0, 5.0, 5.0]).rows(10, 2)
    assert BrailleGraph([1.0]).rows(0, 0) == []


# --- dialogs -------------------------------------------------------------------


def _assert_dialog(screen, size: tuple[int, int], width: int) -> None:
    """A dialog is framed, dimmed, centred and inside the terminal.

    Checked against the laid-out regions rather than the widget tree: a green
    tree has twice hidden a box that never painted a column.
    """
    frame = screen.query_one("#dialog-frame")
    region = frame.region
    assert region.width == width, (region, width)
    assert region.height > 0
    # Framed: a border on every edge, not a bare box of text.
    edges = screen.query_one("#dialog-frame").styles.border
    assert all(edge[0] == "solid" for edge in edges), edges
    # Dimmed backdrop: the modal's own background is translucent.
    assert screen.styles.background.a < 1.0
    # Centred, and clipping nothing at either size.
    w, h = size
    assert region.x > 0 and region.right <= w
    assert abs(region.x - (w - region.right)) <= 1, (region, w)
    assert region.y >= 0 and region.bottom <= h, (region, h)
    # The key hint is the last row inside the frame and the first thing a
    # too-tall dialog loses: it slid under the bottom border once, with the
    # frame itself still measuring as unclipped.
    hint = screen.query_one("#dialog-hint")
    assert frame.content_region.contains_region(hint.region), (hint.region, frame.content_region)


@pytest.mark.parametrize(
    ("key", "width"),
    [("m", MODAL_WIDTH_WIDE), ("p", MODAL_WIDTH), ("g", MODAL_WIDTH), ("?", MODAL_WIDTH_WIDE)],
)
@pytest.mark.parametrize("size", [(80, 24), (120, 40)])
def test_modals_are_framed_centred_and_escapable(rig, key, width, size):
    """Every modal is a Dialog: framed, dimmed, centred, and closed by escape."""

    async def run():
        app = RiggerApp(rig)
        async with app.run_test(size=size) as pilot:
            await pilot.press(key)
            await pilot.pause()
            modal = app.screen
            assert isinstance(modal, Dialog), modal
            _assert_dialog(modal, size, width)
            # The dialog owns escape: one press and the app is back home.
            await pilot.press("escape")
            await pilot.pause()
            assert app.screen.name == "home"

    asyncio.run(run())


def test_provider_key_modals_are_dialogs(rig):
    """The two provider forms are Dialogs too, framed and closed by escape.

    Pushed onto the real app, not a bare one: the app stylesheet is what makes
    an Input one row rather than Textual's three, and these forms hold three
    of them.
    """
    from rigger.llm.providers import PROVIDERS
    from rigger.tui.screens.provider_picker import CustomFormModal, KeyEntryModal

    async def run():
        app = RiggerApp(rig)
        async with app.run_test(size=(80, 24)) as pilot:
            for modal in (KeyEntryModal(PROVIDERS["openai"]), CustomFormModal()):
                app.push_screen(modal)
                await pilot.pause()
                assert isinstance(app.screen, Dialog)
                _assert_dialog(app.screen, (80, 24), MODAL_WIDTH)
                await pilot.press("escape")
                await pilot.pause()
                assert app.screen is not modal

    asyncio.run(run())


def test_help_lists_per_screen_keys(rig):
    """The keymap covers the screens, not just the app-level bindings."""
    from rigger.tui.screens.theses import Theses
    from rigger.tui.widgets import KeyGrid

    async def run():
        app = RiggerApp(rig)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("?")
            await pilot.pause()
            help_screen = app.screen
            await pilot.press("k")
            await pilot.pause()
            assert help_screen.query_one("#help-tabs").active == "help-keys"
            groups = [str(s.render()) for s in help_screen.query(".help-group")]
            assert "Anywhere" in groups and "Theses" in groups
            # A key only Theses binds must be on screen, and the grid painted.
            keys = {
                str(hint.render())
                for grid in help_screen.query(KeyGrid)
                for hint in grid.query(KeyHint)
            }
            thesis_keys = {binding_key(b) for b in shown_bindings(Theses.BINDINGS)}
            assert thesis_keys and thesis_keys <= keys, thesis_keys - keys
            assert all(grid.region.height > 0 for grid in help_screen.query(KeyGrid))
            # t switches back to the tour.
            await pilot.press("t")
            await pilot.pause()
            assert help_screen.query_one("#help-tabs").active == "help-tour"

    asyncio.run(run())
