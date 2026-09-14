"""US equities market plugin."""

from __future__ import annotations

from datetime import UTC, datetime, time, timedelta
from zoneinfo import ZoneInfo

from rigger.core.models import Instrument
from rigger.core.plugin import MarketPlugin

US_EASTERN = ZoneInfo("America/New_York")
MARKET_OPEN = time(9, 30)
MARKET_CLOSE = time(16, 0)

COMMON_SECTORS = {
    "AAPL": "Technology",
    "MSFT": "Technology",
    "NVDA": "Technology",
    "GOOGL": "Communication Services",
    "AMZN": "Consumer Discretionary",
    "META": "Communication Services",
    "TSLA": "Consumer Discretionary",
}


class USMarket(MarketPlugin):
    name = "us"
    currency = "USD"

    def __init__(self) -> None:
        self._tickers: list[str] = []

    def configure(self, cfg: dict) -> None:
        self._tickers = list(cfg.get("tickers", []))

    def universe(self) -> list[Instrument]:
        symbols = self._tickers
        return [
            Instrument(
                id=f"US:{s}",
                market="us",
                symbol=s,
                name=s,
                currency="USD",
                sector=COMMON_SECTORS.get(s),
            )
            for s in symbols
        ]

    def is_open(self, ts: datetime) -> bool:
        local = ts.astimezone(US_EASTERN)
        if local.weekday() >= 5:
            return False
        return MARKET_OPEN <= local.time() <= MARKET_CLOSE

    def next_open(self, ts: datetime) -> datetime:
        local = ts.astimezone(US_EASTERN)
        candidate = local
        for _ in range(14):
            candidate = (candidate + timedelta(days=1)).replace(
                hour=MARKET_OPEN.hour, minute=MARKET_OPEN.minute, second=0, microsecond=0
            )
            if candidate.weekday() < 5:
                return candidate.astimezone(UTC)
        return ts
