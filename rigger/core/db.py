"""SQLModel engine, tables, session helpers.

Single-file SQLite DB, easy to back up and inspect.
"""

from __future__ import annotations

from datetime import date, datetime
from pathlib import Path
from typing import Any

from sqlalchemy import JSON, Column, UniqueConstraint
from sqlalchemy.engine import Engine
from sqlmodel import Field, Session, SQLModel, create_engine


class InstrumentTable(SQLModel, table=True):
    __tablename__ = "instrument"

    id: str = Field(primary_key=True)
    market: str
    symbol: str
    name: str | None = None
    currency: str
    sector: str | None = None


class BarTable(SQLModel, table=True):
    __tablename__ = "bar"
    __table_args__ = (UniqueConstraint("instrument_id", "ts", name="uq_bar_instrument_ts"),)

    id: int | None = Field(default=None, primary_key=True)
    instrument_id: str = Field(index=True)
    ts: datetime = Field(index=True)
    open: float
    high: float
    low: float
    close: float
    volume: float
    source: str


class NewsItemTable(SQLModel, table=True):
    __tablename__ = "newsitem"

    id: str = Field(primary_key=True)
    instrument_ids: str  # JSON-encoded list[str]
    published: datetime = Field(index=True)
    title: str
    url: str
    body: str | None = None
    source: str


class EventTable(SQLModel, table=True):
    __tablename__ = "event"

    id: str = Field(primary_key=True)
    instrument_id: str = Field(index=True)
    ts: datetime = Field(index=True)
    kind: str
    summary: str
    sentiment: float
    evidence_ids: str  # JSON-encoded list[str]
    extracted_by: str
    prompt_version: str


class FundamentalTable(SQLModel, table=True):
    __tablename__ = "fundamental"

    id: int | None = Field(default=None, primary_key=True)
    instrument_id: str = Field(index=True)
    as_of: date
    metric: str
    value: float
    source: str


class SignalTable(SQLModel, table=True):
    __tablename__ = "signal"

    id: str = Field(primary_key=True)
    ts: datetime = Field(index=True)
    instrument_id: str = Field(index=True)
    strategy: str
    direction: str
    conviction: float
    horizon_days: int
    thesis: str
    invalidation: str
    evidence_ids: str  # JSON-encoded list[str]
    model: str | None = None
    prompt_version: str | None = None
    cost_usd: float | None = None
    metadata_: dict[str, Any] | None = Field(
        default_factory=dict, sa_column=Column("metadata", JSON, nullable=True)
    )


class OrderTable(SQLModel, table=True):
    __tablename__ = "order"

    id: str = Field(primary_key=True)
    signal_id: str = Field(index=True)
    instrument_id: str
    side: str
    qty: float
    type: str
    limit_price: float | None = None
    submitted_ts: datetime
    broker: str


class FillTable(SQLModel, table=True):
    __tablename__ = "fill"

    id: int | None = Field(default=None, primary_key=True)
    order_id: str = Field(index=True)
    ts: datetime
    qty: float
    price: float
    fee: float
    slippage: float


class PositionTable(SQLModel, table=True):
    __tablename__ = "position"

    instrument_id: str = Field(primary_key=True)
    qty: float
    avg_price: float
    opened_ts: datetime
    signal_id: str


class EvaluationTable(SQLModel, table=True):
    __tablename__ = "evaluation"

    id: int | None = Field(default=None, primary_key=True)
    signal_id: str = Field(index=True)
    evaluated_ts: datetime
    horizon_return: float
    hit: bool
    benchmark_return: float
    excess_return: float


class LLMCallTable(SQLModel, table=True):
    __tablename__ = "llmcall"

    id: str = Field(primary_key=True)
    ts: datetime
    task: str
    model: str
    prompt_version: str
    prompt_hash: str = Field(index=True)
    input_tokens: int
    output_tokens: int
    cost_usd: float
    latency_ms: int
    cached: bool = Field(index=True)

    # Cached response payload, used for backtest replay.
    response: str | None = None


class CashTable(SQLModel, table=True):
    __tablename__ = "cash"

    id: int = Field(default=1, primary_key=True)
    balance: float
    base_currency: str


def init_engine(db_path: str | Path) -> Engine:
    path = Path(db_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    engine = create_engine(f"sqlite:///{path}", echo=False)
    SQLModel.metadata.create_all(engine)
    _migrate(engine)
    return engine


# Columns added after Phase 1. create_all() never alters existing tables, so
# add them here for databases created before the column existed.
_ADDED_COLUMNS: list[tuple[str, str, str]] = [
    ("signal", "metadata", "JSON"),
]


def _migrate(engine: Engine) -> None:
    with engine.begin() as conn:
        for table, column, ddl_type in _ADDED_COLUMNS:
            existing = {row[1] for row in conn.exec_driver_sql(f"PRAGMA table_info({table})")}
            if column not in existing:
                conn.exec_driver_sql(f"ALTER TABLE {table} ADD COLUMN {column} {ddl_type}")


def ensure_cash(engine: Engine, base_currency: str, starting_cash: float) -> None:
    with Session(engine) as session:
        existing = session.get(CashTable, 1)
        if existing is None:
            session.add(CashTable(id=1, balance=starting_cash, base_currency=base_currency))
            session.commit()


def session_factory(engine: Engine) -> Session:
    return Session(engine)
