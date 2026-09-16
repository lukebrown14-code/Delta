"""Ephemeral Yahoo quotes, independent of gathered daily bars."""

from __future__ import annotations

import asyncio
import json
import math
from collections.abc import Callable
from dataclasses import dataclass
from datetime import UTC, datetime
from typing import Any

from rigger.core.models import Instrument
from rigger.plugins.data.yfinance import yf_symbol


@dataclass(frozen=True)
class SearchResult:
    symbol: str
    name: str
    market: str
    currency: str
    exchange: str = ""
    asset_class: str = "equity"


def classify_yahoo_asset(symbol: str, quote_type: str) -> str:
    """Map Yahoo quote metadata to Rigger's supported asset classes."""
    value = quote_type.casefold()
    if "crypto" in value or symbol.upper().endswith("-USD"):
        return "crypto"
    if "etf" in value:
        return "etf"
    if "bond" in value or "fixed income" in value:
        return "bond"
    if "currency" in value or "forex" in value:
        return "fx"
    if "commodity" in value or "future" in value:
        return "commodity"
    if "cash" in value:
        return "cash"
    return "equity"


def canonical_symbol(symbol: str, market: str, suffixes: dict[str, str] | None = None) -> str:
    """Convert a Yahoo provider symbol to the symbol stored in a target spec."""
    symbol = symbol.strip().upper()
    suffix = (suffixes or {}).get(market.lower(), "")
    if suffix and symbol.endswith(suffix.upper()):
        return symbol[: -len(suffix)]
    return symbol


async def yahoo_search(query: str, max_results: int = 8) -> list[SearchResult]:
    """Search Yahoo's symbol directory without blocking the TUI loop."""
    import yfinance as yf

    response = await asyncio.to_thread(yf.Search, query, max_results=max_results)
    results: list[SearchResult] = []
    # Yahoo's directory can repeat a symbol; the TUI suggestion list keys
    # options by symbol, so a repeat would raise DuplicateID downstream.
    seen: set[str] = set()
    for item in response.quotes:
        symbol = str(item.get("symbol", "")).strip().upper()
        name = str(item.get("longname") or item.get("shortname") or symbol).strip()
        exchange = str(item.get("exchange") or item.get("fullExchangeName") or "").strip()
        if not symbol or symbol in seen:
            continue
        seen.add(symbol)
        exchange_code = exchange.upper()
        if "ASX" in exchange_code:
            market = "asx"
        elif exchange_code in {"NMS", "NAS", "NASDAQ", "NYQ", "NYSE", "ASE", "ARCA", "BTS"}:
            market = "us"
        else:
            # Keep unknown listings usable as US results for compatibility with
            # Yahoo's incomplete exchange metadata; the UI can still be edited.
            market = "us"
        currency = str(item.get("currency") or ("AUD" if market == "asx" else "USD"))
        quote_type = str(item.get("quoteType") or item.get("typeDisp") or "").casefold()
        asset_class = classify_yahoo_asset(symbol, quote_type)
        results.append(SearchResult(symbol, name, market, currency, exchange, asset_class))
    return results


@dataclass(frozen=True)
class Quote:
    price: float
    currency: str
    change_pct: float | None
    timestamp: datetime
    received_at: datetime


def parse_quote(message: dict, currency: str) -> Quote | None:
    try:
        price = float(message["price"])
        timestamp = datetime.fromtimestamp(float(message["time"]) / 1000, UTC)
        change = message.get("change_percent")
        change = float(change) if change is not None else None
        if not math.isfinite(price) or price <= 0:
            return None
        if change is not None and not math.isfinite(change):
            change = None
        return Quote(
            price, message.get("currency") or currency, change, timestamp, datetime.now(UTC)
        )
    except (KeyError, TypeError, ValueError, OverflowError, OSError):
        return None


class YahooQuotes:
    def __init__(
        self,
        instruments: list[Instrument],
        suffixes: dict[str, str],
        on_state: Callable[[str], None],
        client_factory: Any = None,
    ) -> None:
        self.symbols = {yf_symbol(inst, suffixes): inst for inst in instruments}
        self.quotes: dict[str, Quote] = {}
        self.on_state = on_state
        self.client_factory = client_factory

    def receive(self, message: dict) -> None:
        inst = self.symbols.get(message.get("id"))
        if inst is None:
            return
        quote = parse_quote(message, inst.currency)
        previous = self.quotes.get(inst.id)
        if quote and (previous is None or quote.timestamp >= previous.timestamp):
            self.quotes[inst.id] = quote

    async def run(self) -> None:
        if not self.symbols:
            self.on_state("no symbols")
            return
        from yfinance import AsyncWebSocket

        factory = self.client_factory or AsyncWebSocket
        delay = 1
        while True:
            self.on_state("connecting" if delay == 1 else "reconnecting")
            try:
                async with factory(verbose=False) as client:
                    await client.subscribe(list(self.symbols))
                    self.on_state("connected")
                    # yfinance.listen swallows disconnects and cancellation. Keep
                    # its subscription/heartbeat/decoder, but own the receive loop
                    # so lifecycle and reconnect state remain observable to the UI.
                    async for raw in client._ws:
                        try:
                            message = client._decode_message(json.loads(raw).get("message", ""))
                            # Proto3 omits default numeric fields in JSON; on
                            # this wire format an omitted daily change is zero.
                            message.setdefault("change_percent", 0.0)
                            self.receive(message)
                            delay = 1
                        except (ValueError, TypeError, AttributeError):
                            continue
                    raise ConnectionError("quote stream closed")
            except asyncio.CancelledError:
                raise
            except Exception:
                self.on_state("disconnected · retrying")
                await asyncio.sleep(delay)
                delay = min(delay * 2, 30)
