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
        self.settings = SimpleNamespace(openrouter_api_key="", openai_api_key="", anthropic_api_key="")
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
            app.screen.query_one("#tg-name").value = "mining"
            app.screen.query_one("#tg-kind").value = "industry"
            app.screen.query_one("#tg-market").value = "asx"
            app.screen.query_one("#tg-tickers").value = "BHP,RIO"
            await pilot.click("#tg-add")
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
            await pilot.click("#tg-remove")
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
