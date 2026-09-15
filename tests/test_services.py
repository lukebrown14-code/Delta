"""Service layer: research services and read-side queries, offline."""

from __future__ import annotations

from datetime import UTC, datetime
from types import SimpleNamespace

import pytest
from sqlmodel import Session

from rigger import services
from rigger.core.db import LLMCallTable
from rigger.core.models import Instrument
from rigger.plugins.markets.us import USMarket
from tests.conftest import FakeConfig, FakeLLM, seed_bars

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Tech")
MSFT = Instrument(id="US:MSFT", market="us", symbol="MSFT", currency="USD", sector="Tech")


class FakeRig:
    """Just enough of :class:`rigger.runtime.Rigger` for the services."""

    def __init__(self, engine, llm: FakeLLM, universe: list[Instrument]) -> None:
        self.engine = engine
        self.llm = llm
        self._universe = universe
        self.settings = SimpleNamespace(openrouter_api_key="", litellm_proxy_key="")
        self.cfg = SimpleNamespace(
            base_currency="AUD",
            llm_provider="openrouter",
            llm_routing={"analyse": "test/model"},
            plugins={"sec_edgar": {}},
            universe={"us": [i.symbol for i in universe]},
        )
        market = USMarket()
        market.configure({"tickers": [i.symbol for i in universe]})
        self.plugins = {
            "us": market,
            "sec_edgar": SimpleNamespace(enabled=True),
        }

    def universe(self) -> list[Instrument]:
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
    for inst in (AAPL, MSFT):
        seed_bars(tmp_engine, inst.id, price_fn=lambda i: 100.0)
    return FakeRig(tmp_engine, fake_llm, [AAPL, MSFT])


def test_setup_checks_flag_missing_key(rig):
    checks = {c.name: c for c in services.setup_checks(rig)}
    assert not checks["LLM provider (openrouter)"].ok
    assert "OPENROUTER_API_KEY" in checks["LLM provider (openrouter)"].fix
    assert checks["Price history"].ok
    assert not checks["SEC EDGAR contact"].ok


def test_data_health_counts_bars(rig):
    health = services.data_health(rig)
    assert health.counts["bar"] == 160
    assert set(health.latest_bar) >= {AAPL.id, MSFT.id}


def test_llm_costs_groups_by_task_and_model(tmp_engine):
    with Session(tmp_engine) as session:
        for i in range(3):
            session.add(
                LLMCallTable(
                    id=f"c{i}",
                    ts=datetime.now(UTC),
                    task="analyse",
                    model="m",
                    prompt_version="v",
                    prompt_hash=f"h{i}",
                    input_tokens=1,
                    output_tokens=1,
                    cost_usd=0.5,
                    latency_ms=1,
                    cached=False,
                )
            )
        session.commit()
    rows = services.llm_costs(tmp_engine)
    assert rows == [services.CostRow("analyse", "m", 3, 1.5)]


def test_watchlist_add_and_remove(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text('[universe]\nus = ["AAPL"]\n', encoding="utf-8")

    services.add_watchlist("mining", market="asx", tickers=["BHP", "RIO"])
    assert "mining" in services.watchlist_specs()
    services.remove_watchlist("mining")
    assert "mining" not in services.watchlist_specs()


def test_add_watchlist_unknown_market(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    with pytest.raises(ValueError, match="known markets"):
        services.add_watchlist("bogus", market="asz", tickers=["X"])
