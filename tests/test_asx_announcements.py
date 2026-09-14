"""Offline tests for the ASX announcements data plugin (HTTP via respx)."""

from __future__ import annotations

import asyncio
import json
from datetime import UTC, datetime
from pathlib import Path
from typing import cast

import httpx
import pytest
import respx

from rigger.core.models import Instrument, NewsItem
from rigger.plugins.data.asx_announcements import ASXAnnouncements, announcement_id

FIXTURE = Path(__file__).parent / "fixtures" / "asx_announcements_bhp.json"
BHP_URL = "https://www.asx.com.au/asx/1/company/BHP/announcements"
CBA_URL = "https://www.asx.com.au/asx/1/company/CBA/announcements"

BHP = Instrument(id="ASX:BHP", market="asx", symbol="BHP", currency="AUD")
CBA = Instrument(id="ASX:CBA", market="asx", symbol="CBA", currency="AUD")
AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")

SINCE_ALL = datetime(2026, 8, 1, tzinfo=UTC)


@pytest.fixture
def payload() -> dict:
    return json.loads(FIXTURE.read_text())


@pytest.fixture
def plugin() -> ASXAnnouncements:
    p = ASXAnnouncements()
    p.configure({"enabled": True, "backoff_seconds": 0.0, "max_retries": 2})
    return p


def _run(
    plugin: ASXAnnouncements, instruments: list[Instrument], since: datetime
) -> list[NewsItem]:
    items = asyncio.run(plugin.fetch(instruments, since))
    assert all(isinstance(i, NewsItem) for i in items)
    return cast(list[NewsItem], items)


@respx.mock
def test_fetch_maps_announcements(plugin: ASXAnnouncements, payload: dict) -> None:
    route = respx.get(BHP_URL).mock(return_value=httpx.Response(200, json=payload))

    items = _run(plugin, [BHP], SINCE_ALL)

    assert route.called
    assert route.calls.last.request.url.params["count"] == "20"
    assert route.calls.last.request.url.params["market_sensitive"] == "false"
    assert len(items) == 3
    for item in items:
        assert item.instrument_ids == ["ASX:BHP"]
        assert item.source == "asx_announcements"
        assert item.body is None
        assert item.url.endswith(".pdf")
        assert item.published.tzinfo == UTC

    # 08:31:12 +10:00 -> 22:31:12 UTC the previous day.
    first = items[0]
    assert first.published == datetime(2026, 9, 9, 22, 31, 12, tzinfo=UTC)
    assert first.title == "Change of Director's Interest Notice"


@respx.mock
def test_price_sensitive_prefix(plugin: ASXAnnouncements, payload: dict) -> None:
    respx.get(BHP_URL).mock(return_value=httpx.Response(200, json=payload))
    titles = [i.title for i in _run(plugin, [BHP], SINCE_ALL)]
    assert "[PS] Operational Review for the quarter ended 31 August 2026" in titles
    assert "Notice of Annual General Meeting" in titles
    assert sum(t.startswith("[PS] ") for t in titles) == 1


@respx.mock
def test_id_is_stable_across_fetches(plugin: ASXAnnouncements, payload: dict) -> None:
    respx.get(BHP_URL).mock(return_value=httpx.Response(200, json=payload))
    a = [i.id for i in _run(plugin, [BHP], SINCE_ALL)]
    b = [i.id for i in _run(plugin, [BHP], SINCE_ALL)]
    assert a == b
    assert len(set(a)) == 3
    row = payload["data"][0]
    expected = announcement_id(row["url"], datetime(2026, 9, 9, 22, 31, 12, tzinfo=UTC))
    assert a[0] == expected


@respx.mock
def test_since_filters_older_items(plugin: ASXAnnouncements, payload: dict) -> None:
    respx.get(BHP_URL).mock(return_value=httpx.Response(200, json=payload))
    items = _run(plugin, [BHP], datetime(2026, 9, 1, tzinfo=UTC))
    assert len(items) == 2
    assert all(i.published >= datetime(2026, 9, 1, tzinfo=UTC) for i in items)


@respx.mock
def test_404_is_skipped_gracefully(plugin: ASXAnnouncements, payload: dict) -> None:
    respx.get(BHP_URL).mock(return_value=httpx.Response(200, json=payload))
    respx.get(CBA_URL).mock(return_value=httpx.Response(404, json={"error": "not found"}))
    items = _run(plugin, [CBA, BHP], SINCE_ALL)
    assert len(items) == 3
    assert {i.instrument_ids[0] for i in items} == {"ASX:BHP"}


@respx.mock
def test_rate_limit_retries_then_succeeds(plugin: ASXAnnouncements, payload: dict) -> None:
    route = respx.get(BHP_URL).mock(
        side_effect=[
            httpx.Response(429, headers={"Retry-After": "0"}),
            httpx.Response(200, json=payload),
        ]
    )
    items = _run(plugin, [BHP], SINCE_ALL)
    assert route.call_count == 2
    assert len(items) == 3


@respx.mock
def test_non_asx_instruments_are_ignored(plugin: ASXAnnouncements) -> None:
    route = respx.get(url__regex=r"https://www\.asx\.com\.au/.*").mock(
        return_value=httpx.Response(200, json={"data": []})
    )
    assert _run(plugin, [AAPL], SINCE_ALL) == []
    assert not route.called
