"""Shared populated golden seed: builds ``fixtures/golden_seed.db``.

The Rust golden harness (``crates/delta-tui/tests/golden.rs``) loads this DB
file directly instead of rebuilding the scenario data in Rust test code, and
the R3.2 screen streams export their populated states against it. Frozen
clock 2026-09-21 09:30 UTC — the same instant as ``tests/export_golden.py`` —
and the AAPL bars are seeded with the exporter's exact ``seed_bars`` call, so
the seed and the committed goldens describe the same world.

Contents:
- two watchlist instruments (US:AAPL, US:MSFT) with 80 daily bars each
  (the exporter's ``_price`` series for AAPL), news, an event and fundamentals
- one generated report per research target (``build_report`` through the
  FakeLLM fixtures under ``fixtures/llm/report/<name>.json``, written with
  ``write_report`` to ``fixtures/golden_reports/``: markdown + sidecar JSON)
- chat history in the seed's ``chatmessage`` table (the Python app keeps chat
  in memory only, so the seed persists the turns the populated Ask goldens
  will render; the assistant turn quotes ``fixtures/llm/chat/response.json``)
- two theses with linked evidence (the health read model computes from them)
- decisions, one due on the frozen clock with a review on its history
- ``llmcall`` rows so the cost surfaces have data

Deterministic: every insert is ordered, ids are fixed (or derived from fixed
inputs), and the clock is frozen — re-running produces a byte-identical DB,
which ``test_golden_seed_is_deterministic`` asserts.

FakeLLM fixture format (``fixtures/llm/<task>/<name>.json``), shared with the
Rust ``FakeLlm``: ``{"cost_usd": <float>, "text": <payload object>}``. ``text``
is the canned model output; callers serialise it verbatim as the result text.
``<task>/response.json`` is the task's default; other files load under the
``<task>/<name>`` key (per-target canned responses, for example).

Usage::

    uv run python tests/golden_seed.py                # rebuild the committed fixtures
    uv run pytest tests/golden_seed.py -q             # determinism checks
"""

from __future__ import annotations

import asyncio
import json
import math
import shutil
import sqlite3
import sys
import tempfile
from datetime import datetime
from pathlib import Path

import pytest
from sqlalchemy import text as sql_text
from sqlmodel import Session

sys.path.insert(0, str(Path(__file__).parent))  # conftest / test_tui imports

from conftest import FakeLLM, seed_bars  # noqa: E402
from export_golden import BAR_COUNT, BAR_START, NOW, SEED, _price  # noqa: E402
from test_tui import AAPL, FakeRig  # noqa: E402

from delta.core.db import (  # noqa: E402
    EventTable,
    FundamentalTable,
    LLMCallTable,
    NewsItemTable,
    init_engine,
)
from delta.core.json import to_json  # noqa: E402
from delta.core.models import Instrument  # noqa: E402
from delta.decisions import DecisionReviewTable, DecisionTable  # noqa: E402
from delta.reports import build_report, write_report  # noqa: E402
from delta.theses import add_evidence, create_thesis  # noqa: E402

MSFT = Instrument(id="US:MSFT", market="us", symbol="MSFT", currency="USD", sector="Tech")

#: Repo-relative paths of the committed fixtures this script owns.
REPORTS = "fixtures/golden_reports"
LLM_FIXTURES = "fixtures/llm"

#: Routing the seed rig uses; also the ``model`` column of the llmcall rows.
ROUTING = {
    "report": "openrouter/anthropic/claude-sonnet-4.5",
    "chat": "openrouter/anthropic/claude-sonnet-4.5",
    "sentiment": "openrouter/anthropic/claude-sonnet-4.5",
    "thesis_summary": "openrouter/anthropic/claude-sonnet-4.5",
}

#: The Python app keeps chat turns in memory only; the seed persists them so
#: the Rust harness (and the populated Ask goldens) can read the history.
_CHAT_SCHEMA = """
CREATE TABLE IF NOT EXISTS chatmessage (
    seq INTEGER PRIMARY KEY,
    role TEXT NOT NULL,
    text TEXT NOT NULL,
    citations TEXT NOT NULL,
    source TEXT NOT NULL,
    ts TEXT NOT NULL
)
"""


#: The Python build creates these as unnamed constraint autoindexes; the Rust
#: engine's schema expects the named ones, so the seed creates them up front:
#: opening the DB through ``delta_core::db::Db::open`` then performs no writes
#: and the committed fixture stays byte-identical after a harness run.
_RUST_INDEXES = (
    "CREATE UNIQUE INDEX IF NOT EXISTS uq_bar_instrument_ts ON bar (instrument_id, ts)",
    "CREATE UNIQUE INDEX IF NOT EXISTS uq_fundamental_key ON fundamental"
    " (instrument_id, as_of, metric, source)",
    "CREATE UNIQUE INDEX IF NOT EXISTS uq_news_instrument ON news_instrument"
    " (news_id, instrument_id)",
)


def _create_schema(db_path: Path) -> None:
    """Create every table and index in a fixed, sorted order.

    ``metadata.create_all`` iterates each table's index set, whose order
    varies with ``PYTHONHASHSEED`` between processes — fatal for a
    byte-identical fixture. Emitting the DDL ourselves (tables sorted by
    name, indexes sorted by name) removes the variance; the later
    ``create_all`` inside ``init_engine`` then finds everything and skips.
    """
    from sqlalchemy.dialects.sqlite import dialect as sqlite_dialect
    from sqlalchemy.schema import CreateIndex, CreateTable
    from sqlmodel import SQLModel

    from delta.core.db import SentimentTable  # noqa: F401  (registers its table)
    from delta.decisions import DecisionReviewTable, DecisionTable  # noqa: F401
    from delta.theses import ThesisEvidenceTable, ThesisTable  # noqa: F401

    dialect = sqlite_dialect()
    tables = sorted(SQLModel.metadata.tables.values(), key=lambda table: table.name)
    conn = sqlite3.connect(db_path)
    try:
        for table in tables:
            conn.execute(str(CreateTable(table).compile(dialect=dialect)))
        for table in tables:
            for index in sorted(table.indexes, key=lambda index: index.name or ""):
                conn.execute(str(CreateIndex(index).compile(dialect=dialect)))
        conn.commit()
    finally:
        conn.close()


def _msft_price(i: int) -> float:
    return round(90.0 + i * 0.25 + 4 * math.sin(i / 3), 2)


def load_llm_fixtures(root: Path) -> dict[str, dict]:
    """Load ``<task>/<name>.json`` fixtures as ``{"task": canned, "task/name": canned}``."""
    out: dict[str, dict] = {}
    for task_dir in sorted(p for p in root.iterdir() if p.is_dir()):
        for path in sorted(task_dir.glob("*.json")):
            canned = json.loads(path.read_text(encoding="utf-8"))
            key = task_dir.name if path.stem == "response" else f"{task_dir.name}/{path.stem}"
            out[key] = canned
    return out


def _seed_market_data(engine) -> None:
    seed_bars(engine, AAPL.id, n=BAR_COUNT, start=BAR_START, price_fn=_price)
    seed_bars(engine, MSFT.id, n=BAR_COUNT, start=BAR_START, price_fn=_msft_price)
    news = [
        NewsItemTable(
            id="news-aapl-chip",
            instrument_ids=to_json(["US:AAPL"]),
            published=datetime(2026, 9, 15, 14, 30),
            title="Apple announces M6 chip with on-device inference",
            url="https://example.com/news/aapl-chip",
            body="The chip ships in laptops this quarter; analysts see an upgrade cycle.",
            source="rss",
        ),
        NewsItemTable(
            id="news-aapl-10q",
            instrument_ids=to_json(["US:AAPL"]),
            published=datetime(2026, 9, 18, 16, 5),
            title="Apple 10-Q: services revenue up 12% year on year",
            url="https://example.com/filings/aapl-10q",
            body="Services margin held; installed base grew.",
            source="sec_edgar",
        ),
        NewsItemTable(
            id="news-aapl-supply",
            instrument_ids=to_json(["US:AAPL"]),
            published=datetime(2026, 9, 8, 9, 10),
            title="Suppliers flag panel shortages into the holiday quarter",
            url="https://example.com/news/aapl-supply",
            body="Two suppliers trimmed shipment guidance for the December quarter.",
            source="rss",
        ),
        NewsItemTable(
            id="news-msft-cloud",
            instrument_ids=to_json(["US:MSFT"]),
            published=datetime(2026, 9, 16, 11, 0),
            title="Enterprise cloud demand re-accelerates, channel checks say",
            url="https://example.com/news/msft-cloud",
            body="Booking momentum improved for the third straight month.",
            source="rss",
        ),
        NewsItemTable(
            id="news-msft-capex",
            instrument_ids=to_json(["US:MSFT"]),
            published=datetime(2026, 9, 10, 13, 45),
            title="Microsoft steps up data-centre capex into next year",
            url="https://example.com/news/msft-capex",
            body="Capex guidance now implies a step-up faster than revenue.",
            source="rss",
        ),
    ]
    with Session(engine) as session:
        for row in news:
            session.add(row)
        session.commit()
    with Session(engine) as session:
        session.add(
            EventTable(
                id="event-aapl-earnings",
                instrument_id="US:AAPL",
                ts=datetime(2026, 10, 1, 20, 0),
                kind="earnings",
                summary="Q4 results",
                sentiment=0.2,
                evidence_ids=to_json([]),
                extracted_by="test",
                prompt_version="test_v1",
            )
        )
        session.add(
            EventTable(
                id="event-msft-earnings",
                instrument_id="US:MSFT",
                ts=datetime(2026, 10, 27, 21, 0),
                kind="earnings",
                summary="Q1 FY27 results",
                sentiment=0.1,
                evidence_ids=to_json([]),
                extracted_by="test",
                prompt_version="test_v1",
            )
        )
        for instrument_id, metric, value in [
            ("US:AAPL", "Market cap", 3.4e12),
            ("US:AAPL", "P/E", 31.2),
            ("US:AAPL", "Gross margin", 48.7),
            ("US:MSFT", "Market cap", 3.1e12),
            ("US:MSFT", "P/E", 33.9),
            ("US:MSFT", "Gross margin", 69.8),
        ]:
            session.add(
                FundamentalTable(
                    instrument_id=instrument_id,
                    as_of=datetime(2026, 6, 30).date(),
                    metric=metric,
                    value=value,
                    source="yahoo",
                )
            )
        session.commit()


def _seed_theses(engine) -> list:
    one = create_thesis(
        engine,
        "Apple's services line sustains double-digit growth",
        scope="US:AAPL",
        assumptions=["Services mix keeps rising", "Churn stays flat"],
        falsifiers=["Two consecutive quarters of single-digit services growth"],
        targets=["US:AAPL"],
        time_horizon="12m",
    )
    two = create_thesis(
        engine,
        "Cloud capex compresses margins into next year",
        scope="US:MSFT",
        assumptions=["Capex guidance is credible", "Pricing holds"],
        falsifiers=["Free-cash margin expands despite the capex step-up"],
        targets=["US:MSFT"],
        time_horizon="6m",
    )
    add_evidence(
        engine,
        one.id,
        "news:news-aapl-10q",
        "support",
        "Filing confirms the growth rate.",
        accepted=True,
    )
    add_evidence(
        engine,
        one.id,
        "news:news-aapl-supply",
        "against",
        "Candidate: shortages would slow device attach.",
        accepted=False,
    )
    add_evidence(
        engine,
        two.id,
        "news:news-msft-capex",
        "support",
        "Guidance implies margin pressure.",
        accepted=True,
    )
    return [one, two]


def _seed_decisions(engine, theses: list) -> None:
    with Session(engine) as session:
        session.add(
            DecisionTable(
                id="dec-seed-aapl",
                instrument_id="US:AAPL",
                rationale="Chip cycle into the earnings print; services carry the multiple.",
                valuation_context="32x forward earnings",
                time_horizon="3m",
                review_date=datetime(2026, 9, 21).date(),
                invalidation_criteria="Supply warnings escalate or services growth misses",
                thesis_id=theses[0].id,
                thesis_claim_snapshot=theses[0].claim,
                created_at=datetime(2026, 9, 11, 9, 30),
                status="open",
            )
        )
        session.add(
            DecisionTable(
                id="dec-seed-msft",
                instrument_id="US:MSFT",
                rationale="Cloud demand durable; capex narrative needs watching.",
                valuation_context="30x forward earnings",
                time_horizon="6m",
                review_date=datetime(2026, 10, 27).date(),
                invalidation_criteria="Cloud bookings roll over",
                thesis_id=theses[1].id,
                thesis_claim_snapshot=theses[1].claim,
                created_at=datetime(2026, 9, 18, 9, 30),
                status="open",
            )
        )
        session.add(
            DecisionReviewTable(
                decision_id="dec-seed-aapl",
                note="Tracking plan; chip cycle on schedule.",
                created_at=datetime(2026, 8, 21, 9, 30),
                status=None,
            )
        )
        session.commit()


def _seed_llm_calls(engine, fixtures: dict[str, dict]) -> None:
    cost = fixtures["report/apple"]["cost_usd"]
    calls = [
        ("call-seed-report-aapl", datetime(2026, 9, 21, 8, 30), "report", "report_v1",
         "seed-prompt-report-aapl", 1500, 620, 1420),
        ("call-seed-report-msft", datetime(2026, 9, 21, 8, 31), "report", "report_v1",
         "seed-prompt-report-msft", 1480, 590, 1390),
        ("call-seed-chat-1", datetime(2026, 9, 21, 8, 40), "chat", "chat_v1",
         "seed-prompt-chat-1", 900, 210, 640),
        ("call-seed-thesis-1", datetime(2026, 9, 21, 8, 45), "thesis_summary",
         "thesis_summary_v1", "seed-prompt-thesis-1", 700, 180, 510),
    ]
    with Session(engine) as session:
        for id_, ts, task, prompt_version, prompt_hash, input_t, output_t, latency in calls:
            session.add(
                LLMCallTable(
                    id=id_,
                    ts=ts,
                    task=task,
                    model=ROUTING[task],
                    prompt_version=prompt_version,
                    prompt_hash=prompt_hash,
                    input_tokens=input_t,
                    output_tokens=output_t,
                    cost_usd=cost,
                    latency_ms=latency,
                    cached=False,
                    response=None,
                )
            )
        session.commit()


def _seed_chat(engine, fixtures: dict[str, dict]) -> None:
    answer = fixtures["chat"]["text"]["answer"]
    turns = [
        (1, "user", "What drove Apple's latest quarter?", "[]", "user",
         "2026-09-21T08:39:00+00:00"),
        (2, "assistant", answer, to_json(["news:news-aapl-chip", "news:news-aapl-10q"]),
         "stored", "2026-09-21T08:40:00+00:00"),
    ]
    with Session(engine) as session:
        for seq, role, body, citations, source, ts in turns:
            session.execute(
                sql_text(
                    "INSERT INTO chatmessage (seq, role, text, citations, source, ts) "
                    "VALUES (:seq, :role, :text, :citations, :source, :ts)"
                ),
                {
                    "seq": seq,
                    "role": role,
                    "text": body,
                    "citations": citations,
                    "source": source,
                    "ts": ts,
                },
            )
        session.commit()


async def _seed_reports(engine, reports_dir: Path, fixtures: dict[str, dict]) -> None:
    rig = FakeRig(engine, [AAPL, MSFT])
    rig.cfg.llm_routing = ROUTING
    for target_id, fixture_key in (("US:AAPL", "report/apple"), ("US:MSFT", "report/microsoft")):
        canned = fixtures[fixture_key]
        rig.llm = FakeLLM(response_by_task={"report": canned["text"]}, cost=canned["cost_usd"])
        report = await build_report(rig, target_id)
        write_report(report, reports_dir)


def build(db_path: Path, reports_dir: Path, llm_root: Path | None = None) -> None:
    """(Re)create the seed DB and its report fixtures deterministically."""
    import time_machine

    fixtures = load_llm_fixtures(llm_root or Path(LLM_FIXTURES))
    for suffix in ("", "-wal", "-shm"):
        Path(str(db_path) + suffix).unlink(missing_ok=True)
    if reports_dir.exists():
        shutil.rmtree(reports_dir)
    db_path.parent.mkdir(parents=True, exist_ok=True)
    reports_dir.mkdir(parents=True, exist_ok=True)
    _create_schema(db_path)
    engine = init_engine(db_path)
    with Session(engine) as session:
        session.execute(sql_text(_CHAT_SCHEMA))
        for index in _RUST_INDEXES:
            session.execute(sql_text(index))
        session.commit()
    with time_machine.travel(NOW, tick=False):
        _seed_market_data(engine)
        theses = _seed_theses(engine)
        _seed_decisions(engine, theses)
        _seed_llm_calls(engine, fixtures)
        _seed_chat(engine, fixtures)
        asyncio.run(_seed_reports(engine, reports_dir, fixtures))
    engine.dispose()  # close every pooled connection so -wal/-shm are checkpointed away


def main() -> None:
    repo = Path(__file__).parent.parent
    build(repo / SEED, repo / REPORTS)
    print(f"wrote {repo / SEED} and {repo / REPORTS}")


def test_golden_seed_is_deterministic(tmp_path):
    """Building the seed twice yields a byte-identical DB and report files."""
    a, b = tmp_path / "a", tmp_path / "b"
    build(a / "golden_seed.db", a / "reports")
    build(b / "golden_seed.db", b / "reports")
    assert (a / "golden_seed.db").read_bytes() == (b / "golden_seed.db").read_bytes()
    a_files = sorted(p.relative_to(a) for p in a.rglob("*") if p.is_file())
    b_files = sorted(p.relative_to(b) for p in b.rglob("*") if p.is_file())
    assert a_files == b_files and a_files
    for rel in a_files:
        assert (a / rel).read_bytes() == (b / rel).read_bytes(), str(rel)


def test_seed_contents_match_committed_fixtures():
    """The committed seed equals a fresh build table by table.

    Contents rather than bytes: the SQLite header embeds a library version, so
    a build from a different SQLite patch release can differ byte-wise while
    holding exactly the same rows. The byte-identity of same-environment runs
    is covered by ``test_golden_seed_is_deterministic``.
    """
    repo = Path(__file__).parent.parent
    committed_db = repo / SEED
    if not committed_db.exists():
        pytest.skip("fixtures/golden_seed.db not built yet")
    with tempfile.TemporaryDirectory() as td:
        fresh = Path(td) / "golden_seed.db"
        build(fresh, Path(td) / "reports")
        conn = sqlite3.connect(committed_db)
        try:
            fresh_conn = sqlite3.connect(fresh)
            try:
                tables = [
                    row[0]
                    for row in conn.execute(
                        "SELECT name FROM sqlite_master WHERE type='table'"
                        " AND name NOT LIKE 'sqlite_%' ORDER BY name"
                    )
                ]
                for table in tables:
                    columns = [row[1] for row in conn.execute(f"PRAGMA table_info({table})")]
                    order = ", ".join(f'"{c}"' for c in columns)
                    a = conn.execute(f'SELECT * FROM "{table}" ORDER BY {order}').fetchall()
                    b = fresh_conn.execute(
                        f'SELECT * FROM "{table}" ORDER BY {order}'
                    ).fetchall()
                    assert a == b, table
            finally:
                fresh_conn.close()
        finally:
            conn.close()


if __name__ == "__main__":
    main()
