"""Offline tests for Jev news-stance classification and aggregation."""

from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime, timedelta
from types import SimpleNamespace

import httpx
import pytest
import respx
from sqlmodel import Session, select

from delta.core.db import NewsItemTable, SentimentTable, init_engine
from delta.core.json import to_json
from delta.llm.jev import DECISIONS_URL
from delta.sentiment import HALF_LIFE_DAYS, classify_news, stance_of, stock_sentiment
from tests.conftest import FakeConfig

AAPL = SimpleNamespace(id="US:AAPL", symbol="AAPL", market="us")
MSFT = SimpleNamespace(id="US:MSFT", symbol="MSFT", market="us")
NOW = datetime.now(UTC)


def _delta(engine) -> SimpleNamespace:
    return SimpleNamespace(
        engine=engine,
        cfg=FakeConfig({"sentiment": "typesafe/jev-1.13"}),
        settings=SimpleNamespace(openrouter_api_key="k"),
        universe=lambda: [AAPL, MSFT],
    )


def _seed_news(engine, *, good=True, instrument_ids=("US:AAPL",), published=None):
    title = "Apple upgrades guidance on strong demand" if good else "Apple cuts guidance on weak demand"
    with Session(engine) as session:
        session.add(
            NewsItemTable(
                id=f"news-{title[:20]}",
                instrument_ids=to_json(list(instrument_ids)),
                published=published or NOW,
                title=title,
                url="https://example.com/a",
                body="body text",
                source="rss",
            )
        )
        session.commit()


def _stance_answer(choice: str, confidence: float = 0.8) -> dict:
    return {
        "stance": {
            "type": "choice",
            "choice": choice,
            "confidence": confidence,
            "probabilities": {"bull": 0.5, "bear": 0.3, "neutral": 0.2},
        }
    }


def _mock_by_title(monkeypatch_titles=True):
    """Answer bull for upbeat titles, bear otherwise, per request state."""

    def handler(request: httpx.Request) -> httpx.Response:
        body = json.loads(request.content)
        choice = "bull" if "upgrades" in body["state"]["title"] else "bear"
        return httpx.Response(200, json={"model": "typesafe/jev-1.13-20260917", "answers": _stance_answer(choice), "usage": {"input_tokens": 50, "output_tokens": 5, "cost": 0.0001}})

    return respx.post(DECISIONS_URL).mock(side_effect=handler)


def _rows(engine) -> list[SentimentTable]:
    with Session(engine) as session:
        return list(session.exec(select(SentimentTable)).all())


@respx.mock
def test_classify_news_stores_stance_per_instrument(tmp_path):
    engine = init_engine(tmp_path / "t.db")
    _seed_news(engine, good=True)
    _seed_news(engine, good=False, instrument_ids=("US:AAPL", "US:MSFT"))
    route = _mock_by_title()

    stored = asyncio.run(classify_news(_delta(engine)))

    assert route.call_count == 3
    assert len(stored) == 3
    rows = {(r.instrument_id, r.evidence_id): r for r in _rows(engine)}
    assert len(rows) == 3
    good = next(r for r in rows.values() if "upgrades" in r.evidence_id)
    assert good.stance == "bull"
    assert good.confidence == 0.8
    assert json.loads(good.probabilities)["bull"] == 0.5
    assert good.model == "typesafe/jev-1.13-20260917"
    both = [r for r in rows.values() if "cuts" in r.evidence_id]
    assert {r.instrument_id for r in both} == {"US:AAPL", "US:MSFT"}
    assert all(r.stance == "bear" for r in both)


@respx.mock
def test_classify_news_is_idempotent(tmp_path):
    engine = init_engine(tmp_path / "t.db")
    _seed_news(engine, good=True)
    route = _mock_by_title()
    delta = _delta(engine)

    asyncio.run(classify_news(delta))
    again = asyncio.run(classify_news(delta))

    assert route.call_count == 1
    assert again == []


@respx.mock
def test_classify_news_skips_failed_requests(tmp_path):
    engine = init_engine(tmp_path / "t.db")
    _seed_news(engine, good=True, published=NOW - timedelta(days=1))
    _seed_news(engine, good=False, published=NOW)
    responses = iter(
        [
            httpx.Response(502, json={"error": {}}),
            httpx.Response(200, json={"model": "m", "answers": _stance_answer("neutral"), "usage": {"input_tokens": 1, "output_tokens": 1, "cost": 0.0}}),
        ]
    )
    respx.post(DECISIONS_URL).mock(side_effect=lambda request: next(responses))

    stored = asyncio.run(classify_news(_delta(engine)))

    assert len(stored) == 1
    assert stored[0].stance == "neutral"


@respx.mock
def test_classify_news_maps_unknown_choice_to_neutral(tmp_path):
    engine = init_engine(tmp_path / "t.db")
    _seed_news(engine, good=True)
    respx.post(DECISIONS_URL).mock(
        return_value=httpx.Response(
            200,
            json={"model": "m", "answers": _stance_answer("sideways", confidence=1.5), "usage": {}},
        )
    )

    stored = asyncio.run(classify_news(_delta(engine)))

    row = stored[0]
    assert row.stance == "neutral"
    assert row.confidence == 1.0


@respx.mock
def test_services_classify_sentiment_counts_and_logs(tmp_path):
    from delta import services

    engine = init_engine(tmp_path / "t.db")
    _seed_news(engine, good=True)
    _mock_by_title()
    lines: list[str] = []

    result = asyncio.run(
        services.classify_sentiment(_delta(engine), log=lines.append)
    )

    assert result.classified == 1
    assert result.instruments == 1
    assert any("Classified 1" in line for line in lines)


def test_stance_of_handles_bad_answers():
    assert stance_of({"choice": "bull"}) == "bull"
    assert stance_of({"choice": "sideways"}) == "neutral"
    assert stance_of({}) == "neutral"
    assert stance_of(None) == "neutral"
    assert stance_of("bull") == "neutral"


def test_stock_sentiment_weights_recency_and_confidence(tmp_path):
    engine = init_engine(tmp_path / "t.db")
    fresh = NOW - timedelta(hours=1)
    week = NOW - timedelta(days=HALF_LIFE_DAYS)
    with Session(engine) as session:
        session.add(
            SentimentTable(
                id="j1",
                instrument_id="US:AAPL",
                evidence_id="n1",
                ts=fresh,
                stance="bull",
                confidence=1.0,
                probabilities="{}",
                model="m",
                prompt_version="jev_v1",
            )
        )
        session.add(
            SentimentTable(
                id="j2",
                instrument_id="US:AAPL",
                evidence_id="n2",
                ts=week,
                stance="bear",
                confidence=1.0,
                probabilities="{}",
                model="m",
                prompt_version="jev_v1",
            )
        )
        session.add(
            SentimentTable(
                id="j3",
                instrument_id="US:MSFT",
                evidence_id="n3",
                ts=fresh,
                stance="bear",
                confidence=1.0,
                probabilities="{}",
                model="m",
                prompt_version="jev_v1",
            )
        )
        session.commit()

    summary = stock_sentiment(engine, "US:AAPL", days=8, now=NOW)

    assert summary is not None
    assert summary.bull == 1
    assert summary.bear == 1
    assert summary.neutral == 0
    # Fresh bull weighs ~1.0; a one-half-life-old bear weighs ~0.5.
    assert summary.score == pytest.approx((1.0 - 0.5) / (1.0 + 0.5), abs=0.01)

    short = stock_sentiment(engine, "US:AAPL", days=3, now=NOW)
    assert short is not None
    assert short.bear == 0
    assert short.score == 1.0

    assert stock_sentiment(engine, "US:NOPE", now=NOW) is None


def test_stock_sentiment_zero_confidence_scores_neutral(tmp_path):
    engine = init_engine(tmp_path / "t.db")
    with Session(engine) as session:
        session.add(
            SentimentTable(
                id="j1",
                instrument_id="US:AAPL",
                evidence_id="n1",
                ts=NOW,
                stance="bull",
                confidence=0.0,
                probabilities="{}",
                model="m",
                prompt_version="jev_v1",
            )
        )
        session.commit()

    summary = stock_sentiment(engine, "US:AAPL", now=NOW)

    assert summary is not None
    assert summary.score == 0.0
    assert summary.bull == 1
