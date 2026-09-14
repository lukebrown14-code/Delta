"""LLM analyst strategy: assemble a facts-only brief and request a signal."""

from __future__ import annotations

import asyncio
import logging
import uuid
from datetime import UTC, datetime
from typing import Literal

from pydantic import BaseModel, Field, ValidationError

from rigger.brief import Brief, build_brief
from rigger.core.models import Instrument, Signal
from rigger.core.plugin import Context, StrategyPlugin
from rigger.llm.router import model_for

ANALYST_TEMPLATE = "analyst_v1.j2"
PROMPT_VERSION = ANALYST_TEMPLATE.removesuffix(".j2")
log = logging.getLogger(__name__)


class SignalDraft(BaseModel):
    direction: Literal["long", "short", "flat"]
    conviction: float = Field(ge=0, le=1)
    horizon_days: int
    thesis: str
    invalidation: str


async def analyse_one(
    ctx: Context,
    inst: Instrument,
    model: str,
    brief: Brief,
    *,
    strategy: str = "llm_analyst",
) -> Signal | None:
    """Run the analyst prompt for one instrument with one model.

    Shared by the analyst and ensemble strategies so both use the exact same
    prompt and schema. Returns None when the model's answer fails validation.
    """
    from rigger.llm import structured as structured_mod

    try:
        draft, result = await structured_mod.structured(
            ctx.llm,
            task="analyse",
            model=model,
            template=ANALYST_TEMPLATE,
            vars={"symbol": inst.symbol, "instrument_id": inst.id, "brief": brief.render()},
            schema=SignalDraft,
        )
    except ValidationError:
        # One bad model answer must not discard the rest of the universe.
        log.exception("invalid model output for %s from %s; skipping", inst.id, model)
        return None

    return Signal(
        id=uuid.uuid4().hex,
        ts=datetime.now(UTC),
        instrument_id=inst.id,
        strategy=strategy,
        direction=draft.direction,
        conviction=draft.conviction,
        horizon_days=draft.horizon_days,
        thesis=draft.thesis,
        invalidation=draft.invalidation,
        evidence_ids=brief.evidence_ids,
        model=model,
        prompt_version=PROMPT_VERSION,
        cost_usd=result.cost_usd,
    )


class LLMAnalyst(StrategyPlugin):
    name = "llm_analyst"

    async def generate(self, ctx: Context) -> list[Signal]:
        model = model_for(ctx.config, "analyse")
        briefs = [(inst, b) for inst in ctx.universe if (b := build_brief(ctx, inst)) is not None]
        # Independent LLM calls: run them concurrently, keep universe order.
        results = await asyncio.gather(
            *(analyse_one(ctx, inst, model, brief, strategy=self.name) for inst, brief in briefs)
        )
        return [s for s in results if s is not None]
