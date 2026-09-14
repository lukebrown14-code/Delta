"""Critic strategy tests: wraps llm_analyst and revises each signal offline."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

import pytest
from conftest import FakeLLM
from sqlmodel import Session

from rigger.core.db import BarTable, LLMCallTable
from rigger.core.models import Instrument
from rigger.core.plugin import Context
from rigger.llm.client import LLMResult
from rigger.plugins.strategies.critic import Critic
from rigger.plugins.strategies.llm_analyst import LLMAnalyst

ANALYSE = {
    "direction": "long",
    "conviction": 0.8,
    "horizon_days": 20,
    "thesis": "Positive 20-day return.",
    "invalidation": "Close below the 50-day SMA.",
}
CRITIQUE_REDUCE = {
    "counter_argument": "The 20-day return alone is thin evidence.",
    "risks": ["Momentum reversal", "No fundamentals in brief"],
    "revised_conviction": 0.45,
    "verdict": "reduce",
}
CRITIQUE_REJECT = {**CRITIQUE_REDUCE, "revised_conviction": 0.1, "verdict": "reject"}


class RecordingFakeLLM(FakeLLM):
    """FakeLLM that also logs each call to the llmcall table like the real client."""

    def __init__(self, engine, responses: dict[str, dict], cost: float) -> None:
        super().__init__(responses)
        self._engine = engine
        self._cost = cost

    async def complete(self, **kwargs) -> LLMResult:
        result = await super().complete(**kwargs)
        with Session(self._engine) as session:
            session.add(
                LLMCallTable(
                    id=result.call_id,
                    ts=datetime.now(UTC),
                    task=kwargs["task"],
                    model=kwargs["model"],
                    prompt_version=kwargs["prompt_version"],
                    prompt_hash="x",
                    input_tokens=1,
                    output_tokens=1,
                    cost_usd=self._cost,
                    latency_ms=1,
                    cached=False,
                )
            )
            session.commit()
        return LLMResult(
            text=result.text, call_id=result.call_id, cost_usd=self._cost, cached=False
        )


def _seed_bars(engine, instrument_id: str, n: int = 80) -> None:
    with Session(engine) as session:
        start = datetime.now(UTC) - timedelta(days=n)
        for i in range(n):
            price = 100.0 + i * 0.5
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


class _Cfg:
    llm_routing = {"analyse": "test/analyst", "critique": "test/critic"}


def _instruments() -> list[Instrument]:
    return [
        Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD"),
        Instrument(id="US:MSFT", market="us", symbol="MSFT", currency="USD"),
    ]


def _ctx(engine, llm) -> Context:
    universe = _instruments()
    for inst in universe:
        _seed_bars(engine, inst.id)
    analyst = LLMAnalyst()
    critic = Critic()
    critic.configure({"enabled": True, "wraps": "llm_analyst"})
    return Context(
        engine=engine,
        settings=None,
        config=_Cfg(),
        llm=llm,
        universe=universe,
        plugins={"llm_analyst": analyst, "critic": critic},
    )


def test_critic_returns_base_and_revised_signal(tmp_engine):
    llm = FakeLLM({"analyse": ANALYSE, "critique": CRITIQUE_REDUCE})
    ctx = _ctx(tmp_engine, llm)

    signals = asyncio.run(ctx.plugins["critic"].generate(ctx))  # type: ignore[attr-defined]

    assert len(signals) == 4  # two per instrument
    by_inst: dict[str, list] = {}
    for s in signals:
        by_inst.setdefault(s.instrument_id, []).append(s)
    for pair in by_inst.values():
        base = next(s for s in pair if s.strategy == "llm_analyst")
        crit = next(s for s in pair if s.strategy == "critic:llm_analyst")
        assert crit.id != base.id
        assert crit.direction == "long"
        assert crit.conviction == 0.45
        assert crit.horizon_days == base.horizon_days
        assert crit.thesis == base.thesis and crit.invalidation == base.invalidation
        assert crit.evidence_ids == base.evidence_ids
        assert crit.model == "test/critic"
        assert crit.prompt_version == "critic_v1"
        meta = crit.metadata["critic"]
        assert meta == {
            "base_signal_id": base.id,
            "original_conviction": 0.8,
            "counter_argument": CRITIQUE_REDUCE["counter_argument"],
            "risks": CRITIQUE_REDUCE["risks"],
            "verdict": "reduce",
            "model": "test/critic",
            "prompt_version": "critic_v1",
        }

    tasks = [c["task"] for c in llm.calls]
    assert tasks.count("analyse") == 2 and tasks.count("critique") == 2
    critique_call = next(c for c in llm.calls if c["task"] == "critique")
    assert critique_call["model"] == "test/critic"
    assert critique_call["prompt_version"] == "critic_v1"
    prompt = critique_call["prompt"]
    assert "Use only the information provided" in prompt
    assert ANALYSE["thesis"] in prompt and ANALYSE["invalidation"] in prompt
    assert "Latest close" in prompt  # the brief was rendered into the prompt


def test_reject_verdict_flattens_direction(tmp_engine):
    llm = FakeLLM({"analyse": ANALYSE, "critique": CRITIQUE_REJECT})
    ctx = _ctx(tmp_engine, llm)

    signals = asyncio.run(ctx.plugins["critic"].generate(ctx))  # type: ignore[attr-defined]
    crits = [s for s in signals if s.strategy.startswith("critic:")]
    bases = [s for s in signals if s.strategy == "llm_analyst"]

    assert len(crits) == 2
    assert all(s.direction == "flat" for s in crits)
    assert all(s.conviction == 0.1 for s in crits)
    assert all(s.metadata["critic"]["verdict"] == "reject" for s in crits)
    # The base signal is untouched.
    assert all(s.direction == "long" and s.conviction == 0.8 for s in bases)


def test_cost_is_base_plus_critique(tmp_engine):
    llm = RecordingFakeLLM(tmp_engine, {"analyse": ANALYSE, "critique": CRITIQUE_REDUCE}, 0.02)
    ctx = _ctx(tmp_engine, llm)
    ctx.universe = ctx.universe[:1]

    signals = asyncio.run(ctx.plugins["critic"].generate(ctx))  # type: ignore[attr-defined]
    base = next(s for s in signals if s.strategy == "llm_analyst")
    crit = next(s for s in signals if s.strategy == "critic:llm_analyst")

    # The analyst does not record cost on its signal; the critic adds its own logged call.
    assert crit.cost_usd == pytest.approx((base.cost_usd or 0.0) + 0.02)


def test_missing_wrapped_plugin_raises(tmp_engine):
    llm = FakeLLM({"analyse": ANALYSE, "critique": CRITIQUE_REDUCE})
    ctx = _ctx(tmp_engine, llm)
    ctx.plugins.pop("llm_analyst")

    with pytest.raises(LookupError, match="wraps 'llm_analyst' but no such plugin"):
        asyncio.run(ctx.plugins["critic"].generate(ctx))  # type: ignore[attr-defined]


def test_wrapping_non_strategy_raises(tmp_engine):
    llm = FakeLLM({"analyse": ANALYSE, "critique": CRITIQUE_REDUCE})
    ctx = _ctx(tmp_engine, llm)
    critic = Critic()
    critic.configure({"wraps": "critic"})  # a plugin that exists but is not what we expect
    ctx.plugins["critic"] = object()  # type: ignore[assignment]

    with pytest.raises(TypeError, match="not a strategy plugin"):
        asyncio.run(critic.generate(ctx))
