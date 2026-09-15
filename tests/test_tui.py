"""TUI smoke tests: mount the app offline against a seeded engine."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime
from types import SimpleNamespace

import pytest
from sqlmodel import Session

from rigger.core.models import Instrument, Signal
from rigger.paper.portfolio import PaperPortfolio
from rigger.plugins.brokers.paper import PaperBroker
from rigger.plugins.markets.us import USMarket
from rigger.plugins.strategies.llm_analyst import LLMAnalyst
from rigger.tui.app import RiggerApp
from tests.conftest import FakeConfig, seed_bars

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Tech")


class FakeRig:
    def __init__(self, engine, llm, universe):
        self.engine = engine
        self.llm = llm
        self._universe = universe
        self.settings = SimpleNamespace(openrouter_api_key="", litellm_proxy_key="")
        self.cfg = SimpleNamespace(
            base_currency="AUD",
            llm_provider="openrouter",
            llm_routing={"analyse": "test/model"},
            llm_ensemble_models=[],
            risk={"min_conviction": 0.6},
            paper_starting_cash=100_000.0,
            plugins={"sec_edgar": {}},
            universe={"us": [i.symbol for i in universe]},
        )
        self.portfolio = PaperPortfolio(engine, "AUD", 0.0, currencies={"us": "USD"})
        broker = PaperBroker()
        broker.bind(self.portfolio)
        market = USMarket()
        market.configure({"tickers": [i.symbol for i in universe]})
        self.plugins = {
            "llm_analyst": LLMAnalyst(),
            "paper": broker,
            "us": market,
            "sec_edgar": SimpleNamespace(enabled=True),
        }

    def universe(self):
        return self._universe

    def context(self, universe):
        from rigger.core.plugin import Context

        return Context(
            engine=self.engine,
            settings=self.settings,
            config=FakeConfig(self.cfg.llm_routing),
            llm=self.llm,
            universe=universe,
            plugins=self.plugins,
        )


@pytest.fixture
def rig(tmp_engine, fake_llm):
    seed_bars(tmp_engine, AAPL.id, price_fn=lambda i: 100.0)
    seed_bars(tmp_engine, "FX:USDAUD", n=1, price_fn=lambda i: 1.5)
    return FakeRig(tmp_engine, fake_llm, [AAPL])


def _seed_signal(engine):
    from rigger.runtime import store_signal

    signal = Signal(
        id="sig-1",
        ts=datetime.now(UTC),
        instrument_id=AAPL.id,
        strategy="llm_analyst",
        direction="long",
        conviction=0.8,
        horizon_days=20,
        thesis="Positive momentum thesis.",
        invalidation="Break of 50-day SMA.",
        evidence_ids=["1", "2", "3"],
    )
    with Session(engine) as session:
        store_signal(session, signal)
        session.commit()


def test_app_mounts_and_signals_screen_lists_signals(rig):
    async def run():
        _seed_signal(rig.engine)
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            assert app.screen.name == "home"
            await pilot.press("2")
            assert app.screen.name == "signals"
            table = app.screen.query_one("#signals-table")
            assert table.row_count >= 1

    asyncio.run(run())


def test_app_mounts_offline_home(rig):
    async def run():
        app = RiggerApp(rig)
        async with app.run_test() as pilot:
            assert app.screen.name == "home"
            await pilot.press("3")
            assert app.screen.name == "portfolio"

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
