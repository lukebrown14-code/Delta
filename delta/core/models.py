"""Pydantic v2 domain models for Delta.

These are the canonical in-memory representations. Persisted versions are
SQLModel tables in :mod:`delta.core.db` with the same field names.
"""

from __future__ import annotations

from datetime import date, datetime
from typing import Any, Literal

from pydantic import BaseModel, Field

#: Instrument id prefix format: "<MARKET>:<SYMBOL>", e.g. "US:AAPL".
InstrumentId = str
AssetClass = Literal["equity", "etf", "bond", "fx", "commodity", "crypto", "cash", "other"]


class Instrument(BaseModel):
    id: str  # "US:AAPL", "ASX:BHP", "CRYPTO:BTC-USD"
    market: str  # plugin name: "us", "asx", "crypto"
    symbol: str
    name: str | None = None
    currency: str
    sector: str | None = None
    asset_class: AssetClass = "equity"
    watchlists: tuple[str, ...] = ()
    tags: frozenset[str] = frozenset()
    industry: str | None = None
    meta: dict[str, Any] = Field(default_factory=dict)

    @property
    def sector_name(self) -> str:
        return self.sector or "unknown"


class Bar(BaseModel):
    instrument_id: str
    ts: datetime  # bar close, UTC
    open: float
    high: float
    low: float
    close: float
    volume: float
    source: str  # data plugin name


class NewsItem(BaseModel):
    id: str  # hash of url + published
    instrument_ids: list[str] = Field(default_factory=list)
    published: datetime
    title: str
    url: str
    body: str | None = None
    source: str


EventKind = Literal[
    "earnings",
    "guidance",
    "dividend",
    "insider_trade",
    "m&a",
    "regulatory",
    "macro",
    "other",
]


class Event(BaseModel):
    id: str
    instrument_id: str
    ts: datetime
    kind: EventKind
    summary: str
    sentiment: float = Field(ge=-1, le=1)
    evidence_ids: list[str] = Field(default_factory=list)
    extracted_by: str  # model id
    prompt_version: str


class Fundamental(BaseModel):
    instrument_id: str
    as_of: date
    metric: str
    value: float
    source: str


class LLMCall(BaseModel):
    id: str
    ts: datetime
    task: str
    model: str
    prompt_version: str
    prompt_hash: str
    input_tokens: int
    output_tokens: int
    cost_usd: float
    latency_ms: int
    cached: bool
