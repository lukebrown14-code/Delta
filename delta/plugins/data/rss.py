"""RSS data plugin: business news feeds -> NewsItem rows.

Feeds come from ``[plugins.rss].feeds``. Each entry becomes a NewsItem matched
to universe instruments by whole-word ticker or the first word of the company
name. Items that match nothing are kept with ``instrument_ids=[]`` so macro
news is still stored.
"""

from __future__ import annotations

import asyncio
import html
import logging
import re
from calendar import timegm
from datetime import UTC, datetime
from typing import Any

import feedparser
import httpx

from delta.core.http import user_agent
from delta.core.ids import stable_id
from delta.core.models import Bar, Event, Fundamental, Instrument, NewsItem
from delta.core.plugin import DataPlugin
from delta.core.time import to_utc

log = logging.getLogger(__name__)

_TAG_RE = re.compile(r"<[^>]+>")
_WS_RE = re.compile(r"\s+")

#: Cap concurrent feed requests so a large feed list does not fan out unbounded.
MAX_CONCURRENCY = 4


def strip_html(text: str) -> str:
    """Drop tags, unescape entities and collapse whitespace."""
    return _WS_RE.sub(" ", html.unescape(_TAG_RE.sub(" ", text))).strip()


def news_id(link: str, published: datetime) -> str:
    return stable_id(link, published.isoformat())


class _Matcher:
    """Compiled per-instrument patterns: ``\\bAAPL\\b`` and the first name token."""

    def __init__(self, instruments: list[Instrument]) -> None:
        self._patterns: list[tuple[str, list[re.Pattern[str]]]] = []
        for inst in instruments:
            pats: list[re.Pattern[str]] = []
            if inst.symbol:
                # Tickers are upper-case in prose; a case-insensitive match on
                # short symbols such as "F" or "IT" would hit ordinary words.
                pats.append(re.compile(rf"\b{re.escape(inst.symbol)}\b"))
            words = (inst.name or "").split()
            if words:
                token = words[0].strip(".,;:()'\"")
                if token:
                    pats.append(re.compile(rf"\b{re.escape(token)}\b", re.IGNORECASE))
            if pats:
                self._patterns.append((inst.id, pats))

    def match(self, text: str) -> list[str]:
        return [iid for iid, pats in self._patterns if any(p.search(text) for p in pats)]


def _published(entry: Any, fallback: datetime) -> datetime:
    for key in ("published_parsed", "updated_parsed"):
        parsed = entry.get(key)
        if parsed:
            return datetime.fromtimestamp(timegm(parsed), tz=UTC)
    return fallback


def parse_feed(
    raw: bytes,
    instruments: list[Instrument] | _Matcher,
    since: datetime,
    now: datetime | None = None,
) -> list[NewsItem]:
    """Turn feed bytes into NewsItems published at or after ``since``.

    ``now`` is the timestamp given to undated entries; pass one value for a
    whole fetch so the same undated story in two feeds gets the same id.
    ``instruments`` may be a pre-built ``_Matcher`` to avoid recompiling per feed.
    """
    since = to_utc(since)
    now = now or datetime.now(UTC)
    matcher = instruments if isinstance(instruments, _Matcher) else _Matcher(instruments)
    parsed = feedparser.parse(raw)
    items: list[NewsItem] = []
    for entry in parsed.entries:
        link = str(entry.get("link") or "").strip()
        if not link:
            continue
        published = _published(entry, now)
        if published < since:
            continue
        title = strip_html(str(entry.get("title") or ""))
        body = strip_html(str(entry.get("summary") or "")) or None
        items.append(
            NewsItem(
                id=news_id(link, published),
                instrument_ids=matcher.match(f"{title}\n{body or ''}"),
                published=published,
                title=title,
                url=link,
                body=body,
                source="rss",
            )
        )
    return items


class RSSData(DataPlugin):
    name = "rss"
    market = None

    def __init__(self) -> None:
        self.feeds: list[str] = []
        self.timeout: float = 20.0
        self._semaphore = asyncio.Semaphore(MAX_CONCURRENCY)

    def configure(self, cfg: dict[str, Any]) -> None:
        self.feeds = [str(f) for f in cfg.get("feeds", [])]
        self.timeout = float(cfg.get("timeout", self.timeout))

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental | Event]:
        items: dict[str, NewsItem] = {}
        now = datetime.now(UTC)
        matcher = _Matcher(instruments)
        async with httpx.AsyncClient(
            timeout=self.timeout,
            follow_redirects=True,
            headers={"User-Agent": user_agent("research harness")},
        ) as client:
            feeds = await asyncio.gather(*(self._get(client, url) for url in self.feeds))
        for raw in feeds:
            if raw is None:
                continue
            for item in parse_feed(raw, matcher, since, now):
                # The same story syndicated in two feeds has the same id.
                items.setdefault(item.id, item)
        return list(items.values())

    async def _get(self, client: httpx.AsyncClient, url: str) -> bytes | None:
        async with self._semaphore:
            try:
                resp = await client.get(url)
                resp.raise_for_status()
            except httpx.HTTPError as exc:
                # One dead feed must not abort the whole ingest.
                log.warning("rss: skipping feed %s: %s", url, exc)
                return None
            return resp.content
