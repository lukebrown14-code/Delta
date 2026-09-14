"""ASX company announcements data plugin.

Pulls the public announcements feed for each ASX instrument and turns each
announcement into a :class:`NewsItem` pointing at the PDF. Price-sensitive
announcements are marked with a ``[PS] `` title prefix until ``NewsItem`` grows
a dedicated flag.
"""

from __future__ import annotations

import asyncio
import hashlib
import logging
from datetime import UTC, datetime
from typing import Any

import httpx

from rigger.core.models import Bar, Fundamental, Instrument, NewsItem
from rigger.core.plugin import DataPlugin

log = logging.getLogger(__name__)

BASE_URL = "https://www.asx.com.au/asx/1/company/{code}/announcements"
PS_PREFIX = "[PS] "
RETRY_STATUSES = {429, 500, 502, 503, 504}


def announcement_id(url: str, published: datetime) -> str:
    """Stable id: sha256(url + published) with ``published`` normalised to UTC ISO-8601."""
    key = f"{url}{published.astimezone(UTC).isoformat()}"
    return hashlib.sha256(key.encode("utf-8")).hexdigest()


def _parse_published(raw: str) -> datetime:
    """Parse the ASX timestamp (ISO-8601 with a ``+1000``-style offset) to UTC."""
    text = raw.strip().replace("Z", "+00:00")
    # ASX emits offsets without a colon (``+1000``); fromisoformat wants ``+10:00``.
    if len(text) >= 5 and text[-5] in "+-" and text[-3] != ":" and text[-4:].isdigit():
        text = f"{text[:-2]}:{text[-2:]}"
    dt = datetime.fromisoformat(text)
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=UTC)
    return dt.astimezone(UTC)


class ASXAnnouncements(DataPlugin):
    name = "asx_announcements"
    market = "asx"

    def __init__(self) -> None:
        self.count = 20
        self.timeout = 20.0
        self.max_retries = 3
        self.backoff_seconds = 1.0
        self.user_agent = "Rigger/0.1 (+https://codeberg.org/LukeBro14/Rigger)"

    def configure(self, cfg: dict[str, Any]) -> None:
        self.count = int(cfg.get("count", self.count))
        self.timeout = float(cfg.get("timeout", self.timeout))
        self.max_retries = int(cfg.get("max_retries", self.max_retries))
        self.backoff_seconds = float(cfg.get("backoff_seconds", self.backoff_seconds))
        self.user_agent = str(cfg.get("user_agent", self.user_agent))

    async def fetch(
        self, instruments: list[Instrument], since: datetime
    ) -> list[Bar | NewsItem | Fundamental]:
        since_utc = since.astimezone(UTC) if since.tzinfo else since.replace(tzinfo=UTC)
        items: list[Bar | NewsItem | Fundamental] = []
        headers = {"User-Agent": self.user_agent, "Accept": "application/json"}
        async with httpx.AsyncClient(timeout=self.timeout, headers=headers) as client:
            for inst in instruments:
                if inst.market != self.market:
                    continue
                payload = await self._get(client, inst.symbol)
                if payload is None:
                    continue
                items.extend(self._to_news(inst, payload, since_utc))
        return items

    async def _get(self, client: httpx.AsyncClient, code: str) -> dict[str, Any] | None:
        url = BASE_URL.format(code=code.upper())
        params: dict[str, str | int] = {"count": self.count, "market_sensitive": "false"}
        for attempt in range(self.max_retries + 1):
            try:
                resp = await client.get(url, params=params)
            except httpx.HTTPError as exc:
                log.warning("asx_announcements: %s request failed: %s", code, exc)
                if attempt >= self.max_retries:
                    return None
                await asyncio.sleep(self.backoff_seconds * (2**attempt))
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
            return data if isinstance(data, dict) else {"data": data}
        return None

    def _retry_delay(self, resp: httpx.Response, attempt: int) -> float:
        retry_after = resp.headers.get("Retry-After")
        if retry_after:
            try:
                return max(float(retry_after), 0.0)
            except ValueError:
                pass
        return float(self.backoff_seconds * (2**attempt))

    def _to_news(
        self, inst: Instrument, payload: dict[str, Any], since: datetime
    ) -> list[NewsItem]:
        out: list[NewsItem] = []
        for row in payload.get("data", []) or []:
            if not isinstance(row, dict):
                continue
            url = row.get("url") or row.get("relative_url")
            raw_published = row.get("document_release_date") or row.get("document_date")
            if not url or not raw_published:
                continue
            try:
                published = _parse_published(str(raw_published))
            except ValueError:
                log.warning(
                    "asx_announcements: %s bad timestamp %r; skipping", inst.symbol, raw_published
                )
                continue
            if published < since:
                continue
            title = str(row.get("header") or "").strip() or "Untitled announcement"
            if bool(row.get("market_sensitive")):
                title = f"{PS_PREFIX}{title}"
            out.append(
                NewsItem(
                    id=announcement_id(str(url), published),
                    instrument_ids=[inst.id],
                    published=published,
                    title=title,
                    url=str(url),
                    body=None,
                    source=self.name,
                )
            )
        return out
