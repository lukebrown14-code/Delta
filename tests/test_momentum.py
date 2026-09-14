"""Tests for the non-LLM momentum baseline on synthetic bar histories."""

from __future__ import annotations

import asyncio
from collections.abc import Callable
from datetime import UTC, datetime

import pytest
from sqlmodel import Session, select

from rigger.core.db import BarTable
from rigger.core.models import Instrument, Signal
from rigger.core.plugin import Context
from rigger.plugins.strategies.momentum import (
    INVALIDATION,
    LOOKBACK_BARS,
    SKIP_BARS,
    Momentum,
)
from tests.conftest import seed_bars

N_BARS = 260
START = datetime(2025, 9, 1, tzinfo=UTC)


def _inst(symbol: str) -> Instrument:
    return Instrument(id=f"US:{symbol}", market="us", symbol=symbol, currency="USD")


def _linear(slope: float) -> Callable[[int], float]:
    return lambda i: 100.0 * (1 + slope * i)


def _late_spike(i: int) -> float:
    # Flat for a year, then +50% inside the skipped final month. 12-1 momentum
    # must ignore this.
    return 150.0 if i >= N_BARS - 20 else 100.0


# Expected ranking: STRONG > MILD > FLAT > SPIKE (0.0) > DOWN.
PRICE_FN: dict[str, Callable[[int], float]] = {
    "STRONG": _linear(0.004),
    "MILD": _linear(0.002),
    "FLAT": _linear(0.0005),
    "SPIKE": _late_spike,
    "DOWN": _linear(-0.002),
}


def _expected_momentum(price_fn: Callable[[int], float]) -> float:
    return price_fn(N_BARS - SKIP_BARS) / price_fn(N_BARS - LOOKBACK_BARS) - 1


@pytest.fixture
def universe(tmp_engine) -> list[Instrument]:
    insts = [_inst(sym) for sym in PRICE_FN]
    for inst in insts:
        seed_bars(tmp_engine, inst.id, n=N_BARS, start=START, price_fn=PRICE_FN[inst.symbol])
    short = _inst("SHORT")  # 100 bars only, must be skipped
    seed_bars(tmp_engine, short.id, n=100, start=START, price_fn=_linear(0.01))
    return [*insts, short]


def _run(engine, universe: list[Instrument]) -> dict[str, Signal]:
    ctx = Context(engine=engine, settings=None, config=None, universe=universe)
    signals = asyncio.run(Momentum().generate(ctx))
    return {s.instrument_id: s for s in signals}


def test_ranks_and_quintile_cut(tmp_engine, universe):
    by_id = _run(tmp_engine, universe)
    assert set(by_id) == {f"US:{s}" for s in PRICE_FN}, "short history must be skipped"

    ranked = sorted(by_id.values(), key=lambda s: -s.conviction)
    assert [s.instrument_id for s in ranked] == [
        "US:STRONG",
        "US:MILD",
        "US:FLAT",
        "US:SPIKE",
        "US:DOWN",
    ]
    assert [s.conviction for s in ranked] == [1.0, 0.75, 0.5, 0.25, 0.0]
    # Top 20 % of five is one instrument.
    assert [s.direction for s in ranked] == ["long", "flat", "flat", "flat", "flat"]


def test_recent_month_is_skipped(tmp_engine, universe):
    spike = _run(tmp_engine, universe)["US:SPIKE"]
    assert _expected_momentum(_late_spike) == 0.0
    assert "+0.0%" in spike.thesis
    assert spike.direction == "flat"


def test_signal_fields_and_evidence(tmp_engine, universe):
    strong = _run(tmp_engine, universe)["US:STRONG"]
    assert strong.strategy == "momentum"
    assert strong.horizon_days == 21
    assert strong.model is None and strong.prompt_version is None
    assert strong.cost_usd == 0.0
    assert strong.invalidation == INVALIDATION
    assert f"{_expected_momentum(PRICE_FN['STRONG']):+.1%}" in strong.thesis
    assert "ranked 1 of 5" in strong.thesis

    # Evidence is exactly the two bars the figure was computed from.
    assert len(strong.evidence_ids) == 2
    with Session(tmp_engine) as session:
        rows = session.exec(
            select(BarTable).where(BarTable.id.in_([int(e) for e in strong.evidence_ids]))  # type: ignore[attr-defined]
        ).all()
    offsets = sorted((r.ts.replace(tzinfo=UTC) - START).days for r in rows)
    assert offsets == [N_BARS - LOOKBACK_BARS, N_BARS - SKIP_BARS]


def test_single_instrument_is_long_with_full_conviction(tmp_engine):
    inst = _inst("ONLY")
    seed_bars(tmp_engine, inst.id, n=N_BARS, start=START, price_fn=_linear(-0.001))
    sig = _run(tmp_engine, [inst])[inst.id]
    assert sig.direction == "long" and sig.conviction == 1.0


def test_no_history_yields_no_signals(tmp_engine):
    assert _run(tmp_engine, [_inst("NONE")]) == {}
