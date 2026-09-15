"""FX rates for the paper book, stored as daily bars of synthetic ``FX:<CCY><BASE>`` instruments.

The yfinance symbol ``USDAUD=X`` closes at AUD per 1 USD, so
``local_amount * rate == base_amount``. No new table: rates ride the existing
bar pipeline (``rig ingest`` -> ``store_items`` -> ``BarTable``).
"""

from __future__ import annotations

from sqlalchemy.engine import Engine
from sqlmodel import Session, select

from rigger.core.db import BarTable
from rigger.core.models import Instrument

FX_MARKET = "fx"


def market_of(instrument_id: str) -> str:
    """``"US:AAPL"`` -> ``"us"``; ids without a prefix are treated as US."""
    return instrument_id.split(":", 1)[0].lower() if ":" in instrument_id else "us"


def fx_instrument_id(ccy: str, base: str) -> str:
    return f"FX:{ccy.upper()}{base.upper()}"


def fx_instrument(ccy: str, base: str) -> Instrument:
    """Synthetic instrument whose bars are ``base`` per 1 ``ccy``."""
    return Instrument(
        id=fx_instrument_id(ccy, base),
        market=FX_MARKET,
        symbol=f"{ccy.upper()}{base.upper()}=X",
        name=f"{ccy.upper()}/{base.upper()}",
        currency=base.upper(),
    )


def fx_instruments(universe: list[Instrument], base: str) -> list[Instrument]:
    """One FX instrument per foreign currency present in ``universe``."""
    seen: set[str] = set()
    out: list[Instrument] = []
    for inst in universe:
        ccy = inst.currency.upper()
        if ccy == base.upper() or ccy in seen:
            continue
        seen.add(ccy)
        out.append(fx_instrument(ccy, base))
    return out


def latest_fx_rate(engine: Engine, ccy: str, base: str) -> float | None:
    """Latest stored ``base`` per 1 ``ccy``; 1.0 for the base itself; None if never ingested."""
    if ccy.upper() == base.upper():
        return 1.0
    with Session(engine) as session:
        row = session.exec(
            select(BarTable)
            .where(BarTable.instrument_id == fx_instrument_id(ccy, base))
            .order_by(BarTable.ts.desc())  # type: ignore[attr-defined]
        ).first()
    return row.close if row else None
