"""TypeSafe Jev decision-model client over OpenRouter's Decisions API.

Jev is not a chat-completions model: it answers typed questions about a state
and returns probabilities instead of text, so it must not go through
:class:`delta.llm.client.LLMClient` or ``structured.py``. This client owns the
same guarantees the chat client provides elsewhere — responses cached by
payload hash, every call persisted to ``llmcall`` with tokens, cost and
latency — against the Decisions endpoint.
"""

from __future__ import annotations

import json
import time
import uuid
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

import httpx
from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from delta.core.db import LLMCallTable
from delta.core.ids import stable_id

DECISIONS_URL = "https://openrouter.ai/api/alpha/decisions"
JEV_MODEL = "typesafe/jev-1.13"
PROMPT_VERSION = "jev_v1"
REQUEST_TIMEOUT = 30.0


@dataclass(frozen=True)
class ChoiceQuestion:
    """One typed question: which of these options fits the state?"""

    key: str
    instructions: str
    criteria: dict[str, str]

    def payload(self) -> dict[str, Any]:
        return {
            "type": "choice",
            "instructions": self.instructions,
            "criteria": self.criteria,
        }


@dataclass(frozen=True)
class Usage:
    input_tokens: int
    output_tokens: int
    cost_usd: float


@dataclass(frozen=True)
class Decision:
    """One Decisions response: typed answers plus what the call cost."""

    model: str
    answers: dict[str, Any]
    usage: Usage
    cached: bool


class JevError(RuntimeError):
    """A Decisions request failed at the API."""


def _payload_hash(model: str, state: Any, questions: dict[str, dict[str, Any]]) -> str:
    body = json.dumps(
        {"model": model, "prompt_version": PROMPT_VERSION, "state": state, "questions": questions},
        sort_keys=True,
    )
    return stable_id(body)


def _usage_of(data: dict[str, Any]) -> Usage:
    raw = data.get("usage") or {}
    return Usage(
        input_tokens=int(raw.get("input_tokens", 0)),
        output_tokens=int(raw.get("output_tokens", 0)),
        cost_usd=float(raw.get("cost", 0.0)),
    )


class JevClient:
    """One OpenRouter key, one engine: cached typed decisions, logged cost."""

    def __init__(self, api_key: str, engine: Engine, *, timeout: float = REQUEST_TIMEOUT) -> None:
        self.api_key = api_key
        self.engine = engine
        self._timeout = timeout

    async def decide(
        self,
        *,
        task: str,
        state: Any,
        questions: list[ChoiceQuestion],
        model: str = JEV_MODEL,
    ) -> Decision:
        """Ask Jev one batch of independent questions about one state."""
        payload_questions = {q.key: q.payload() for q in questions}
        phash = _payload_hash(model, state, payload_questions)
        cached = self._lookup(phash)
        if cached is not None:
            return cached
        started = time.perf_counter()
        async with httpx.AsyncClient(timeout=self._timeout) as client:
            resp = await client.post(
                DECISIONS_URL,
                headers={"Authorization": f"Bearer {self.api_key}"},
                json={"model": model, "state": state, "questions": payload_questions},
            )
        latency_ms = int((time.perf_counter() - started) * 1000)
        if resp.status_code != 200:
            raise JevError(f"jev decisions request failed ({resp.status_code}): {resp.text[:200]}")
        data: dict[str, Any] = resp.json()
        decision = Decision(
            model=str(data.get("model", model)),
            answers=dict(data.get("answers") or {}),
            usage=_usage_of(data),
            cached=False,
        )
        self._store(task, phash, latency_ms, decision, data)
        return decision

    def _lookup(self, phash: str) -> Decision | None:
        """Cached decision for a payload hash, or None; a bad body is a miss."""
        with Session(self.engine) as session:
            row = session.exec(
                select(LLMCallTable).where(LLMCallTable.prompt_hash == phash)
            ).first()
            if row is None or row.response is None:
                return None
            try:
                data: dict[str, Any] = json.loads(row.response)
            except json.JSONDecodeError:
                return None
        return Decision(
            model=str(data.get("model", row.model)),
            answers=dict(data.get("answers") or {}),
            usage=Usage(0, 0, 0.0),
            cached=True,
        )

    def _store(
        self,
        task: str,
        phash: str,
        latency_ms: int,
        decision: Decision,
        raw: dict[str, Any],
    ) -> None:
        with Session(self.engine) as session:
            session.add(
                LLMCallTable(
                    id=uuid.uuid4().hex,
                    ts=datetime.now(UTC),
                    task=task,
                    model=decision.model,
                    prompt_version=PROMPT_VERSION,
                    prompt_hash=phash,
                    input_tokens=decision.usage.input_tokens,
                    output_tokens=decision.usage.output_tokens,
                    cost_usd=decision.usage.cost_usd,
                    latency_ms=latency_ms,
                    cached=False,
                    response=json.dumps(raw),
                )
            )
            session.commit()
