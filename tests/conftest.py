"""Shared test fixtures: FakeLLM replaces the model provider for offline tests."""

from __future__ import annotations

import json
from collections.abc import Callable
from datetime import UTC, datetime, timedelta
from pathlib import Path

import pytest
from sqlmodel import Session

from rigger.core.db import BarTable
from rigger.llm.client import LLMResult


class FakeConfig:
    """Minimal stand-in for AppConfig: only the fields strategies read."""

    def __init__(
        self,
        llm_routing: dict[str, str] | None = None,
        llm_ensemble_models: list[str] | None = None,
    ) -> None:
        self.llm_routing = llm_routing or {}
        self.llm_ensemble_models = llm_ensemble_models or []


def seed_bars(
    engine,
    instrument_id: str,
    n: int = 80,
    base: float = 100.0,
    start: datetime | None = None,
    price_fn: Callable[[int], float] | None = None,
) -> None:
    """Insert n daily bars ending today unless ``start`` is given.

    Prices rise 0.5/day from ``base`` unless ``price_fn(i)`` supplies them.
    """
    start = start or datetime.now(UTC) - timedelta(days=n)
    with Session(engine) as session:
        for i in range(n):
            price = price_fn(i) if price_fn else base + i * 0.5
            session.add(
                BarTable(
                    instrument_id=instrument_id,
                    ts=start + timedelta(days=i),
                    open=price,
                    high=price + 1,
                    low=price - 1,
                    close=price,
                    volume=1000.0,
                    source="test",
                )
            )
        session.commit()


class FakeLLM:
    """A duck-typed stand-in for LLMClient that returns canned JSON."""

    def __init__(self, response_by_task: dict[str, dict] | None = None, cost: float = 0.01) -> None:
        self.calls: list[dict] = []
        self._responses = response_by_task or {}
        self.cost = cost
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
            cost_usd=self.cost,
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
