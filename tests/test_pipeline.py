"""End-to-end pipeline test without network using FakeLLM."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

from sqlmodel import Session

from rigger.core.db import BarTable
from rigger.core.models import Instrument
from rigger.core.plugin import Context, Report
from rigger.paper.portfolio import PaperPortfolio
from rigger.paper.risk import RiskLimits, size_signal
from rigger.plugins.reports.markdown import MarkdownReport
from rigger.plugins.strategies.llm_analyst import LLMAnalyst


def _seed_bars(engine, instrument_id: str, n: int = 80, base: float = 100.0) -> None:
    with Session(engine) as session:
        ts = datetime.now(UTC) - timedelta(days=n)
        for i in range(n):
            price = base + i * 0.5
            session.add(
                BarTable(
                    instrument_id=instrument_id,
                    ts=ts + timedelta(days=i),
                    open=price,
                    high=price + 1,
                    low=price - 1,
                    close=price,
                    volume=1000.0,
                    source="test",
                )
            )
        session.commit()


class _Cfg:
    llm_routing = {"analyse": "test/model"}


def test_pipeline_end_to_end(tmp_engine, fake_llm, tmp_path):
    inst = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Technology")
    _seed_bars(tmp_engine, inst.id)

    ctx = Context(engine=tmp_engine, settings=None, config=_Cfg(), llm=fake_llm, universe=[inst])

    analyst = LLMAnalyst()
    signals = asyncio.run(analyst.generate(ctx))
    assert len(signals) == 1
    sig = signals[0]
    assert sig.direction in ("long", "short", "flat")
    assert sig.evidence_ids, "signal must carry evidence"
    assert sig.thesis and sig.invalidation

    # store signal
    from rigger.cli import _store_signal

    with Session(tmp_engine) as session:
        _store_signal(session, sig)
        session.commit()

    # risk + sizing + paper fill
    portfolio = PaperPortfolio(tmp_engine, "AUD", 5.0)
    limits = RiskLimits()
    decision = size_signal(sig, inst, portfolio.equity(), 140.0, limits)
    assert decision.approved
    from rigger.core.models import Order

    order = Order(
        id="ord-1",
        signal_id=sig.id,
        instrument_id=inst.id,
        side="buy",
        qty=decision.qty,
        type="market",
        submitted_ts=datetime.now(UTC),
        broker="paper",
    )
    fill = portfolio.fill(order, market="us", fill_price=140.0)
    assert fill.qty == decision.qty

    positions = portfolio.positions()
    assert len(positions) == 1

    report = Report(
        date="2026-09-14",
        signals=[sig],
        orders=[order],
        fills=[fill],
        positions=positions,
        cash=portfolio.cash(),
    )
    path = MarkdownReport().render(report)
    assert path.exists()
    content = path.read_text()
    assert "AAPL" in content
    assert sig.thesis in content
