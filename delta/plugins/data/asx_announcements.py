"""ASX company announcements data plugin.

Pulls the announcements feed ASX serves through Markit Digital (the old
``asx.com.au/asx/1`` API was retired) and turns each announcement into a
:class:`NewsItem`. Price-sensitive announcements are marked with a ``[PS] ``
title prefix until ``NewsItem`` grows a dedicated flag.
"""

from __future__ import annotations

import asyncio
import logging
from datetime import UTC, datetime
from typing import Any

import httpx

from delta.core.http import user_agent
from delta.core.ids import stable_id
from delta.core.models import Bar, Event, Fundamental, Instrument, NewsItem
from delta.core.plugin import DataPlugin
from delta.core.time import to_utc

log = logging.getLogger(__name__)

BASE_URL = "https://asx.api.markitdigital.com/asx-research/1.0/companies/{code}/announcements"
# The API returns no per-document URL; this page lists the company's announcements
# and the fragment keeps the link unique per document.
PAGE_URL = "https://www.asx.com.au/markets/trade-our-cash-market/announcements.{code}#{key}"
PS_PREFIX = "[PS] "
RETRY_STATUSES = {429, 500, 502, 503, 504}
#: Cap concurrent announcement fetches so a full watchlist does not fan out
#: without bound against Markit Digital.
MAX_CONCURRENCY = 4


def announcement_id(document_key: str) -> str:
    """Stable id from ASX's own document key."""
    return stable_id("asx_announcement", document_key)


class ASXAnnouncements(DataPlugin):
    name = "asx_announcements"
    market = "asx"

    def __init__(self) -> None:
        self.count = 20
        self.timeout = 20.0
        self.max_retries = 3
        self.backoff_seconds = 1.0
        self.user_agent = user_agent("+https://github.com/lukebrown14-code/Delta")
        self._semaphore = asyncio.Semaphore(MAX_CONCURRENCY)

    def configure(self, cfg: dict[str, Any]) -> None:
        self.count = int(cfg.get("count", self.count))
        self.timeout = float(cfg.get("timeout", self.timeout))
        self.max_retries = int(cfg.get("max_retries", self.max_retries))
        self.backoff_seconds = float(cfg.get("backoff_seconds", self.backoff_seconds))
        self.user_agent = str(cfg.get("user_agent", self.user_agent))

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental | Event]:
        since_utc = to_utc(since)
        targets = [inst for inst in instruments if inst.market == self.market]
        headers = {"User-Agent": self.user_agent, "Accept": "application/json"}
        async with httpx.AsyncClient(timeout=self.timeout, headers=headers) as client:
            payloads = await asyncio.gather(
                *(self._get(client, inst.symbol, since_utc) for inst in targets)
            )
        items: list[Bar | NewsItem | Fundamental | Event] = []
        for inst, rows in zip(targets, payloads, strict=True):
            if rows:
                items.extend(self._to_news(inst, rows, since_utc))
        return items

    async def _get(
        self, client: httpx.AsyncClient, code: str, since: datetime
    ) -> list[dict[str, Any]] | None:
        """Announcement rows for one code, or None when the request ultimately failed."""
        async with self._semaphore:
            return await self._get_one(client, code, since)

    async def _get_one(
        self, client: httpx.AsyncClient, code: str, since: datetime
    ) -> list[dict[str, Any]] | None:
        url = BASE_URL.format(code=code.lower())
        params: dict[str, str | int] = {
            "fromDate": since.date().isoformat(),
            "toDate": datetime.now(UTC).date().isoformat(),
            "itemsPerPage": self.count,
            "page": 0,
        }
        for attempt in range(self.max_retries + 1):
            try:
                resp = await client.get(url, params=params)
            except httpx.HTTPError as exc:
                log.warning("asx_announcements: %s request failed: %s", code, exc)
                if attempt >= self.max_retries:
                    return None
                await asyncio.sleep(self._backoff(attempt))
                continue
            if resp.status_code == 404:
                log.info("asx_announcements: %s not found (404); skipping", code)
                return None
            if resp.status_code in RETRY_STATUSES and attempt < self.max_retries:
                delay = self._retry_delay(resp, attempt)
                log.info(
                    "asx_announcements: %s got %s; retrying in %.1fs", code, resp.status_code, delay
                )
                await asyncio.sleep(delay)
                continue
            if resp.status_code != 200:
                log.warning("asx_announcements: %s returned %s; skipping", code, resp.status_code)
                return None
            try:
                data = resp.json()
            except ValueError:
                log.warning("asx_announcements: %s returned non-JSON body; skipping", code)
                return None
            rows = data.get("data", {}).get("items") if isinstance(data, dict) else None
            return [row for row in rows or [] if isinstance(row, dict)]
        return None

    def _backoff(self, attempt: int) -> float:
        return float(self.backoff_seconds * (2**attempt))

    def _retry_delay(self, resp: httpx.Response, attempt: int) -> float:
        retry_after = resp.headers.get("Retry-After")
        if retry_after:
            try:
                return max(float(retry_after), 0.0)
            except ValueError:
                pass
        return self._backoff(attempt)

    def _to_news(
        self, inst: Instrument, rows: list[dict[str, Any]], since: datetime
    ) -> list[NewsItem]:
        out: list[NewsItem] = []
        for row in rows:
            key = row.get("documentKey")
            raw_published = row.get("date")
            if not key or not raw_published:
                continue
            try:
                published = to_utc(datetime.fromisoformat(str(raw_published)))
            except ValueError:
                log.warning(
                    "asx_announcements: %s bad timestamp %r; skipping", inst.symbol, raw_published
                )
                continue
            if published < since:
                continue
            title = str(row.get("headline") or "").strip() or "Untitled announcement"
            if bool(row.get("isPriceSensitive")):
                title = f"{PS_PREFIX}{title}"
            url = str(row.get("url") or "") or PAGE_URL.format(code=inst.symbol.lower(), key=key)
            out.append(
                NewsItem(
                    id=announcement_id(str(key)),
                    instrument_ids=[inst.id],
                    published=published,
                    title=title,
                    url=url,
                    body=None,
                    source=self.name,
                )
            )
        return out
