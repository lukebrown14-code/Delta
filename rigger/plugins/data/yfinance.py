"""yfinance data plugin: historical daily bars for any market.

yfinance needs an exchange suffix for non-US symbols (BHP -> BHP.AX). The
suffix per market comes from ``[plugins.yfinance].suffixes`` so adding a market
is a config change, not a code change.
"""

from __future__ import annotations

from datetime import UTC, datetime
from typing import Any

from rigger.core.models import Bar, Event, Fundamental, Instrument, NewsItem
from rigger.core.plugin import DataPlugin

DEFAULT_SUFFIXES: dict[str, str] = {"us": "", "asx": ".AX"}


class YFinanceData(DataPlugin):
    name = "yfinance"
    market = None  # works for any market with a known suffix

    def __init__(self) -> None:
        self.suffixes: dict[str, str] = dict(DEFAULT_SUFFIXES)

    def configure(self, cfg: dict[str, Any]) -> None:
        super().configure(cfg)
        self.suffixes.update(cfg.get("suffixes", {}))

    def yf_symbol(self, inst: Instrument) -> str:
        return f"{inst.symbol.upper()}{self.suffixes.get(inst.market, '')}"

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental | Event]:
        import yfinance as yf

        bars: list[Bar] = []
        for inst in instruments:
            ticker = yf.Ticker(self.yf_symbol(inst))
            df = ticker.history(start=since.date(), interval="1d", auto_adjust=True)
            if df is None or df.empty:
                continue
            for ts, row in df.iterrows():
                bars.append(
                    Bar(
                        instrument_id=inst.id,
                        ts=ts.to_pydatetime().astimezone(UTC),
                        open=float(row["Open"]),
                        high=float(row["High"]),
                        low=float(row["Low"]),
                        close=float(row["Close"]),
                        volume=float(row["Volume"]),
                        source=self.name,
                    )
                )
        return bars
