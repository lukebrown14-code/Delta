"""yfinance data plugin: historical bars and (Phase 2) fundamentals/news."""

from __future__ import annotations

from datetime import UTC, datetime

from rigger.core.models import Bar, Fundamental, Instrument, NewsItem
from rigger.core.plugin import DataPlugin


class YFinanceData(DataPlugin):
    name = "yfinance"
    market = "us"

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental]:
        import yfinance as yf

        bars: list[Bar] = []
        for inst in instruments:
            ticker = yf.Ticker(inst.symbol)
            df = ticker.history(start=since.date(), interval="1d", auto_adjust=True)
            if df is None or df.empty:
                continue
            for ts, row in df.iterrows():
                bars.append(
                    Bar(
                        instrument_id=inst.id,
                        ts=ts.to_pydatetime().replace(tzinfo=UTC),
                        open=float(row["Open"]),
                        high=float(row["High"]),
                        low=float(row["Low"]),
                        close=float(row["Close"]),
                        volume=float(row["Volume"]),
                        source=self.name,
                    )
                )
        return bars
