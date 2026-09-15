"""Sizing rules."""

from __future__ import annotations

from datetime import UTC, datetime

from rigger.core.models import Instrument, Signal
from rigger.paper.risk import RiskLimits, size_signal

INST = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Tech")


def _sig(direction: str = "long") -> Signal:
    return Signal(
        id="s",
        ts=datetime.now(UTC),
        instrument_id=INST.id,
        strategy="t",
        direction=direction,  # type: ignore[arg-type]
        conviction=0.8,
        horizon_days=20,
        thesis="t",
        invalidation="i",
    )


def test_long_rejected_when_already_held():
    d = size_signal(_sig(), INST, 100_000.0, 150.0, RiskLimits(), held_qty=5.0)
    assert not d.approved
    assert d.reason == "already long"


def test_long_approved_when_flat():
    d = size_signal(_sig(), INST, 100_000.0, 150.0, RiskLimits(), held_qty=0.0)
    assert d.approved
    assert d.qty > 0


def test_short_not_blocked_by_holding():
    d = size_signal(_sig("short"), INST, 100_000.0, 150.0, RiskLimits(), held_qty=5.0)
    assert d.approved
