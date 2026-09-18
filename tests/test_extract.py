"""Tests for news -> Event extraction."""

from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime, timedelta

from sqlmodel import Session, select

from delta.core.db import EventTable, NewsItemTable
from delta.core.json import from_json, to_json
from delta.core.models import Instrument
from delta.core.plugin import Context
from delta.extract import event_id, extract_events
from delta.llm.client import LLMResult
from tests.conftest import FakeConfig, FakeLLM

NOW = datetime(2026, 3, 22, tzinfo=UTC)
SINCE = NOW - timedelta(days=14)
AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")
MSFT = Instrument(id="US:MSFT", market="us", symbol="MSFT", currency="USD")


def _ctx(engine, llm) -> Context:
    return Context(
        engine=engine,
        settings=None,
        config=FakeConfig({"extract": "fake/extract-model"}),
        llm=llm,
        universe=[AAPL, MSFT],
    )


def _seed_news(engine, rows: list[tuple[str, list[str], int]]) -> None:
    """rows: (id, instrument_ids, days_ago)."""
    with Session(engine) as session:
        for nid, inst_ids, days_ago in rows:
            session.add(
                NewsItemTable(
                    id=nid,
                    instrument_ids=to_json(inst_ids),
                    published=NOW - timedelta(days=days_ago),
                    title=f"Title {nid}",
                    url=f"https://example.com/{nid}",
                    body=f"Body {nid}",
                    source="rss",
                )
            )
        session.commit()


def _canned(kind: str, summary: str, evidence: list[str], sentiment: float = 0.5) -> dict:
    return {"kind": kind, "summary": summary, "sentiment": sentiment, "evidence_ids": evidence}


def _event_rows(engine) -> list[EventTable]:
    with Session(engine) as session:
        return list(session.exec(select(EventTable)).all())


def test_events_are_stored_with_provenance(tmp_engine):
    _seed_news(tmp_engine, [("n1", [AAPL.id], 2), ("n2", [AAPL.id], 1)])
    llm = FakeLLM(
        {
            "extract": {
                "events": [
                    _canned("earnings", "Apple reported quarterly revenue of $90bn.", ["n1", "n2"])
                ]
            }
        }
    )

    events = asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE))

    assert len(events) == 1
    ev = events[0]
    assert ev.instrument_id == AAPL.id
    assert ev.kind == "earnings"
    assert ev.evidence_ids == ["n1", "n2"]
    assert ev.extracted_by == "fake/extract-model"
    assert ev.prompt_version == "extract_v1"
    assert ev.id == event_id(AAPL.id, "earnings", "Apple reported quarterly revenue of $90bn.")
    # ts is the latest published among the evidence (n2, one day ago).
    assert ev.ts == NOW - timedelta(days=1)

    rows = _event_rows(tmp_engine)
    assert len(rows) == 1
    assert from_json(rows[0].evidence_ids) == ["n1", "n2"]

    # One call per instrument batch, routed to the extract task with facts-only prompt.
    assert len(llm.calls) == 1
    call = llm.calls[0]
    assert call["task"] == "extract"
    assert call["model"] == "fake/extract-model"
    assert call["prompt_version"] == "extract_v1"
    assert "Do not rely on prior knowledge of prices, news or events." in call["prompt"]
    assert "Title n1" in call["prompt"] and "Title n2" in call["prompt"]


def test_covered_items_are_not_resent(tmp_engine):
    _seed_news(tmp_engine, [("n1", [AAPL.id], 3), ("n2", [AAPL.id], 1), ("old", [AAPL.id], 40)])
    with Session(tmp_engine) as session:
        session.add(
            EventTable(
                id="existing",
                instrument_id=AAPL.id,
                ts=NOW - timedelta(days=3),
                kind="other",
                summary="Already extracted.",
                sentiment=0.0,
                evidence_ids=to_json(["n1"]),
                extracted_by="fake",
                prompt_version="extract_v1",
            )
        )
        session.commit()
    llm = FakeLLM({"extract": {"events": []}})

    events = asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE))

    assert events == []
    assert len(llm.calls) == 1
    prompt = llm.calls[0]["prompt"]
    assert "Item n2" in prompt
    assert "Item n1" not in prompt  # already cited by an event
    assert "Item old" not in prompt  # before `since`


def test_unmapped_items_are_skipped(tmp_engine):
    _seed_news(tmp_engine, [("macro", [], 1)])
    llm = FakeLLM({"extract": {"events": []}})

    events = asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE))

    assert events == []
    assert llm.calls == []


def test_items_with_several_instruments_go_to_each(tmp_engine):
    _seed_news(tmp_engine, [("shared", [AAPL.id, MSFT.id], 1)])
    llm = FakeLLM({"extract": {"events": [_canned("m&a", "A deal was announced.", ["shared"])]}})

    events = asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE))

    assert len(llm.calls) == 2
    assert {e.instrument_id for e in events} == {AAPL.id, MSFT.id}
    assert len(_event_rows(tmp_engine)) == 2


def test_invalid_output_is_skipped(tmp_engine):
    _seed_news(tmp_engine, [("n1", [AAPL.id], 1), ("m1", [MSFT.id], 1)])

    class MixedLLM(FakeLLM):
        async def complete(self, **kwargs) -> LLMResult:
            self.calls.append(kwargs)
            if "US:AAPL" in kwargs["prompt"]:
                payload: dict = {"events": [{"kind": "not-a-kind", "summary": 1}]}
            else:
                payload = {"events": [_canned("guidance", "Microsoft raised guidance.", ["m1"])]}
            return LLMResult(
                text=json.dumps(payload),
                call_id=f"fake-{len(self.calls)}",
                cost_usd=0,
                cached=False,
            )

    llm = MixedLLM()
    events = asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE))

    # AAPL batch failed validation (and its one retry); MSFT still stored.
    assert [e.instrument_id for e in events] == [MSFT.id]
    assert len(_event_rows(tmp_engine)) == 1
    # structured() re-prompts once on failure: AAPL x2 + MSFT x1.
    assert len(llm.calls) == 3


def test_dedup_by_id(tmp_engine):
    _seed_news(tmp_engine, [("n1", [AAPL.id], 2), ("n2", [AAPL.id], 1)])
    duplicate = _canned("dividend", "Apple declared a $0.25 dividend.", ["n1"])
    llm = FakeLLM({"extract": {"events": [duplicate, dict(duplicate, evidence_ids=["n2"])]}})

    first = asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE))
    assert len(first) == 1  # same instrument + kind + summary within one batch

    # Add a fresh, uncovered item and run again: the model repeats the same event.
    _seed_news(tmp_engine, [("n3", [AAPL.id], 0)])
    second = asyncio.run(
        extract_events(_ctx(tmp_engine, FakeLLM({"extract": {"events": [duplicate]}})), SINCE)
    )
    assert second == []  # id already in EventTable
    assert len(_event_rows(tmp_engine)) == 1


def test_events_citing_no_provided_items_are_dropped(tmp_engine):
    _seed_news(tmp_engine, [("n1", [AAPL.id], 1)])
    llm = FakeLLM({"extract": {"events": [_canned("other", "Hallucinated.", ["nope"])]}})

    events = asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE))

    assert events == []
    assert _event_rows(tmp_engine) == []


def test_batches_respect_batch_size(tmp_engine):
    _seed_news(tmp_engine, [(f"n{i}", [AAPL.id], 1) for i in range(5)])
    llm = FakeLLM({"extract": {"events": []}})

    asyncio.run(extract_events(_ctx(tmp_engine, llm), SINCE, batch_size=2))

    assert len(llm.calls) == 3
