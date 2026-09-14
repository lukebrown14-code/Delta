"""Paper broker plugin: wraps PaperPortfolio, fills at next session open."""

from __future__ import annotations

from rigger.core.models import Fill, Order, Position
from rigger.core.plugin import BrokerPlugin
from rigger.paper.portfolio import PaperPortfolio


class PaperBroker(BrokerPlugin):
    name = "paper"

    def __init__(self) -> None:
        self._portfolio: PaperPortfolio | None = None

    def configure(self, cfg: dict) -> None:
        pass

    def bind(self, portfolio: PaperPortfolio) -> None:
        self._portfolio = portfolio

    async def submit(self, order: Order) -> Fill:
        assert self._portfolio is not None, "PaperBroker not bound to a portfolio"
        return self._portfolio.fill(order, market=self._market_of(order.instrument_id))

    async def positions(self) -> list[Position]:
        assert self._portfolio is not None
        return self._portfolio.positions()

    async def cash(self) -> float:
        assert self._portfolio is not None
        return self._portfolio.cash()

    @staticmethod
    def _market_of(instrument_id: str) -> str:
        return instrument_id.split(":", 1)[0].lower() if ":" in instrument_id else "us"
