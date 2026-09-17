"""TUI smoke tests: mount the app offline against a seeded engine."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

import pytest

from rigger import services
from rigger.core.models import Instrument
from rigger.tui.app import RiggerApp
from rigger.tui.shell import ALL_ITEMS, OFF_BAR_ITEMS, ScreenFooter, StatusBar
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
            # Home is a splash screen with no shell chrome; the bar lives on
            # the panel screens.
            await pilot.press("2")
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
    offenders = [
        f"{path.relative_to(tui)}:{n}"
        for path in list(tui.rglob("*.py")) + list(tui.rglob("*.tcss"))
        if path.name != "theme.py"
        for n, line in enumerate(path.read_text().splitlines(), 1)
        if pattern.search(line)
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
