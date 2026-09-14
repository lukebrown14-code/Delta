"""Critic strategy: wrap another strategy and stress-test each of its signals.

For every base signal the ``critique`` model is asked, using only the same
brief, for the strongest counter-argument, a list of risks, a revised
conviction and a verdict. Both the base signal and the critic's revised signal
are returned so the scorecard can compare them side by side.
"""

from __future__ import annotations

import logging
import uuid
from datetime import UTC, datetime
from typing import Any, Literal

from pydantic import BaseModel, Field, ValidationError
from sqlmodel import Session, select

from rigger.brief import build_brief
from rigger.core.db import LLMCallTable
from rigger.core.models import Instrument, Signal
from rigger.core.plugin import Context, StrategyPlugin

CRITIC_TEMPLATE = "critic_v1.j2"
PROMPT_VERSION = CRITIC_TEMPLATE.removesuffix(".j2")
DEFAULT_WRAPS = "llm_analyst"
log = logging.getLogger(__name__)


class CritiqueDraft(BaseModel):
    counter_argument: str
    risks: list[str]
    revised_conviction: float = Field(ge=0, le=1)
    verdict: Literal["hold", "reduce", "reject"]


class Critic(StrategyPlugin):
    name = "critic"
    wraps: str = DEFAULT_WRAPS

    def configure(self, cfg: dict[str, Any]) -> None:
        self.wraps = str(cfg.get("wraps", DEFAULT_WRAPS))

    async def generate(self, ctx: Context) -> list[Signal]:
        base_plugin = ctx.plugins.get(self.wraps)
        if base_plugin is None:
            available = ", ".join(sorted(ctx.plugins)) or "none"
            raise LookupError(
                f"critic wraps {self.wraps!r} but no such plugin is loaded "
                f"(available: {available}); check [plugins.critic].wraps in config.toml"
            )
        if not isinstance(base_plugin, StrategyPlugin):
            raise TypeError(
                f"critic wraps {self.wraps!r} which is not a strategy plugin "
                f"({type(base_plugin).__name__})"
            )

        model = ctx.config.llm_routing.get("critique", "openai/gpt-4o")
        instruments = {inst.id: inst for inst in ctx.universe}

        base_signals = await base_plugin.generate(ctx)
        out: list[Signal] = list(base_signals)
        for base in base_signals:
            inst = instruments.get(base.instrument_id)
            if inst is None:
                log.warning("critic: %s not in universe; skipping", base.instrument_id)
                continue
            critic_signal = await self._critique(ctx, base, inst, model)
            if critic_signal is not None:
                out.append(critic_signal)
        return out

    async def _critique(
        self, ctx: Context, base: Signal, inst: Instrument, model: str
    ) -> Signal | None:
        from rigger.llm import structured as structured_mod

        brief = build_brief(ctx, inst)
        if brief is None:
            log.warning("critic: no brief for %s; skipping", inst.id)
            return None

        try:
            draft, call_id = await structured_mod.structured(
                ctx.llm,
                task="critique",
                model=model,
                template=CRITIC_TEMPLATE,
                vars={
                    "symbol": inst.symbol,
                    "brief": brief.render(),
                    "direction": base.direction,
                    "conviction": base.conviction,
                    "thesis": base.thesis,
                    "invalidation": base.invalidation,
                },
                schema=CritiqueDraft,
            )
        except ValidationError:
            # One bad critique must not discard the rest of the universe.
            log.exception("invalid critique output for %s from %s; skipping", inst.id, model)
            return None

        critique_cost = _call_cost(ctx, call_id)
        direction = "flat" if draft.verdict == "reject" else base.direction

        return Signal(
            id=uuid.uuid4().hex,
            ts=datetime.now(UTC),
            instrument_id=base.instrument_id,
            strategy=f"critic:{self.wraps}",
            direction=direction,
            conviction=draft.revised_conviction,
            horizon_days=base.horizon_days,
            thesis=base.thesis,
            invalidation=base.invalidation,
            evidence_ids=list(brief.evidence_ids or base.evidence_ids),
            model=model,
            prompt_version=PROMPT_VERSION,
            cost_usd=(base.cost_usd or 0.0) + critique_cost,
            metadata={
                **base.metadata,
                "critic": {
                    "base_signal_id": base.id,
                    "original_conviction": base.conviction,
                    "counter_argument": draft.counter_argument,
                    "risks": draft.risks,
                    "verdict": draft.verdict,
                    "model": model,
                    "prompt_version": PROMPT_VERSION,
                },
            },
        )


def _call_cost(ctx: Context, call_id: str) -> float:
    """Cost of one logged LLM call. Cache hits (empty id) and unknown ids cost 0."""
    if not call_id:
        return 0.0
    with Session(ctx.engine) as session:
        row = session.exec(select(LLMCallTable).where(LLMCallTable.id == call_id)).first()
    return row.cost_usd if row is not None else 0.0
