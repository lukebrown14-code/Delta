"""Critic strategy: wrap another strategy and stress-test each of its signals.

For every base signal the ``critique`` model is asked, using only the same
brief, for the strongest counter-argument, a list of risks, a revised
conviction and a verdict. Both the base signal and the critic's revised signal
are returned so the scorecard can compare them side by side.
"""

from __future__ import annotations

import asyncio
import logging
import uuid
from datetime import UTC, datetime
from typing import Any, Literal

from pydantic import BaseModel, Field, ValidationError

from rigger.brief import build_brief
from rigger.core.models import Instrument, Signal
from rigger.core.plugin import Context, StrategyPlugin
from rigger.llm.router import model_for

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

        model = model_for(ctx.config, "critique")
        instruments = {inst.id: inst for inst in ctx.universe}

        base_signals = await base_plugin.generate(ctx)
        targets = []
        for base in base_signals:
            inst = instruments.get(base.instrument_id)
            if inst is None:
                log.warning("critic: %s not in universe; skipping", base.instrument_id)
                continue
            targets.append((base, inst))
        # Each critique is an independent LLM call; run them concurrently.
        critiques = await asyncio.gather(
            *(self._critique(ctx, base, inst, model) for base, inst in targets)
        )
        return list(base_signals) + [c for c in critiques if c is not None]

    async def _critique(
        self, ctx: Context, base: Signal, inst: Instrument, model: str
    ) -> Signal | None:
        from rigger.llm import structured as structured_mod

        brief = build_brief(ctx, inst)
        if brief is None:
            log.warning("critic: no brief for %s; skipping", inst.id)
            return None

        try:
            draft, result = await structured_mod.structured(
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
            evidence_ids=brief.evidence_ids,
            model=model,
            prompt_version=PROMPT_VERSION,
            cost_usd=(base.cost_usd or 0.0) + result.cost_usd,
            metadata={
                **base.metadata,
                "critic": {
                    "base_signal_id": base.id,
                    "original_conviction": base.conviction,
                    "counter_argument": draft.counter_argument,
                    "risks": draft.risks,
                    "verdict": draft.verdict,
                },
            },
        )
