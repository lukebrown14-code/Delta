"""SQLModel engine, tables, session helpers.

Single-file SQLite DB, easy to back up and inspect.
"""

from __future__ import annotations

from collections.abc import Iterable
from datetime import date, datetime
from pathlib import Path
from typing import Any

from sqlalchemy import UniqueConstraint, event
from sqlalchemy.dialects.sqlite import insert as sqlite_insert
from sqlalchemy.engine import Connection, Engine
from sqlmodel import Field, Session, SQLModel, create_engine

from delta.core.json import to_json
from delta.core.models import Bar, Event, Fundamental, NewsItem


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


class NewsInstrumentTable(SQLModel, table=True):
    """Indexed news -> instrument mapping, kept in sync with ``newsitem.instrument_ids``.

    Replaces the ``LIKE '%"id"%'`` scan over the JSON column for target
    filtering: chat, reports, review and theses all join through here instead.
    """

    __tablename__ = "news_instrument"
    __table_args__ = (
        UniqueConstraint("news_id", "instrument_id", name="uq_news_instrument"),
    )

    id: int | None = Field(default=None, primary_key=True)
    news_id: str = Field(index=True)
    instrument_id: str = Field(index=True)


@event.listens_for(NewsItemTable, "after_insert")
def _sync_news_instrument(_mapper: Any, connection: Connection, target: NewsItemTable) -> None:
    """Mirror a news row's ``instrument_ids`` into the join table on every write.

    Registering here (rather than only in ``store_items``) keeps the mapping
    correct for any writer — including direct ``Session.add`` in tests — so the
    join never silently disagrees with the JSON column it indexes.
    """
    from delta.core.json import from_json

    for instrument_id in from_json(target.instrument_ids):
        connection.execute(
            sqlite_insert(NewsInstrumentTable)
            .values(news_id=target.id, instrument_id=instrument_id)
            .on_conflict_do_nothing(index_elements=["news_id", "instrument_id"])
        )


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


class SentimentTable(SQLModel, table=True):
    __tablename__ = "sentiment"

    id: str = Field(primary_key=True)
    instrument_id: str = Field(index=True)
    evidence_id: str = Field(index=True)
    ts: datetime = Field(index=True)
    stance: str
    confidence: float
    probabilities: str  # JSON-encoded dict[str, float]
    model: str
    prompt_version: str


def _apply_pragmas(dbapi_connection: Any, _record: Any) -> None:
    """WAL + NORMAL synchronous + a long busy timeout on every new connection.

    WAL lets readers proceed during writes; ``synchronous=NORMAL`` keeps WAL
    durable against process crash (the trade the audit accepts) while avoiding
    a per-commit fsync; ``busy_timeout`` makes concurrent access wait instead of
    raising ``database is locked``.
    """
    cursor = dbapi_connection.cursor()
    cursor.execute("PRAGMA journal_mode=WAL")
    cursor.execute("PRAGMA synchronous=NORMAL")
    cursor.execute("PRAGMA busy_timeout=5000")
    cursor.close()


def init_engine(db_path: str | Path) -> Engine:
    path = Path(db_path)
    path.parent.mkdir(parents=True, exist_ok=True)
    engine = create_engine(f"sqlite:///{path}", echo=False)
    event.listen(engine, "connect", _apply_pragmas)
    # ``create_all`` checks before creating, so this runs once at init and is the
    # single place tables are materialised; per-call ``_ensure_tables`` copies
    # elsewhere re-run it harmlessly but needlessly.  The news_instrument mapping
    # is backfilled from any pre-existing ``newsitem.instrument_ids`` JSON.
    SQLModel.metadata.create_all(engine)
    with engine.begin() as conn:
        _migrate(conn)
        _backfill_news_instrument(conn)
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


def _migrate(conn: Connection) -> None:
    for name, table, cols in _ADDED_UNIQUE_INDEXES:
        if _has_unique_index(conn, table, cols):
            continue  # fresh DB: the model's UniqueConstraint already created one
        key = ", ".join(cols)
        conn.exec_driver_sql(
            f"DELETE FROM {table} WHERE id NOT IN (SELECT MIN(id) FROM {table} GROUP BY {key})"
        )
        conn.exec_driver_sql(f"CREATE UNIQUE INDEX IF NOT EXISTS {name} ON {table} ({key})")


def _backfill_news_instrument(conn: Connection) -> None:
    """Insert one ``news_instrument`` row per instrument_id for every newsitem.

    Idempotent via the ``(news_id, instrument_id)`` unique constraint, so it is
    safe to run on every startup: existing databases gain the indexed mapping
    from their ``instrument_ids`` JSON without rewriting any news rows.
    """
    from delta.core.json import from_json

    for news_id, raw_ids in conn.exec_driver_sql(
        "SELECT id, instrument_ids FROM newsitem"
    ).all():
        for instrument_id in from_json(raw_ids):
            conn.exec_driver_sql(
                "INSERT OR IGNORE INTO news_instrument (news_id, instrument_id) VALUES (?, ?)",
                (news_id, instrument_id),
            )


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
        # Keep the indexed news -> instrument mapping in sync with the JSON column.
        if counts.get("newsitem", 0):
            links = [
                {"news_id": item.id, "instrument_id": iid}
                for item in items
                if isinstance(item, NewsItem)
                for iid in item.instrument_ids
            ]
            for start in range(0, len(links), _MAX_SQL_VARIABLES // 3):
                session.exec(
                    sqlite_insert(NewsInstrumentTable)
                    .values(links[start : start + _MAX_SQL_VARIABLES // 3])
                    .on_conflict_do_nothing(index_elements=["news_id", "instrument_id"])
                )
        session.commit()
    return counts
