"""Position sizing and exposure rules."""

from __future__ import annotations

from dataclasses import dataclass

from rigger.core.models import Instrument, Signal


@dataclass
class RiskLimits:
    max_position_pct: float = 5.0
    max_sector_pct: float = 25.0
    max_gross_exposure_pct: float = 100.0
    min_conviction: float = 0.6
    daily_loss_halt_pct: float = 3.0


@dataclass
class SizingDecision:
    approved: bool
    qty: float = 0.0
    reason: str = ""


def size_signal(
    signal: Signal,
    instrument: Instrument,
    equity: float,
    price: float,
    limits: RiskLimits,
    sector_exposure_pct: float = 0.0,
    gross_exposure_pct: float = 0.0,
    cash: float | None = None,
) -> SizingDecision:
    if signal.direction == "flat":
        return SizingDecision(approved=False, reason="flat signal")

    if signal.conviction < limits.min_conviction:
        return SizingDecision(
            approved=False,
            reason=f"conviction {signal.conviction:.2f} < min {limits.min_conviction}",
        )

    if equity <= 0:
        return SizingDecision(approved=False, reason="no equity")

    # conviction <= 1 (validated on Signal), so this is already capped at max_position_pct.
    notional = equity * limits.max_position_pct / 100.0 * signal.conviction
    notional_pct = notional / equity * 100

    if instrument.sector and sector_exposure_pct + notional_pct > limits.max_sector_pct:
        return SizingDecision(approved=False, reason="exceeds sector exposure limit")

    if gross_exposure_pct + notional_pct > limits.max_gross_exposure_pct:
        return SizingDecision(approved=False, reason="exceeds gross exposure limit")

    if cash is not None and signal.direction == "long" and notional > cash:
        return SizingDecision(approved=False, reason="insufficient cash")

    if price <= 0:
        return SizingDecision(approved=False, reason="non-positive price")

    qty = notional / price
    if qty <= 0:
        return SizingDecision(approved=False, reason="zero quantity")

    return SizingDecision(approved=True, qty=qty, reason="ok")
