"""SQLModel engine, tables, session helpers.

Single-file SQLite DB, easy to back up and inspect.
"""

from __future__ import annotations

from collections.abc import Iterable
from datetime import date, datetime
from pathlib import Path

from sqlalchemy import UniqueConstraint
from sqlalchemy.dialects.sqlite import insert as sqlite_insert
from sqlalchemy.engine import Connection, Engine
from sqlmodel import Field, Session, SQLModel, create_engine

from delta.core.json import to_json
from delta.core.models import Bar, Event, Fundamental, NewsItem


class InstrumentTable(SQLModel, table=True):
    __tablename__ = "instrument"
    __table_args__ = (UniqueConstraint("id", name="uq_instrument_id"),)

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
    __table_args__ = (
        UniqueConstraint("instrument_id", "as_of", "metric", "source", name="uq_fundamental_key"),
    )

    id: int | None = Field(default=None, primary_key=True)
    instrument_id: str = Field(index=True)
    as_of: date
    metric: str
    value: float
    source: str


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


def init_engine(db_path: str | Path) -> Engine:
    path = Path(db_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    engine = create_engine(f"sqlite:///{path}", echo=False)
    SQLModel.metadata.create_all(engine)
    _migrate(engine)
    return engine


# Unique indexes added after Phase 1; SQLite cannot ALTER a constraint in, but
# a unique index is equivalent for ON CONFLICT purposes.
_ADDED_UNIQUE_INDEXES: list[tuple[str, str, tuple[str, ...]]] = [
    # Phase 1 databases were created before BarTable declared this constraint.
    ("uq_bar_instrument_ts", "bar", ("instrument_id", "ts")),
    ("uq_fundamental_key", "fundamental", ("instrument_id", "as_of", "metric", "source")),
]


def _has_unique_index(conn: Connection, table: str, cols: tuple[str, ...]) -> bool:
    """True when any unique index (named or constraint autoindex) covers exactly ``cols``."""
    for row in conn.exec_driver_sql(f"PRAGMA index_list({table})"):
        name, unique = row[1], row[2]
        if not unique:
            continue
        indexed = tuple(r[2] for r in conn.exec_driver_sql(f"PRAGMA index_info({name})"))
        if indexed == cols:
            return True
    return False


def _migrate(engine: Engine) -> None:
    with engine.begin() as conn:
        for name, table, cols in _ADDED_UNIQUE_INDEXES:
            if _has_unique_index(conn, table, cols):
                continue  # fresh DB: the model's UniqueConstraint already created one
            key = ", ".join(cols)
            conn.exec_driver_sql(
                f"DELETE FROM {table} WHERE id NOT IN (SELECT MIN(id) FROM {table} GROUP BY {key})"
            )
            conn.exec_driver_sql(f"CREATE UNIQUE INDEX IF NOT EXISTS {name} ON {table} ({key})")


# SQLITE_MAX_VARIABLE_NUMBER default since SQLite 3.32.
_MAX_SQL_VARIABLES = 32766

# (model, table, JSON-encoded list field or None, conflict key)
_STORES: tuple[tuple[type, type[SQLModel], str | None, tuple[str, ...]], ...] = (
    (Bar, BarTable, None, ("instrument_id", "ts")),
    (NewsItem, NewsItemTable, "instrument_ids", ("id",)),
    (Event, EventTable, "evidence_ids", ("id",)),
    (Fundamental, FundamentalTable, None, ("instrument_id", "as_of", "metric", "source")),
)


def store_items(
    engine: Engine, items: Iterable[Bar | NewsItem | Fundamental | Event]
) -> dict[str, int]:
    """Persist data-plugin output idempotently. Returns rows actually inserted per table."""
    items = list(items)
    counts: dict[str, int] = {}
    with Session(engine) as session:
        for model, table, json_field, key in _STORES:
            rows = []
            for item in items:
                if not isinstance(item, model):
                    continue
                data = item.model_dump()
                if json_field:
                    data[json_field] = to_json(data[json_field])
                rows.append(data)
            name = str(table.__tablename__)
            counts[name] = 0
            chunk = _MAX_SQL_VARIABLES // (len(table.__table__.columns) + 1)  # type: ignore[attr-defined]
            for start in range(0, len(rows), chunk):
                result = session.exec(
                    sqlite_insert(table)
                    .values(rows[start : start + chunk])
                    .on_conflict_do_nothing(index_elements=list(key))
                )
                counts[name] += int(result.rowcount)
        session.commit()
    return counts


def session_factory(engine: Engine) -> Session:
    return Session(engine)
