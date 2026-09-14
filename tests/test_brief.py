"""Tests for the facts-only brief builder."""

from __future__ import annotations

from datetime import UTC, date, datetime, timedelta

from sqlmodel import Session

from rigger.brief import build_brief
from rigger.core.db import EventTable, FundamentalTable, NewsItemTable
from rigger.core.json import to_json
from rigger.core.models import Instrument
from rigger.core.plugin import Context
from tests.conftest import seed_bars

AS_OF = datetime(2026, 3, 22, tzinfo=UTC)
INST = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Technology")
OTHER = Instrument(id="US:MSFT", market="us", symbol="MSFT", currency="USD")

# Captured from the Phase 1 LLMAnalyst._build_brief on 80 bars starting at
# 100.0 and rising 0.5/day from 2026-01-01. The price section must not change.
PHASE1_PRICE_SECTION = (
    "Latest close: 139.50 USD\n"
    "20-day return: +7.72%\n"
    "Return over window: +39.50%\n"
    "20-day SMA: 134.75, 50-day SMA: 127.25\n"
    "60-day range: 110.00 - 139.50\n"
    "Sector: Technology"
)


def _seed_everything(engine) -> None:
    seed_bars(engine, INST.id, start=datetime(2026, 1, 1, tzinfo=UTC))
    with Session(engine) as session:
        session.add(
            NewsItemTable(
                id="news-1",
                instrument_ids=to_json([INST.id]),
                published=AS_OF - timedelta(days=2),
                title="Apple announces results",
                url="https://example.com/1",
                source="rss",
            )
        )
        session.add(  # other instrument, must be excluded
            NewsItemTable(
                id="news-2",
                instrument_ids=to_json([OTHER.id]),
                published=AS_OF - timedelta(days=1),
                title="Microsoft news",
                url="https://example.com/2",
                source="rss",
            )
        )
        session.add(  # too old, must be excluded
            NewsItemTable(
                id="news-3",
                instrument_ids=to_json([INST.id]),
                published=AS_OF - timedelta(days=30),
                title="Old news",
                url="https://example.com/3",
                source="rss",
            )
        )
        session.add(
            EventTable(
                id="event-past",
                instrument_id=INST.id,
                ts=AS_OF - timedelta(days=3),
                kind="earnings",
                summary="Reported EPS above consensus",
                sentiment=0.4,
                evidence_ids=to_json(["news-1"]),
                extracted_by="test/model",
                prompt_version="extract_v1",
            )
        )
        session.add(
            EventTable(
                id="event-future",
                instrument_id=INST.id,
                ts=AS_OF + timedelta(days=10),
                kind="dividend",
                summary="Ex-dividend 2026-04-01",
                sentiment=0.0,
                evidence_ids=to_json([]),
                extracted_by="yfinance",
                prompt_version="n/a",
            )
        )
        session.add(
            FundamentalTable(
                instrument_id=INST.id,
                as_of=date(2025, 12, 31),
                metric="eps",
                value=6.1,
                source="edgar",
            )
        )
        session.add(  # older value of same metric, must be superseded
            FundamentalTable(
                instrument_id=INST.id,
                as_of=date(2025, 9, 30),
                metric="eps",
                value=5.9,
                source="edgar",
            )
        )
        session.commit()


def test_price_section_matches_phase1(tmp_engine):
    seed_bars(tmp_engine, INST.id, start=datetime(2026, 1, 1, tzinfo=UTC))
    ctx = Context(engine=tmp_engine, settings=None, config=None, universe=[INST])
    brief = build_brief(ctx, INST, as_of=AS_OF)
    assert brief is not None
    assert brief.prices.render() == PHASE1_PRICE_SECTION
    assert len(brief.prices.evidence_ids) == 60
    assert brief.render() == (
        PHASE1_PRICE_SECTION
        + "\nFundamentals: none available"
        + "\nEvents: none available"
        + "\nUpcoming events: none available"
        + "\nNews: none available"
    )


def test_all_sections_render_with_evidence(tmp_engine):
    _seed_everything(tmp_engine)
    ctx = Context(engine=tmp_engine, settings=None, config=None, universe=[INST])
    brief = build_brief(ctx, INST, as_of=AS_OF)
    assert brief is not None

    assert brief.news.lines == ["2026-03-20 [rss] Apple announces results"]
    assert brief.news.evidence_ids == ["news-1"]

    assert brief.events.lines == [
        "2026-03-19 earnings: Reported EPS above consensus (sentiment +0.4)"
    ]
    assert brief.events.evidence_ids == ["event-past"]

    assert brief.calendar.lines == ["2026-04-01 dividend: Ex-dividend 2026-04-01"]
    assert brief.calendar.evidence_ids == ["event-future"]

    assert brief.fundamentals.lines == ["eps: 6.10 (as of 2025-12-31, edgar)"]
    assert len(brief.fundamentals.evidence_ids) == 1

    ids = brief.evidence_ids
    assert "news-1" in ids and "event-past" in ids and "event-future" in ids
    assert any(i.startswith("fundamental:") for i in ids)
    assert len(ids) == len(set(ids)), "evidence ids must be unique"

    text = brief.render()
    assert "none available" not in text
    assert text.startswith(PHASE1_PRICE_SECTION)


def test_no_bars_returns_none(tmp_engine):
    ctx = Context(engine=tmp_engine, settings=None, config=None, universe=[INST])
    assert build_brief(ctx, INST, as_of=AS_OF) is None
