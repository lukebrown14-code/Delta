"""Offline tests for the Jev Decisions client: parsing, caching, cost logging."""

from __future__ import annotations

import asyncio
import json

import httpx
import pytest
import respx
from sqlmodel import Session, select

from delta.core.db import LLMCallTable
from delta.llm.jev import DECISIONS_URL, JEV_MODEL, ChoiceQuestion, JevClient, JevError, Usage

QUESTION = ChoiceQuestion(
    key="stance",
    instructions="Bull or bear?",
    criteria={"bull": "Good.", "bear": "Bad.", "neutral": "Neither."},
)


def _body(answers: dict, cost: float = 0.0005) -> dict:
    return {
        "id": "gen-dec-1",
        "model": "typesafe/jev-1.13-20260917",
        "provider": "TypeSafe",
        "answers": answers,
        "usage": {"input_tokens": 100, "output_tokens": 10, "cost": cost},
    }


@respx.mock
def test_decide_parses_answers_and_logs_call(tmp_engine):
    route = respx.post(DECISIONS_URL).mock(
        return_value=httpx.Response(
            200, json=_body({"stance": {"type": "choice", "choice": "bull", "confidence": 0.8}})
        )
    )
    client = JevClient("k", tmp_engine)

    decision = asyncio.run(
        client.decide(task="sentiment", state={"title": "x"}, questions=[QUESTION])
    )

    assert decision.model == "typesafe/jev-1.13-20260917"
    assert decision.answers["stance"]["choice"] == "bull"
    assert decision.usage == Usage(input_tokens=100, output_tokens=10, cost_usd=0.0005)
    assert decision.cached is False
    assert route.calls.last.request.headers["Authorization"] == "Bearer k"
    sent = json.loads(route.calls.last.request.content)
    assert sent["model"] == JEV_MODEL
    assert sent["questions"]["stance"]["criteria"]["bear"] == "Bad."

    with Session(tmp_engine) as session:
        row = session.exec(select(LLMCallTable)).one()
    assert row.task == "sentiment"
    assert row.model == "typesafe/jev-1.13-20260917"
    assert row.prompt_version == "jev_v1"
    assert row.input_tokens == 100
    assert row.cost_usd == 0.0005
    assert row.cached is False
    assert json.loads(row.response or "{}")["answers"]["stance"]["choice"] == "bull"


@respx.mock
def test_decide_caches_identical_payload(tmp_engine):
    route = respx.post(DECISIONS_URL).mock(
        return_value=httpx.Response(
            200, json=_body({"stance": {"type": "choice", "choice": "bear"}})
        )
    )
    client = JevClient("k", tmp_engine)
    state = {"title": "same"}

    first = asyncio.run(client.decide(task="sentiment", state=state, questions=[QUESTION]))
    second = asyncio.run(client.decide(task="sentiment", state=state, questions=[QUESTION]))

    assert route.call_count == 1
    assert first.cached is False
    assert second.cached is True
    assert second.answers["stance"]["choice"] == "bear"
    assert second.usage.cost_usd == 0.0


@respx.mock
def test_decide_raises_on_api_error(tmp_engine):
    respx.post(DECISIONS_URL).mock(return_value=httpx.Response(502, json={"error": {}}))
    client = JevClient("k", tmp_engine)

    with pytest.raises(JevError, match="502"):
        asyncio.run(client.decide(task="sentiment", state={"title": "x"}, questions=[QUESTION]))
