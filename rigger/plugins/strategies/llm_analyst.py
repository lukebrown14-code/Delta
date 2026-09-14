"""LLM analyst strategy: assemble a facts-only brief and request a signal."""

from __future__ import annotations

import logging
import uuid
from datetime import UTC, datetime
from typing import Literal

from pydantic import BaseModel, Field, ValidationError
from sqlmodel import Session, select

from rigger.core.db import BarTable
from rigger.core.models import Instrument, Signal
from rigger.core.plugin import Context, StrategyPlugin

ANALYST_TEMPLATE = "analyst_v1.j2"
log = logging.getLogger(__name__)


class SignalDraft(BaseModel):
    direction: Literal["long", "short", "flat"]
    conviction: float = Field(ge=0, le=1)
    horizon_days: int
    thesis: str
    invalidation: str


class LLMAnalyst(StrategyPlugin):
    name = "llm_analyst"

    async def generate(self, ctx: Context) -> list[Signal]:
        from rigger.llm import structured as structured_mod

        signals: list[Signal] = []
        model = ctx.config.llm_routing.get("analyse", "anthropic/claude-sonnet-4")

        for inst in ctx.universe:
            brief, evidence_ids = self._build_brief(ctx, inst)
            if brief is None:
                continue

            try:
                draft, _call_id = await structured_mod.structured(
                    ctx.llm,
                    task="analyse",
                    model=model,
                    template=ANALYST_TEMPLATE,
                    vars={"symbol": inst.symbol, "instrument_id": inst.id, "brief": brief},
                    schema=SignalDraft,
                )
            except ValidationError:
                # One bad model answer must not discard the rest of the universe.
                log.exception("invalid model output for %s; skipping", inst.id)
                continue

            signal = Signal(
                id=uuid.uuid4().hex,
                ts=datetime.now(UTC),
                instrument_id=inst.id,
                strategy=self.name,
                direction=draft.direction,
                conviction=draft.conviction,
                horizon_days=draft.horizon_days,
                thesis=draft.thesis,
                invalidation=draft.invalidation,
                evidence_ids=evidence_ids,
                model=model,
                prompt_version=ANALYST_TEMPLATE.removesuffix(".j2"),
            )
            signals.append(signal)

        return signals

    def _build_brief(self, ctx: Context, inst: Instrument) -> tuple[str | None, list[str]]:
        with Session(ctx.engine) as session:
            rows = session.exec(
                select(BarTable)
                .where(BarTable.instrument_id == inst.id)
                .order_by(BarTable.ts.desc())
                .limit(120)
            ).all()

        if not rows:
            return None, []

        rows = list(reversed(rows))
        closes = [r.close for r in rows]
        evidence_ids = [str(r.id) for r in rows[-60:]]

        latest = rows[-1].close
        change_20 = (latest / closes[-21] - 1) * 100 if len(closes) > 21 else 0.0
        sma20 = sum(closes[-20:]) / min(20, len(closes))
        sma50 = sum(closes[-50:]) / min(50, len(closes))
        momentum_21 = (latest / closes[0] - 1) * 100 if len(closes) > 1 else 0.0
        high = max(closes[-60:])
        low = min(closes[-60:])

        lines = [
            f"Latest close: {latest:.2f} {inst.currency}",
            f"20-day return: {change_20:+.2f}%",
            f"Return over window: {momentum_21:+.2f}%",
            f"20-day SMA: {sma20:.2f}, 50-day SMA: {sma50:.2f}",
            f"60-day range: {low:.2f} - {high:.2f}",
        ]
        if inst.sector:
            lines.append(f"Sector: {inst.sector}")
        lines.append("Fundamentals: none available")
        lines.append("Upcoming events: none available")
        lines.append("News: none available")

        return "\n".join(lines), evidence_ids
