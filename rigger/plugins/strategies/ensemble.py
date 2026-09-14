"""Ensemble strategy: run the analyst prompt through N models and combine."""

from __future__ import annotations

import asyncio
import logging
import statistics
import uuid
from collections import Counter
from datetime import UTC, datetime
from typing import Any

from rigger.brief import Brief, build_brief
from rigger.core.models import Direction, Instrument, Signal
from rigger.core.plugin import Context, StrategyPlugin
from rigger.plugins.strategies.llm_analyst import ANALYST_TEMPLATE, analyse_one

MIN_MODELS = 2
log = logging.getLogger(__name__)


class Ensemble(StrategyPlugin):
    """Majority-vote ensemble over ``[llm.ensemble].models``.

    Each model answers the exact analyst prompt via ``analyse_one``; the answers
    are combined into one signal that records per-model disagreement in
    ``metadata["ensemble"]``.
    """

    name = "ensemble"

    async def generate(self, ctx: Context) -> list[Signal]:
        models: list[str] = list(ctx.config.llm_ensemble_models)
        if len(models) < MIN_MODELS:
            raise ValueError(
                "ensemble strategy needs at least two models in [llm.ensemble].models; "
                f"got {models!r}"
            )

        signals: list[Signal] = []
        for inst in ctx.universe:
            brief = build_brief(ctx, inst)
            if brief is None:
                continue
            results = await asyncio.gather(
                *(analyse_one(ctx, inst, model, brief, strategy=self.name) for model in models)
            )
            answers = [
                (model, sig) for model, sig in zip(models, results, strict=True) if sig is not None
            ]
            if len(answers) < MIN_MODELS:
                log.warning(
                    "ensemble: only %d valid answer(s) for %s; skipping", len(answers), inst.id
                )
                continue
            signals.append(combine(inst, brief, answers))
        return signals


def combine(inst: Instrument, brief: Brief, answers: list[tuple[str, Signal]]) -> Signal:
    """Fold per-model signals into a single ensemble signal."""
    models = [m for m, _ in answers]
    sigs = [s for _, s in answers]
    directions = [s.direction for s in sigs]
    convictions = [s.conviction for s in sigs]
    counts = Counter(directions)

    majority = _majority(counts)
    agreeing = [s for s in sigs if s.direction == majority]
    if agreeing:
        conviction = statistics.fmean(s.conviction for s in agreeing)
        lead = max(agreeing, key=lambda s: s.conviction)
    else:
        # Tie resolved to flat with no flat voter: nobody agrees, so no conviction.
        conviction = 0.0
        lead = max(sigs, key=lambda s: s.conviction)

    horizon = round(statistics.median(s.horizon_days for s in sigs))
    summary = (
        f"Ensemble of {len(sigs)} models: {counts.get('long', 0)} long, "
        f"{counts.get('short', 0)} short, {counts.get('flat', 0)} flat."
    )
    dispersion = statistics.stdev(convictions) if len(convictions) > 1 else 0.0
    costs = [s.cost_usd for s in sigs if s.cost_usd is not None]

    metadata: dict[str, Any] = {
        "ensemble": {
            "models": models,
            "directions": directions,
            "convictions": convictions,
            "dispersion": dispersion,
            "agreement": len(agreeing) / len(sigs),
        }
    }
    return Signal(
        id=uuid.uuid4().hex,
        ts=datetime.now(UTC),
        instrument_id=inst.id,
        strategy="ensemble",
        direction=majority,
        conviction=conviction,
        horizon_days=horizon,
        thesis=f"{summary} {lead.thesis}",
        invalidation=lead.invalidation,
        evidence_ids=brief.evidence_ids,
        model="ensemble",
        prompt_version=ANALYST_TEMPLATE.removesuffix(".j2"),
        cost_usd=sum(costs) if costs else None,
        metadata=metadata,
    )


def _majority(counts: Counter[Direction]) -> Direction:
    ranked = counts.most_common()
    if len(ranked) > 1 and ranked[0][1] == ranked[1][1]:
        return "flat"
    return ranked[0][0]
