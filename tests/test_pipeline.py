"""End-to-end pipeline test without network using FakeLLM."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime

from sqlmodel import Session

from rigger.core.db import BarTable
from rigger.core.models import Instrument
from rigger.core.plugin import Context, Report
from rigger.paper.portfolio import PaperPortfolio
from rigger.paper.risk import RiskLimits, size_signal
from rigger.plugins.reports.markdown import MarkdownReport
from rigger.plugins.strategies.llm_analyst import LLMAnalyst
from tests.conftest import FakeConfig, seed_bars


def test_pipeline_end_to_end(tmp_engine, fake_llm, tmp_path):
    inst = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Technology")
    seed_bars(tmp_engine, inst.id)

    ctx = Context(
        engine=tmp_engine,
        settings=None,
        config=FakeConfig({"analyse": "test/model"}),
        llm=fake_llm,
        universe=[inst],
    )

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


def test_signal_metadata_round_trips(tmp_engine):
    from datetime import UTC, datetime

    from sqlmodel import select

    from rigger.cli import _store_signal
    from rigger.core.db import SignalTable
    from rigger.core.models import Signal

    sig = Signal(
        id="sig-meta",
        ts=datetime.now(UTC),
        instrument_id="US:AAPL",
        strategy="critic:llm_analyst",
        direction="long",
        conviction=0.5,
        horizon_days=10,
        thesis="t",
        invalidation="i",
        evidence_ids=["1"],
        metadata={"critic": {"verdict": "reduce", "risks": ["a", "b"]}},
    )
    with Session(tmp_engine) as session:
        _store_signal(session, sig)
        session.commit()
    with Session(tmp_engine) as session:
        row = session.exec(select(SignalTable).where(SignalTable.id == "sig-meta")).one()
        assert row.metadata_ == {"critic": {"verdict": "reduce", "risks": ["a", "b"]}}


def test_ingest_stores_all_row_types(tmp_engine):
    from datetime import UTC, date, datetime

    from sqlmodel import select

    from rigger.core.db import EventTable, FundamentalTable, NewsItemTable, store_items
    from rigger.core.models import Bar, Event, Fundamental, NewsItem

    now = datetime.now(UTC)
    fetched = [
        Bar(
            instrument_id="US:AAPL",
            ts=now,
            open=1,
            high=2,
            low=0.5,
            close=1.5,
            volume=10,
            source="t",
        ),
        NewsItem(
            id="n1", instrument_ids=["US:AAPL"], published=now, title="x", url="u", source="t"
        ),
        Fundamental(
            instrument_id="US:AAPL", as_of=date(2026, 1, 1), metric="eps", value=1.0, source="t"
        ),
        Event(
            id="e1",
            instrument_id="US:AAPL",
            ts=now,
            kind="earnings",
            summary="s",
            sentiment=0.0,
            extracted_by="t",
            prompt_version="n/a",
        ),
    ]
    first = store_items(tmp_engine, fetched)
    second = store_items(tmp_engine, fetched)  # re-ingest must not duplicate
    assert first == {"bar": 1, "newsitem": 1, "event": 1, "fundamental": 1}
    assert second == {"bar": 0, "newsitem": 0, "event": 0, "fundamental": 0}
    with Session(tmp_engine) as session:
        assert len(session.exec(select(NewsItemTable)).all()) == 1
        assert len(session.exec(select(FundamentalTable)).all()) == 1
        assert len(session.exec(select(EventTable)).all()) == 1
        assert len(session.exec(select(BarTable)).all()) == 1
