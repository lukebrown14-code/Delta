"""yfinance data plugin: historical daily bars for any market.

yfinance needs an exchange suffix for non-US symbols (BHP -> BHP.AX). The
suffix per market comes from ``[plugins.yfinance].suffixes`` so adding a market
is a config change, not a code change.
"""

from __future__ import annotations

import asyncio
import math
from datetime import UTC, datetime
from typing import Any

from delta.core.models import Bar, Event, Fundamental, Instrument, NewsItem
from delta.core.plugin import DataPlugin

DEFAULT_SUFFIXES: dict[str, str] = {"us": "", "asx": ".AX"}


def yf_symbol(inst: Instrument, suffixes: dict[str, str] | None = None) -> str:
    """yfinance ticker for an instrument: BHP on asx -> BHP.AX. One source of truth."""
    table = suffixes if suffixes is not None else DEFAULT_SUFFIXES
    return f"{inst.symbol.upper()}{table.get(inst.market, '')}"


class YFinanceSymbols(DataPlugin):
    """Base for yfinance-backed plugins: shares the ``suffixes`` config table.

    Every subclass reads ``[plugins.yfinance].suffixes`` (via ``shared_config``)
    so a market added there applies to bars and calendar alike.
    """

    market = None  # works for any market with a known suffix
    shared_config = "yfinance"

    def __init__(self) -> None:
        self.suffixes: dict[str, str] = dict(DEFAULT_SUFFIXES)

    def configure(self, cfg: dict[str, Any]) -> None:
        self.suffixes.update(cfg.get("suffixes", {}))

    def set_market_suffixes(self, suffixes: dict[str, str]) -> None:
        """Apply config-backed exchange profiles after plugin configuration."""
        self.suffixes.update(suffixes)

    def yf_symbol(self, inst: Instrument) -> str:
        return yf_symbol(inst, self.suffixes)


def _history_bars(inst: Instrument, symbol: str, since: datetime) -> list[Bar]:
    """Blocking yfinance download plus DataFrame walk, in one thread.

    Kept separate from :meth:`YFinanceData.fetch` so the network call and the
    row conversion run together on a worker thread; the async layer never blocks
    the event loop on yfinance.
    """
    import yfinance as yf

    df = yf.Ticker(symbol).history(start=since.date(), interval="1d", auto_adjust=True)
    if df is None or df.empty:
        return []
    bars: list[Bar] = []
    for ts, open_, high, low, close, volume in df[["Open", "High", "Low", "Close", "Volume"]].itertuples(name=None):
        # FX and some thin symbols return rows with NaN prices; SQLite would
        # store them as NULL and violate the bar NOT NULL columns.
        if any(math.isnan(float(v)) for v in (open_, high, low, close)):
            continue
        bars.append(
            Bar(
                instrument_id=inst.id,
                ts=ts.to_pydatetime().astimezone(UTC),
                open=float(open_),
                high=float(high),
                low=float(low),
                close=float(close),
                volume=float(volume),
                source="yfinance",
            )
        )
    return bars


class YFinanceData(YFinanceSymbols):
    name = "yfinance"

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental | Event]:
        # Independent blocking downloads run concurrently on worker threads so a
        # slow symbol never freezes the UI or stalls the others.
        results = await asyncio.gather(
            *(asyncio.to_thread(_history_bars, inst, self.yf_symbol(inst), since) for inst in instruments),
            return_exceptions=True,
        )
        bars: list[Bar] = []
        for _inst, result in zip(instruments, results, strict=True):
            if isinstance(result, BaseException):
                continue
            bars.extend(result)
        return bars
