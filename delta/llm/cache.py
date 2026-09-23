"""Shared cache and logging primitives for the LLM layer.

Both :class:`delta.llm.client.LLMClient` (chat completions) and
:class:`delta.llm.jev.JevClient` (typed decisions) persist every call to the
``llmcall`` table and replay cached responses by prompt hash. These helpers
are the one place that SQL lives, so the two clients cannot drift apart on
how a cache hit or a logged call is represented.
"""

from __future__ import annotations

import uuid
from datetime import UTC, datetime

from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from delta.core.db import LLMCallTable


def lookup_cache(engine: Engine, prompt_hash: str) -> LLMCallTable | None:
    """The first cached row for ``prompt_hash``, or None when absent or empty.

    A row with no ``response`` is not a usable cache entry (it was a failed or
    partial persist), so it is treated as a miss.
    """
    with Session(engine) as session:
        row = session.exec(
            select(LLMCallTable).where(LLMCallTable.prompt_hash == prompt_hash)
        ).first()
        if row is not None and row.response is not None:
            return row
    return None


def store_call(
    engine: Engine,
    *,
    task: str,
    model: str,
    prompt_version: str,
    prompt_hash: str,
    input_tokens: int,
    output_tokens: int,
    cost_usd: float,
    latency_ms: int,
    cached: bool,
    response: str | None,
) -> None:
    """Persist one ``llmcall`` row: a live call or a replayed cache hit."""
    with Session(engine) as session:
        session.add(
            LLMCallTable(
                id=uuid.uuid4().hex,
                ts=datetime.now(UTC),
                task=task,
                model=model,
                prompt_version=prompt_version,
                prompt_hash=prompt_hash,
                input_tokens=input_tokens,
                output_tokens=output_tokens,
                cost_usd=cost_usd,
                latency_ms=latency_ms,
                cached=cached,
                response=response,
            )
        )
        session.commit()