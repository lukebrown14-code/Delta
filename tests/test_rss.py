"""Offline tests for the RSS data plugin (respx serves a saved feed)."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime
from pathlib import Path
from typing import cast

import httpx
import respx

from delta.core.models import Instrument, NewsItem
from delta.plugins.data.rss import RSSData, news_id, strip_html

FIXTURE = Path(__file__).parent / "fixtures" / "business_feed.xml"
FEED_URL = "https://feeds.example.com/business.xml"
BAD_URL = "https://feeds.example.com/broken.xml"
SINCE = datetime(2026, 1, 1, tzinfo=UTC)

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", name="Apple Inc.", currency="USD")
MSFT = Instrument(
    id="US:MSFT", market="us", symbol="MSFT", name="Microsoft Corporation", currency="USD"
)
NVDA = Instrument(id="US:NVDA", market="us", symbol="NVDA", name=None, currency="USD")
UNIVERSE = [AAPL, MSFT, NVDA]


def _plugin(*feeds: str) -> RSSData:
    plugin = RSSData()
    plugin.configure({"feeds": list(feeds) or [FEED_URL]})
    return plugin


def _fetch(plugin: RSSData, since: datetime = SINCE) -> list[NewsItem]:
    out = asyncio.run(plugin.fetch(UNIVERSE, since))
    assert all(isinstance(x, NewsItem) for x in out)
    return cast(list[NewsItem], out)


def _by_url(items: list[NewsItem]) -> dict[str, NewsItem]:
    return {i.url: i for i in items}


def _serve_fixture(url: str = FEED_URL) -> None:
    respx.get(url).mock(return_value=httpx.Response(200, content=FIXTURE.read_bytes()))


@respx.mock
def test_item_count_and_fields():
    _serve_fixture()
    items = _fetch(_plugin())
    # Six entries in the fixture; the 2020 archive story is older than `since`.
    assert len(items) == 5
    apple = _by_url(items)["https://example.com/news/apple-event"]
    assert apple.source == "rss"
    assert apple.title == "Apple unveils new iPhone line-up at September event"
    assert apple.published == datetime(2026, 3, 14, 9, 0, tzinfo=UTC)


@respx.mock
def test_body_is_html_stripped():
    _serve_fixture()
    apple = _by_url(_fetch(_plugin()))["https://example.com/news/apple-event"]
    assert apple.body == "Apple Inc. showed off four new phones & a watch."
    assert strip_html("<p>a&nbsp;&amp;   b</p>") == "a & b"


@respx.mock
def test_instrument_matching():
    _serve_fixture()
    by_url = _by_url(_fetch(_plugin()))
    # Company name (case-insensitive first word) in title.
    assert by_url["https://example.com/news/apple-event"].instrument_ids == ["US:AAPL"]
    # Whole-word ticker in body only.
    assert by_url["https://example.com/news/cloud-spend"].instrument_ids == ["US:MSFT"]
    # Ticker for an instrument with no name, plus a name match, in one story.
    assert set(by_url["https://example.com/news/chip-rally"].instrument_ids) == {
        "US:MSFT",
        "US:NVDA",
    }


@respx.mock
def test_macro_item_kept_with_empty_match():
    _serve_fixture()
    fed = _by_url(_fetch(_plugin()))["https://example.com/news/fed-holds"]
    assert fed.instrument_ids == []


@respx.mock
def test_id_is_stable_and_derived_from_link_and_published():
    _serve_fixture()
    first = _by_url(_fetch(_plugin()))["https://example.com/news/apple-event"]
    second = _by_url(_fetch(_plugin()))["https://example.com/news/apple-event"]
    assert first.id == second.id
    assert first.id == news_id("https://example.com/news/apple-event", first.published)


@respx.mock
def test_since_filter_and_missing_date_falls_back_to_now():
    _serve_fixture()
    before = datetime.now(UTC)
    items = _fetch(_plugin(), since=datetime(2026, 3, 12, tzinfo=UTC))
    assert set(_by_url(items)) == {
        "https://example.com/news/apple-event",
        "https://example.com/news/cloud-spend",
        "https://example.com/news/fed-holds",
        "https://example.com/news/undated",
    }
    undated = _by_url(items)["https://example.com/news/undated"]
    assert before <= undated.published <= datetime.now(UTC)


@respx.mock
def test_failing_feed_is_skipped_without_raising():
    respx.get(BAD_URL).mock(return_value=httpx.Response(500))
    _serve_fixture()
    items = _fetch(_plugin(BAD_URL, FEED_URL))
    assert len(items) == 5


@respx.mock
def test_same_story_in_two_feeds_is_deduplicated():
    mirror = "https://feeds.example.com/mirror.xml"
    _serve_fixture()
    _serve_fixture(mirror)
    items = _fetch(_plugin(FEED_URL, mirror))
    assert len(items) == 5
