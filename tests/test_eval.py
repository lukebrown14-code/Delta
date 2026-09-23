"""Offline tests for the citation-validity eval harness and the flows it scores.

The harness (``delta.llm.eval``) is pure; here it is exercised both directly on
synthetic citations and end-to-end through the grounded chat and report flows,
which already drop unsupported citations. All model calls go through FakeLLM.
"""

from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime

from sqlmodel import Session

from delta.chat import ChatMessage, chat
from delta.core.db import EventTable, NewsItemTable
from delta.core.json import to_json
from delta.evidence import evidence
from delta.llm.eval import (
    GoldenCase,
    citation_validity_rate,
    evaluate,
    hallucinated_citations,
    valid_citations,
)
from delta.reports import build_report, gather
from tests.conftest import FakeConfig, FakeLLM, seed_bars

INST = "US:AAPL"
ROUTING = {"chat": "test/chat-model"}

GOLDEN_IDS = {"bar:1", "bar:2", "news:news-1"}


class FakeChatLLM:
    """Offline LLMClient.chat returning a fixed ChatDraft payload."""

    def __init__(self, drafts: list[dict]) -> None:
        self.drafts = list(drafts)
        self.calls: list[dict] = []

    async def chat(self, **kwargs):
        self.calls.append(kwargs)
        payload = self.drafts.pop(0)
        from delta.llm.client import LLMResult

        return LLMResult(
            text=json.dumps(payload),
            call_id=f"fake-{len(self.calls)}",
            cost_usd=0.0,
            cached=False,
        )


class FakeRig:
    def __init__(self, engine, llm, routing):
        self.engine = engine
        self.llm = llm
        self.cfg = FakeConfig(llm_routing=routing)


def _history(*texts: str) -> list[ChatMessage]:
    return [ChatMessage(role="user", text=text, source="user") for text in texts]


def test_valid_citations_and_hallucinations_split_correctly():
    assert valid_citations(["bar:1", "bogus:9", "bar:2", "bar:2"], GOLDEN_IDS) == ["bar:1", "bar:2"]
    assert hallucinated_citations(["bar:1", "bogus:9", "ghost:1", "bogus:9"], GOLDEN_IDS) == [
        "bogus:9",
        "ghost:1",
    ]


def test_citation_validity_rate_ranges_and_empty_is_perfect():
    assert citation_validity_rate(["bar:1", "bar:2"], GOLDEN_IDS) == 1.0
    assert citation_validity_rate(["bar:1", "bogus:9"], GOLDEN_IDS) == 0.5
    assert citation_validity_rate([], GOLDEN_IDS) == 1.0


def test_evaluate_flags_hallucinations_against_golden_set():
    case = GoldenCase(name="sample", evidence_ids=GOLDEN_IDS, min_validity=1.0)
    result = evaluate(case, ["bar:1", "bogus:9"])
    assert result.rate == 0.5
    assert result.hallucinated == ["bogus:9"]
    assert result.passed is False


def test_eval_harness_asserts_minimum_validity():
    case = GoldenCase(name="strict", evidence_ids=GOLDEN_IDS, min_validity=1.0)
    evaluate(case, ["bar:1", "bar:2"]).assert_valid()
    try:
        evaluate(case, ["bar:1", "bogus:9"]).assert_valid()
    except AssertionError as exc:
        assert "50.00%" in str(exc)
    else:  # pragma: no cover
        raise AssertionError("expected AssertionError for a sub-threshold run")


def test_chat_flow_reaches_full_golden_validity(tmp_engine):
    """The grounded chat drops unmatched citations, so a golden run scores 1.0."""
    seed_bars(tmp_engine, INST, n=3)
    llm = FakeChatLLM([{"answer": "AAPL closed at 100.", "citations": ["bar:1", "bogus:9"]}])
    reply = asyncio.run(
        chat(FakeRig(tmp_engine, llm, ROUTING), _history("How did AAPL do?"), targets=[INST])
    )

    valid = {item.id for item in evidence(tmp_engine)}
    case = GoldenCase(name="chat-golden", evidence_ids=valid, min_validity=1.0)
    result = evaluate(case, reply.citations)
    assert result.rate == 1.0
    assert result.hallucinated == []
    assert result.passed


def test_report_flow_drops_unsupported_and_scores_perfect(tmp_engine, tmp_path, monkeypatch):
    """The report keeps only gathered claims, so a golden run scores 1.0."""
    monkeypatch.chdir(tmp_path)
    seed_bars(tmp_engine, INST, n=2, start=datetime(2026, 3, 18, tzinfo=UTC))
    with Session(tmp_engine) as session:
        session.add(
            NewsItemTable(
                id="news-1",
                instrument_ids=to_json([INST]),
                published=datetime(2026, 3, 20, 12, 0, tzinfo=UTC),
                title="Apple and Microsoft sign cloud deal",
                url="https://example.com/news-1",
                body="Both companies announced a partnership.",
                source="rss",
            )
        )
        session.add(
            EventTable(
                id="event-1",
                instrument_id=INST,
                ts=datetime(2026, 3, 19, 12, 0, tzinfo=UTC),
                kind="earnings",
                summary="Reported EPS above consensus",
                sentiment=0.4,
                evidence_ids=to_json(["news-1"]),
                extracted_by="test/model",
                prompt_version="extract_v1",
            )
        )
        session.commit()

    draft = {
        "summary": "Signed a partnership.",
        "bull": [
            {"text": "Partnership announced.", "evidence_ids": ["news:news-1"]},
            {"text": "Hallucinated claim.", "evidence_ids": ["news:ghost"]},
        ],
        "bear": [],
        "risks": [],
        "catalysts": [],
        "unknowns": [],
        "sentiment": 0.5,
        "sentiment_reasons": [],
    }
    llm = FakeLLM({"report": draft})
    report = asyncio.run(build_report(FakeRig(tmp_engine, llm, {"report": "test/model"}), INST))

    gathered = {item.id for item in gather(INST, tmp_engine)}
    citations = [cid for claim in report.bull for cid in claim.evidence_ids]
    case = GoldenCase(name="report-golden", evidence_ids=gathered, min_validity=1.0)
    result = evaluate(case, citations)
    assert result.rate == 1.0
    assert result.hallucinated == []
    assert result.passed