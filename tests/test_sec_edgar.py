"""Offline tests for the SEC EDGAR data plugin using respx fixtures."""

from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime
from pathlib import Path

import httpx
import pytest
import respx

from delta.core.http import user_agent
from delta.core.ids import stable_id
from delta.core.models import Fundamental, Instrument, NewsItem
from delta.plugins.data.sec_edgar import (
    COMPANYFACTS_URL,
    SUBMISSIONS_URL,
    TICKERS_URL,
    SECEdgar,
)

FIXTURES = Path(__file__).parent / "fixtures" / "edgar"
CIK = "0000320193"
AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")
SINCE = datetime(2025, 5, 1, tzinfo=UTC)


def _load(name: str) -> dict:
    return json.loads((FIXTURES / name).read_text())


@pytest.fixture
def edgar():
    with respx.mock(assert_all_called=False) as mock:
        mock.get(TICKERS_URL).mock(
            return_value=httpx.Response(200, json=_load("company_tickers.json"))
        )
        mock.get(SUBMISSIONS_URL.format(cik=CIK)).mock(
            return_value=httpx.Response(200, json=_load(f"submissions_CIK{CIK}.json"))
        )
        mock.get(COMPANYFACTS_URL.format(cik=CIK)).mock(
            return_value=httpx.Response(200, json=_load(f"companyfacts_CIK{CIK}.json"))
        )
        yield mock


def _fetch(plugin: SECEdgar, instruments: list[Instrument], since: datetime = SINCE):
    return asyncio.run(plugin.fetch(instruments, since))


def test_cik_lookup_and_user_agent(edgar):
    plugin = SECEdgar()
    plugin.configure({"contact": "ops@example.com"})
    _fetch(plugin, [AAPL])

    assert plugin._cik_by_symbol == {"AAPL": "0000320193", "MSFT": "0000789019"}
    assert edgar.calls.call_count == 3
    for call in edgar.calls:
        assert call.request.headers["User-Agent"] == user_agent("ops@example.com")


def test_filings_filtered_by_form_and_date(edgar):
    items = [x for x in _fetch(SECEdgar(), [AAPL]) if isinstance(x, NewsItem)]
    titles = sorted(i.title for i in items)

    # SC 13G/A excluded by form; the 2024-11-01 10-K excluded by date.
    assert titles == [
        "10-Q: 10-Q",
        "10-Q: 10-Q",
        "4: insider transaction report (Form 4)",
        "8-K: 8-K",
    ]
    assert all(i.instrument_ids == ["US:AAPL"] for i in items)
    assert all(i.source == "sec_edgar" and i.body is None for i in items)
    assert all(i.published.tzinfo is UTC for i in items)
    assert min(i.published for i in items) == datetime(2025, 5, 2, tzinfo=UTC)


def test_filing_url_and_id(edgar):
    items = {i.title: i for i in _fetch(SECEdgar(), [AAPL]) if isinstance(i, NewsItem)}
    eight_k = items["8-K: 8-K"]

    assert eight_k.url == (
        "https://www.sec.gov/Archives/edgar/data/320193/000032019325000071/aapl-20250731.htm"
    )
    assert eight_k.id == stable_id("0000320193-25-000071")
    assert eight_k.published == datetime(2025, 7, 31, tzinfo=UTC)


def test_fundamentals_latest_annual_and_quarterly(edgar):
    funds = {f.metric: f for f in _fetch(SECEdgar(), [AAPL]) if isinstance(f, Fundamental)}

    # Revenues falls back to RevenueFromContractWithCustomerExcludingAssessedTax.
    assert funds["Revenues_FY"].value == 391035000000
    assert str(funds["Revenues_FY"].as_of) == "2024-09-28"
    # Latest 10-Q by end date, single-quarter duration rather than year-to-date.
    assert funds["Revenues_Q3"].value == 94036000000
    assert str(funds["Revenues_Q3"].as_of) == "2025-06-28"
    assert funds["NetIncomeLoss_FY"].value == 93736000000
    assert funds["NetIncomeLoss_Q3"].value == 23434000000
    assert funds["EarningsPerShareDiluted_FY"].value == 6.08
    assert funds["EarningsPerShareDiluted_Q3"].value == 1.57
    # Shares outstanding from the dei namespace (instant facts, no start).
    assert funds["CommonStockSharesOutstanding_FY"].value == 15115823000
    assert funds["CommonStockSharesOutstanding_Q3"].value == 14840390000
    assert len(funds) == 8
    assert all(f.instrument_id == "US:AAPL" and f.source == "sec_edgar" for f in funds.values())


def test_unknown_ticker_and_non_us_skipped(edgar, caplog):
    unknown = Instrument(id="US:ZZZZ", market="us", symbol="ZZZZ", currency="USD")
    asx = Instrument(id="ASX:BHP", market="asx", symbol="BHP", currency="AUD")

    with caplog.at_level("WARNING"):
        out = _fetch(SECEdgar(), [unknown, asx])

    assert out == []
    assert edgar.calls.call_count == 1  # only company_tickers.json
    assert "ZZZZ" in caplog.text


def test_company_tickers_cached_across_fetches(edgar):
    plugin = SECEdgar()
    _fetch(plugin, [AAPL])
    _fetch(plugin, [AAPL])
    assert edgar.get(TICKERS_URL).call_count == 1
