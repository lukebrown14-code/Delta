"""TUI smoke tests: mount the app offline against a seeded engine."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

import pytest

from rigger import services
from rigger.core.models import Instrument
from rigger.tui.app import RiggerApp
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
def rig(tmp_engine):
    seed_bars(tmp_engine, AAPL.id, price_fn=lambda i: 100.0)
    return FakeRig(tmp_engine, [AAPL])


def test_app_mounts_and_navigates(rig):
    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            assert app.screen.name == "home"
            await pilot.press("2")
            assert app.screen.name == "data"
            await pilot.press("3")
            assert app.screen.name == "config"

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
            await pilot.press("w")
            assert app.screen.name == "targets"
            app.screen.query_one("#tg-name").value = "mining"
            app.screen.query_one("#tg-kind").value = "industry"
            app.screen.query_one("#tg-market").value = "asx"
            app.screen.query_one("#tg-tickers").value = "BHP,RIO"
            await pilot.click("#tg-add")
            assert "mining" in services.target_specs()
            assert app.screen.query_one("#target-table").row_count == 1

    asyncio.run(run())


def test_console_target_list(rig, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[targets.mining]\nkind = "industry"\nmarket = "asx"\ntickers = ["BHP"]\n',
        encoding="utf-8",
    )

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            await pilot.press("c")
            assert app.screen.name == "console"
            app.screen.query_one("#console-input").value = "target list"
            await pilot.press("enter")
            assert any("mining" in line.text for line in app.screen.query_one("#console-log").lines)

    asyncio.run(run())


def test_console_target_add(rig, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            await pilot.press("c")
            app.screen.query_one(
                "#console-input"
            ).value = "target add gold --kind sector --market asx --tickers BHP,RIO --tags miners"
            await pilot.press("enter")
            target = services.target_specs()["gold"]
            assert target.kind == "sector"
            assert target.tickers == ("BHP", "RIO")
            assert target.tags == frozenset({"miners"})
            assert any(
                "Added target gold" in line.text
                for line in app.screen.query_one("#console-log").lines
            )

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
            await pilot.press("w")
            assert app.screen.query_one("#target-table").row_count == 0
            await pilot.click("#tg-remove")
            await pilot.pause()

    asyncio.run(run())
