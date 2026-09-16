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
        self.settings = SimpleNamespace(openrouter_api_key="", openai_api_key="", anthropic_api_key="")
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


def test_target_add_and_remove(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text('[universe]\nus = ["AAPL"]\n', encoding="utf-8")

    services.add_target(
        "mining",
        kind="industry",
        market="asx",
        tickers=["BHP", "RIO"],
        tags=["diggers"],
        notes="big miners",
    )
    specs = services.target_specs()
    assert specs["mining"].kind == "industry"
    assert specs["mining"].tickers == ("BHP", "RIO")
    assert specs["mining"].tags == frozenset({"diggers"})
    assert specs["mining"].notes == "big miners"
    assert "universe_us" not in specs

    services.remove_target("mining")
    assert "mining" not in services.target_specs()


def test_add_target_unknown_market(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    with pytest.raises(ValueError, match="known markets"):
        services.add_target("bogus", market="asz", tickers=["X"])


def test_add_target_unknown_kind(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    with pytest.raises(ValueError, match="known kinds"):
        services.add_target("bogus", kind="planet", market="asx", tickers=["X"])


def test_add_target_market_kind_takes_no_tickers(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    services.add_target("aussie", kind="market", market="asx")
    assert services.target_specs()["aussie"].tickers == ()
    with pytest.raises(ValueError, match="takes no tickers"):
        services.add_target("aus2", kind="market", market="asx", tickers=["BHP"])


def test_add_target_non_market_kind_requires_tickers(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    with pytest.raises(ValueError, match="requires tickers"):
        services.add_target("bhp", kind="company", market="asx", tickers=[])


def test_remove_target_also_removes_legacy_watchlist(tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[watchlists.mining]\nmarket = "asx"\ntickers = ["BHP"]\n', encoding="utf-8"
    )
    services.remove_target("mining")
    assert services.target_specs() == {}
    with pytest.raises(KeyError, match="unknown target"):
        services.remove_target("mining")
