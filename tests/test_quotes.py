"""Quote normalization and stream lifecycle without external connections."""

import asyncio
import json
from datetime import UTC, datetime

import pytest

from rigger.core.models import Instrument
from rigger.quotes import YahooQuotes, classify_yahoo_asset, parse_quote, yahoo_search


@pytest.mark.parametrize(
    ("quote_type", "expected"),
    [
        ("EQUITY", "equity"),
        ("ETF", "etf"),
        ("BOND", "bond"),
        ("CURRENCY", "fx"),
        ("FUTURE", "commodity"),
        ("CRYPTOCURRENCY", "crypto"),
    ],
)
def test_classify_yahoo_asset(quote_type, expected):
    assert classify_yahoo_asset("TEST", quote_type) == expected


def message(**overrides):
    return dict(id="BHP.AX", price=42.18, time="1789516800000", change_percent=1.2, **overrides)


@pytest.mark.parametrize("change", [1.2, -0.4, 0, None, float("nan")])
def test_quote_change(change):
    payload = message()
    payload["change_percent"] = change
    quote = parse_quote(payload, "AUD")
    assert quote.currency == "AUD"
    assert quote.change_pct == (None if change is None or str(change) == "nan" else change)
    assert quote.timestamp == datetime.fromtimestamp(1789516800, UTC)


@pytest.mark.parametrize(
    "field,value",
    [
        ("price", float("nan")),
        ("price", -1),
        ("time", "bad"),
        ("time", float("inf")),
        ("price", None),
    ],
)
def test_invalid_quote(field, value):
    payload = message()
    payload[field] = value
    assert parse_quote(payload, "AUD") is None


def test_yahoo_search_normalizes_exchange_and_currency(monkeypatch):
    import yfinance

    class Search:
        def __init__(self, query, max_results):
            assert query == "BHP"
            assert max_results == 8
            self.quotes = [
                {
                    "symbol": "BHP.AX",
                    "longname": "BHP Group Limited",
                    "exchange": "ASX",
                    "currency": "AUD",
                },
                {"symbol": "AAPL", "shortname": "Apple Inc.", "exchange": "NMS"},
                {
                    "symbol": "BTC-USD",
                    "shortname": "Bitcoin USD",
                    "exchange": "CCC",
                    "quoteType": "CRYPTOCURRENCY",
                },
            ]

    monkeypatch.setattr(yfinance, "Search", Search)
    results = asyncio.run(yahoo_search("BHP"))
    assert results[0].market == "asx"
    assert results[0].currency == "AUD"
    assert results[1].market == "us"
    assert results[1].currency == "USD"
    assert results[2].asset_class == "crypto"


def instrument():
    return Instrument(id="ASX:BHP", market="asx", symbol="BHP", currency="AUD")


def test_deduplication_and_out_of_order():
    feed = YahooQuotes([instrument(), instrument()], {"asx": ".AX"}, lambda _: None)
    assert list(feed.symbols) == ["BHP.AX"]
    feed.receive(message())
    older = message()
    older.update(time="1789516700000", price=10)
    feed.receive(older)
    feed.receive({"id": "UNKNOWN"})
    assert feed.quotes["ASX:BHP"].price == 42.18


def test_stream_reconnect_and_cancellation():
    async def run():
        states, clients = [], []
        ready = asyncio.Event()

        class Client:
            def __init__(self, **kwargs):
                self.closed = False
                self._ws = self
                clients.append(self)

            async def __aenter__(self):
                return self

            async def __aexit__(self, *args):
                self.closed = True

            async def subscribe(self, symbols):
                assert symbols == ["BHP.AX"]

            def __aiter__(self):
                return self

            async def __anext__(self):
                if len(clients) == 1:
                    raise ConnectionError()
                if not ready.is_set():
                    ready.set()
                    return json.dumps({"message": message()})
                await asyncio.Event().wait()

            def _decode_message(self, payload):
                return payload

        feed = YahooQuotes([instrument()], {"asx": ".AX"}, states.append, Client)
        task = asyncio.create_task(feed.run())
        await asyncio.wait_for(ready.wait(), 3)
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        assert "disconnected · retrying" in states
        assert feed.quotes["ASX:BHP"].price == 42.18
        assert all(client.closed for client in clients)

    asyncio.run(run())


def test_yahoo_wire_flat_quote_and_cleanup():
    import base64

    from yfinance import AsyncWebSocket
    from yfinance.pricing_pb2 import PricingData

    async def run():
        ready = asyncio.Event()
        closed = []
        payload = PricingData(id="BHP.AX", price=42, time=1789516800000, change_percent=0)
        wire = json.dumps({"message": base64.b64encode(payload.SerializeToString()).decode()})

        class Client(AsyncWebSocket):
            async def __aenter__(self):
                self._ws = self.messages()
                return self

            async def __aexit__(self, *args):
                closed.append(True)

            async def subscribe(self, symbols):
                pass

            async def messages(self):
                yield wire
                ready.set()
                await asyncio.Event().wait()

        feed = YahooQuotes([instrument()], {"asx": ".AX"}, lambda _: None, Client)
        task = asyncio.create_task(feed.run())
        await asyncio.wait_for(ready.wait(), 2)
        assert feed.quotes["ASX:BHP"].change_pct == 0
        task.cancel()
        with pytest.raises(asyncio.CancelledError):
            await task
        assert closed == [True]

    asyncio.run(run())
