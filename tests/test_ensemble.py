"""Ensemble strategy tests: per-model answers combined offline via FakeLLM."""

from __future__ import annotations

import asyncio
import json
import statistics

import pytest

from rigger.core.models import Instrument
from rigger.core.plugin import Context
from rigger.llm.client import LLMResult
from rigger.plugins.strategies.ensemble import Ensemble
from tests.conftest import FakeLLM
from tests.test_pipeline import _seed_bars

MODELS = ["m/a", "m/b", "m/c"]


def _answer(direction: str, conviction: float, horizon: int = 20, tag: str = "") -> dict:
    return {
        "direction": direction,
        "conviction": conviction,
        "horizon_days": horizon,
        "thesis": f"thesis {tag or direction}",
        "invalidation": f"invalidation {tag or direction}",
    }


class PerModelLLM(FakeLLM):
    """FakeLLM keyed on the ``model`` kwarg instead of ``task``.

    A value of ``None`` returns unparseable text so the model's answer fails
    validation. Every call blocks until ``expected`` calls are in flight, so
    the test deadlocks (times out) unless the strategy runs them concurrently.
    """

    def __init__(self, by_model: dict[str, dict | None], expected: int) -> None:
        super().__init__()
        self._by_model = by_model
        self._expected = expected
        self._in_flight = 0
        self._all_started = asyncio.Event()
        self.max_in_flight = 0

    async def complete(self, **kwargs) -> LLMResult:
        self.calls.append(kwargs)
        self._in_flight += 1
        self.max_in_flight = max(self.max_in_flight, self._in_flight)
        if self._in_flight >= self._expected:
            self._all_started.set()
        await asyncio.wait_for(self._all_started.wait(), timeout=2)
        self._in_flight -= 1
        payload = self._by_model[kwargs["model"]]
        text = json.dumps(payload) if payload is not None else "not json at all"
        return LLMResult(text=text, call_id=f"fake-{len(self.calls)}", cost_usd=0.01, cached=False)


class _Cfg:
    def __init__(self, models: list[str]) -> None:
        self.llm_ensemble_models = models
        self.llm_routing = {}


def _ctx(engine, llm, models: list[str] = MODELS) -> tuple[Context, Instrument]:
    inst = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")
    _seed_bars(engine, inst.id)
    return Context(
        engine=engine, settings=None, config=_Cfg(models), llm=llm, universe=[inst]
    ), inst


def test_majority_vote_and_metadata(tmp_engine):
    llm = PerModelLLM(
        {
            "m/a": _answer("long", 0.8, horizon=10, tag="a"),
            "m/b": _answer("long", 0.6, horizon=30, tag="b"),
            "m/c": _answer("short", 0.9, horizon=20, tag="c"),
        },
        expected=3,
    )
    ctx, inst = _ctx(tmp_engine, llm)
    signals = asyncio.run(Ensemble().generate(ctx))

    assert len(signals) == 1
    sig = signals[0]
    assert sig.strategy == "ensemble"
    assert sig.model == "ensemble"
    assert sig.prompt_version == "analyst_v1"
    assert sig.instrument_id == inst.id
    assert sig.direction == "long"
    assert sig.conviction == pytest.approx(0.7)  # mean of agreeing models only
    assert sig.horizon_days == 20  # median of 10, 30, 20
    assert sig.thesis.startswith("Ensemble of 3 models: 2 long, 1 short, 0 flat. thesis a")
    assert sig.invalidation == "invalidation a"  # highest-conviction agreeing model
    assert sig.evidence_ids, "ensemble signal must carry brief evidence"

    ens = sig.metadata["ensemble"]
    assert ens["models"] == MODELS
    assert ens["directions"] == ["long", "long", "short"]
    assert ens["convictions"] == [0.8, 0.6, 0.9]
    assert ens["dispersion"] == pytest.approx(statistics.stdev([0.8, 0.6, 0.9]))
    assert ens["agreement"] == pytest.approx(2 / 3)


def test_tie_resolves_to_flat(tmp_engine):
    llm = PerModelLLM(
        {"m/a": _answer("long", 0.8), "m/b": _answer("short", 0.7)},
        expected=2,
    )
    ctx, _ = _ctx(tmp_engine, llm, models=["m/a", "m/b"])
    signals = asyncio.run(Ensemble().generate(ctx))

    assert len(signals) == 1
    sig = signals[0]
    assert sig.direction == "flat"
    assert sig.conviction == 0.0
    assert sig.metadata["ensemble"]["agreement"] == 0.0
    assert "1 long, 1 short, 0 flat" in sig.thesis


def test_skips_when_fewer_than_two_valid_answers(tmp_engine):
    # Two of three models return garbage; structured() retries each once, so
    # the failing models account for two calls each.
    llm = PerModelLLM({"m/a": _answer("long", 0.8), "m/b": None, "m/c": None}, expected=3)
    ctx, _ = _ctx(tmp_engine, llm)
    signals = asyncio.run(Ensemble().generate(ctx))

    assert signals == []
    assert len(llm.calls) == 5


def test_per_model_calls_run_concurrently(tmp_engine):
    llm = PerModelLLM({m: _answer("long", 0.5) for m in MODELS}, expected=3)
    ctx, _ = _ctx(tmp_engine, llm)
    asyncio.run(Ensemble().generate(ctx))

    assert len(llm.calls) == 3
    assert sorted(c["model"] for c in llm.calls) == sorted(MODELS)
    assert llm.max_in_flight == 3


def test_requires_at_least_two_models(tmp_engine):
    ctx, _ = _ctx(tmp_engine, FakeLLM(), models=["m/a"])
    with pytest.raises(ValueError, match="at least two models"):
        asyncio.run(Ensemble().generate(ctx))


def test_skips_instrument_without_bars(tmp_engine):
    llm = PerModelLLM({m: _answer("long", 0.5) for m in MODELS}, expected=3)
    inst = Instrument(id="US:NOBARS", market="us", symbol="NOBARS", currency="USD")
    ctx = Context(engine=tmp_engine, settings=None, config=_Cfg(MODELS), llm=llm, universe=[inst])
    assert asyncio.run(Ensemble().generate(ctx)) == []
    assert llm.calls == []
