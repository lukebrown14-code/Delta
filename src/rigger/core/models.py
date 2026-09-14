"""Pydantic v2 domain models for Rigger.

These are the canonical in-memory representations. Persisted versions are
SQLModel tables in :mod:`rigger.core.db` with the same field names.
"""

from __future__ import annotations

from datetime import date, datetime
from typing import Literal

from pydantic import BaseModel, Field

#: Instrument id prefix format: "<MARKET>:<SYMBOL>", e.g. "US:AAPL".
InstrumentId = str


class Instrument(BaseModel):
    id: str  # "US:AAPL", "ASX:BHP", "CRYPTO:BTC-USD"
    market: str  # plugin name: "us", "asx", "crypto"
    symbol: str
    name: str | None = None
    currency: str
    sector: str | None = None


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


Direction = Literal["long", "short", "flat"]


class Signal(BaseModel):
    id: str
    ts: datetime
    instrument_id: str
    strategy: str  # plugin name
    direction: Direction
    conviction: float = Field(ge=0, le=1)
    horizon_days: int
    thesis: str
    invalidation: str
    evidence_ids: list[str] = Field(default_factory=list)
    model: str | None = None
    prompt_version: str | None = None
    cost_usd: float | None = None


class Order(BaseModel):
    id: str
    signal_id: str
    instrument_id: str
    side: Literal["buy", "sell"]
    qty: float
    type: Literal["market", "limit"] = "market"
    limit_price: float | None = None
    submitted_ts: datetime
    broker: str


class Fill(BaseModel):
    order_id: str
    ts: datetime
    qty: float
    price: float
    fee: float
    slippage: float


class Position(BaseModel):
    instrument_id: str
    qty: float
    avg_price: float
    opened_ts: datetime
    signal_id: str


class Evaluation(BaseModel):
    signal_id: str
    evaluated_ts: datetime
    horizon_return: float
    hit: bool
    benchmark_return: float
    excess_return: float


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
