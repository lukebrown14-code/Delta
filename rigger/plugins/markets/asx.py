"""ASX (Australian Securities Exchange) market plugin."""

from __future__ import annotations

from datetime import UTC, datetime, time, timedelta
from typing import Any
from zoneinfo import ZoneInfo

from rigger.core.ids import make_instrument_id
from rigger.core.models import Instrument
from rigger.core.plugin import MarketPlugin

SYDNEY = ZoneInfo("Australia/Sydney")
MARKET_OPEN = time(10, 0)
MARKET_CLOSE = time(16, 0)

FEE_RATE = 0.001  # 0.1 % of notional
FEE_MIN = 10.0  # AUD


class ASXMarket(MarketPlugin):
    name = "asx"
    currency = "AUD"

    def __init__(self) -> None:
        self._tickers: list[str] = []

    def configure(self, cfg: dict[str, Any]) -> None:
        # The CLI passes config.universe["asx"] as {"tickers": [...]}, mirroring USMarket.
        self._tickers = [str(t).upper() for t in cfg.get("tickers", [])]

    def universe(self) -> list[Instrument]:
        return [
            Instrument(
                id=make_instrument_id("ASX", code),
                market=self.name,
                symbol=code,
                name=code,
                currency=self.currency,
                sector=None,  # TODO: sector lookup (yfinance .info) once it is cheap enough
            )
            for code in self._tickers
        ]

    # TODO: add an ASX public-holiday calendar; weekends only for now.
    def is_open(self, ts: datetime) -> bool:
        local = ts.astimezone(SYDNEY)
        if local.weekday() >= 5:
            return False
        return MARKET_OPEN <= local.time() <= MARKET_CLOSE

    def next_open(self, ts: datetime) -> datetime:
        local = ts.astimezone(SYDNEY)
        candidate = local.replace(
            hour=MARKET_OPEN.hour, minute=MARKET_OPEN.minute, second=0, microsecond=0
        )
        if candidate <= local:
            candidate += timedelta(days=1)
        while candidate.weekday() >= 5:
            candidate += timedelta(days=1)
        return candidate.astimezone(UTC)

    @staticmethod
    def fee(notional: float) -> float:
        """Brokerage: 0.1 % of notional with a $10 minimum."""
        return max(abs(notional) * FEE_RATE, FEE_MIN)
