"""Momentum baseline: 12-1 month price momentum ranked across the universe.

A non-LLM benchmark the model strategies have to beat. Momentum is the
return from 253 to 22 trading days ago (twelve months, skipping the most
recent month, which tends to mean-revert). The top quintile goes long,
everything else is flat.
"""

from __future__ import annotations

import uuid
from dataclasses import dataclass
from datetime import UTC, datetime

from sqlmodel import Session, select

from rigger.core.db import BarTable
from rigger.core.models import Instrument, Signal
from rigger.core.plugin import Context, StrategyPlugin

LOOKBACK_BARS = 253  # close[-253]: roughly twelve months of sessions
SKIP_BARS = 22  # close[-22]: skip the most recent month
TOP_FRACTION = 0.2
HORIZON_DAYS = 21
INVALIDATION = "Falls out of top quintile of 12-1 momentum at next monthly rank."


@dataclass
class _Score:
    instrument: Instrument
    momentum: float
    start_close: float
    end_close: float
    evidence_ids: list[str]


def _score(session: Session, inst: Instrument, as_of: datetime) -> _Score | None:
    rows = session.exec(
        select(BarTable)
        .where(BarTable.instrument_id == inst.id)
        .where(BarTable.ts <= as_of)
        .order_by(BarTable.ts.desc())  # type: ignore[attr-defined]
        .limit(LOOKBACK_BARS)
    ).all()
    if len(rows) < LOOKBACK_BARS:
        return None
    bars = list(reversed(rows))
    start, end = bars[-LOOKBACK_BARS], bars[-SKIP_BARS]
    if start.close <= 0:
        return None
    return _Score(
        instrument=inst,
        momentum=end.close / start.close - 1,
        start_close=start.close,
        end_close=end.close,
        evidence_ids=[str(start.id), str(end.id)],
    )


class Momentum(StrategyPlugin):
    name = "momentum"

    async def generate(self, ctx: Context) -> list[Signal]:
        as_of = datetime.now(UTC)
        scores: list[_Score] = []
        with Session(ctx.engine) as session:
            for inst in ctx.universe:
                score = _score(session, inst, as_of)
                if score is not None:
                    scores.append(score)
        if not scores:
            return []

        # Best momentum first; ties broken by id so the ranking is reproducible.
        scores.sort(key=lambda s: (-s.momentum, s.instrument.id))
        n = len(scores)
        signals: list[Signal] = []
        for rank, s in enumerate(scores):
            percentile = 1.0 if n == 1 else (n - 1 - rank) / (n - 1)
            is_long = percentile >= 1.0 - TOP_FRACTION
            signals.append(
                Signal(
                    id=uuid.uuid4().hex,
                    ts=as_of,
                    instrument_id=s.instrument.id,
                    strategy=self.name,
                    direction="long" if is_long else "flat",
                    conviction=percentile,
                    horizon_days=HORIZON_DAYS,
                    thesis=(
                        f"12-1 month momentum {s.momentum:+.1%} "
                        f"(close {s.start_close:.2f} -> {s.end_close:.2f}); "
                        f"ranked {rank + 1} of {n} in universe."
                    ),
                    invalidation=INVALIDATION,
                    evidence_ids=s.evidence_ids,
                    model=None,
                    prompt_version=None,
                    cost_usd=0.0,
                )
            )
        return signals
