"""Shared test fixtures: FakeLLM replaces the model provider for offline tests."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from rigger.llm.client import LLMResult


class FakeLLM:
    """A duck-typed stand-in for LLMClient that returns canned JSON."""

    def __init__(self, response_by_task: dict[str, dict] | None = None) -> None:
        self.calls: list[dict] = []
        self._responses = response_by_task or {}
        self._default = {
            "direction": "long",
            "conviction": 0.75,
            "horizon_days": 20,
            "thesis": "The 20-day return is positive and momentum is favourable.",
            "invalidation": "A break below the 50-day SMA would invalidate this thesis.",
        }

    async def complete(self, **kwargs) -> LLMResult:
        self.calls.append(kwargs)
        task = kwargs.get("task", "analyse")
        payload = self._responses.get(task, self._default)
        return LLMResult(
            text=json.dumps(payload),
            call_id=f"fake-{len(self.calls)}",
            cost_usd=0.01,
            cached=False,
        )


@pytest.fixture
def fake_llm() -> FakeLLM:
    return FakeLLM()


@pytest.fixture
def tmp_engine(tmp_path: Path):
    from rigger.core.db import ensure_cash, init_engine

    engine = init_engine(tmp_path / "test.db")
    ensure_cash(engine, "AUD", 100_000.0)
    return engine
