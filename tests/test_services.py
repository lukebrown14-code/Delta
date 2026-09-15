"""Service layer: pipeline steps and read-side queries, offline."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace

import pytest
from sqlmodel import Session

from rigger import services
from rigger.core.db import EventTable, FundamentalTable, NewsItemTable
from rigger.core.models import Instrument, Signal
from rigger.paper.portfolio import PaperPortfolio
from rigger.plugins.brokers.paper import PaperBroker
from rigger.plugins.markets.us import USMarket
from rigger.plugins.strategies.llm_analyst import LLMAnalyst
from rigger.runtime import store_signal
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
    seed_bars(tmp_engine, "FX:USDAUD", n=1, price_fn=lambda i: 1.5)
    return FakeRig(tmp_engine, fake_llm, [AAPL, MSFT])


def test_analyse_stores_signals_and_logs(rig):
    lines: list[str] = []
    signals = asyncio.run(services.analyse(rig, log=lines.append))
    assert len(signals) == 2
    assert services.signals(rig.engine) and len(services.signals(rig.engine)) == 2
    assert any("Stored 2 signals" in line for line in lines)


def test_analyse_dry_run_stores_nothing(rig):
    asyncio.run(services.analyse(rig, dry_run=True))
    assert services.signals(rig.engine) == []


def test_execute_fills_then_skips_already_long(rig):
    asyncio.run(services.analyse(rig))
    first = asyncio.run(services.execute(rig))
    assert len(first.fills) == 2
    # Fill price is USD, cash moved in AUD at 1.5.
    spent = 100_000.0 - rig.portfolio.cash()
    assert spent == pytest.approx(sum(f.qty * f.price for f in first.fills) * 1.5)

    asyncio.run(services.analyse(rig))  # a second run's signals
    second = asyncio.run(services.execute(rig))
    assert second.fills == []
    assert sorted(second.skipped) == [("US:AAPL", "already long"), ("US:MSFT", "already long")]


def test_execute_ignores_stale_signals_unless_all(rig):
    old = Signal(
        id="old",
        ts=datetime.now(UTC) - timedelta(days=10),
        instrument_id=AAPL.id,
        strategy="t",
        direction="long",
        conviction=0.9,
        horizon_days=5,
        thesis="t",
        invalidation="i",
    )
    with Session(rig.engine) as session:
        store_signal(session, old)
        session.commit()
    assert asyncio.run(services.execute(rig)).fills == []
    assert len(asyncio.run(services.execute(rig, all_=True)).fills) == 1


def test_resolve_evidence_groups_mixed_ids(tmp_engine):
    seed_bars(tmp_engine, AAPL.id, n=3, price_fn=lambda i: [10.0, 12.0, 11.0][i])
    now = datetime.now(UTC)
    with Session(tmp_engine) as session:
        session.add(
            NewsItemTable(
                id="news1",
                instrument_ids='["US:AAPL"]',
                published=now,
                title="T",
                url="u",
                source="rss",
            )
        )
        session.add(
            EventTable(
                id="ev1",
                instrument_id=AAPL.id,
                ts=now,
                kind="earnings",
                summary="s",
                sentiment=0.1,
                evidence_ids="[]",
                extracted_by="m",
                prompt_version="v",
            )
        )
        session.add(
            FundamentalTable(
                id=7,
                instrument_id=AAPL.id,
                as_of=now.date(),
                metric="eps",
                value=1.0,
                source="edgar",
            )
        )
        session.commit()

    ev = services.resolve_evidence(
        tmp_engine, ["1", "2", "3", "news1", "ev1", "fundamental:7", "ghost"]
    )
    assert ev.bars.count == 3
    assert (ev.bars.low, ev.bars.high) == (10.0, 12.0)
    assert [n.id for n in ev.news] == ["news1"]
    assert [e.id for e in ev.events] == ["ev1"]
    assert [f.metric for f in ev.fundamentals] == ["eps"]
    assert ev.unresolved == ["ghost"]
    assert ev.total == 6


def test_setup_checks_flag_missing_key_and_fx(rig):
    checks = {c.name: c for c in services.setup_checks(rig)}
    assert not checks["LLM provider (openrouter)"].ok
    assert "OPENROUTER_API_KEY" in checks["LLM provider (openrouter)"].fix
    assert checks["Price history"].ok
    assert checks["FX rates"].ok
    assert not checks["SEC EDGAR contact"].ok


def test_data_health_and_portfolio_summary(rig):
    asyncio.run(services.analyse(rig))
    asyncio.run(services.execute(rig))
    health = services.data_health(rig)
    assert health.counts["signal"] == 2
    assert health.counts["fill"] == 2
    assert health.fx_rates == {"USD/AUD": 1.5}
    assert set(health.latest_bar) >= {AAPL.id, MSFT.id}

    summary = services.portfolio_summary(rig)
    assert len(summary.positions) == 2
    assert all(v.currency == "USD" for v in summary.positions)
    assert summary.equity == pytest.approx(100_000.0)
    assert len(summary.fills) == 2


def test_llm_costs_groups_by_task_and_model(rig, fake_llm):
    # FakeLLM does not write llmcall rows; seed directly.
    from rigger.core.db import LLMCallTable

    with Session(rig.engine) as session:
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
    rows = services.llm_costs(rig.engine)
    assert rows == [services.CostRow("analyse", "m", 3, 1.5)]
