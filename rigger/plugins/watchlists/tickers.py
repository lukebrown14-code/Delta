"""Config-only ticker watchlists."""

from __future__ import annotations

from typing import Any

from rigger.core.ids import make_instrument_id
from rigger.core.models import AssetClass, Instrument
from rigger.core.plugin import WatchlistPlugin


class TickerWatchlist(WatchlistPlugin):
    kind = "tickers"
    version = "0.1.0"

    def __init__(self) -> None:
        self.name = ""
        self.label = ""
        self.market = ""
        self.tickers: list[str] = []
        self.asset_class: AssetClass = "equity"
        self.sector: str | None = None
        self.tags: frozenset[str] = frozenset()
        self.max_pct: float | None = None
        self.overrides: dict[str, dict[str, Any]] = {}

    def configure(self, cfg: dict[str, Any]) -> None:
        self.name = str(cfg.get("name", self.name))
        self.label = str(cfg.get("label", self.name))
        self.market = str(cfg.get("market", "")).lower()
        self.tickers = [str(t).upper() for t in cfg.get("tickers", [])]
        self.asset_class = cfg.get("asset_class", "equity")
        self.sector = cfg.get("sector")
        self.tags = frozenset(str(tag) for tag in cfg.get("tags", []))
        self.max_pct = float(cfg["max_pct"]) if cfg.get("max_pct") is not None else None
        self.overrides = {
            str(symbol).upper(): dict(values) for symbol, values in cfg.get("overrides", {}).items()
        }

    def instruments(self) -> list[Instrument]:
        if not self.market:
            raise ValueError(f"watchlist {self.name!r} must set market")
        currency = {"us": "USD", "asx": "AUD"}.get(self.market, "AUD")
        result = []
        for symbol in self.tickers:
            override = self.overrides.get(symbol, {})
            result.append(
                Instrument(
                    id=make_instrument_id(self.market.upper(), symbol),
                    market=self.market,
                    symbol=symbol,
                    name=symbol,
                    currency=str(override.get("currency", currency)),
                    sector=override.get("sector", self.sector),
                    asset_class=override.get("asset_class", self.asset_class),
                    watchlists=(self.name,),
                    tags=frozenset(override.get("tags", self.tags)),
                    industry=override.get("industry"),
                    meta=dict(override.get("meta", {})),
                )
            )
        return result
