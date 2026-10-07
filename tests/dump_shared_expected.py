"""Dump expected shared-services values from the golden seed as JSON.

The R3.1b Rust parity test (``crates/delta-services/tests/shared_parity.rs``)
asserts the Rust ports of ``delta/evidence.py``, ``delta/theses.py``,
``delta/decisions.py``, ``delta/review.py`` and the ``setup_checks`` /
``data_provider_status`` group against the outputs produced here by running
the Python implementations over ``fixtures/golden_seed.db`` (frozen clock
2026-09-21 09:30 UTC, per ``tests/export_golden.py``).

Read-only against the committed fixture: the DB is copied to a temp dir
before ``init_engine`` opens it, so the seed never gains a -wal or a
backfilled row. ``setup_checks`` is dumped with the provider env vars cleared
and a temp working directory, so the output does not depend on the dumping
machine.

Also verifies cross-implementation reads: ``--verify-written <db>`` opens a
database the Rust tests wrote (theses/decisions tables through the Rust write
APIs) and asserts the rows read back correctly through the Python models.

Usage::

    uv run python tests/dump_shared_expected.py                 # (re)write the JSON
    uv run python tests/dump_shared_expected.py --verify-written DB
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import sys
import tempfile
from datetime import UTC, datetime, timedelta
from pathlib import Path
from types import SimpleNamespace

sys.path.insert(0, str(Path(__file__).parent))

from delta import decisions, review, services, theses  # noqa: E402
from delta.core.config import read_env_value  # noqa: E402
from delta.core.db import init_engine  # noqa: E402
from delta.core.models import Instrument  # noqa: E402
from delta.evidence import cite, evidence, evidence_by_ids  # noqa: E402

SEED = Path(__file__).parent.parent / "fixtures" / "golden_seed.db"
OUT = Path(__file__).parent / "fixtures" / "shared_services_expected.json"

#: The frozen clock of the seed (``tests/export_golden.py``).
NOW = datetime(2026, 9, 21, 9, 30)

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")
MSFT = Instrument(id="US:MSFT", market="us", symbol="MSFT", currency="USD")


def ts_str(value: datetime) -> str:
    """Naive UTC ISO dump; the Rust side parses the same format."""
    return value.replace(tzinfo=None).isoformat()


def item_dict(item) -> dict:
    return {
        "id": item.id,
        "target_ids": list(item.target_ids),
        "ts": ts_str(item.ts),
        "kind": item.kind,
        "title": item.title,
        "body": item.body,
        "source": item.source,
        "url": item.url,
        "sentiment": item.sentiment,
        "quality": item.quality,
        "cite": cite(item),
        "raw": item.raw,
    }


def thesis_dict(thesis) -> dict:
    return {
        "id": thesis.id,
        "claim": thesis.claim,
        "scope": thesis.scope,
        "assumptions": list(thesis.assumptions),
        "falsifiers": list(thesis.falsifiers),
        "targets": list(thesis.targets),
        "time_horizon": thesis.time_horizon,
        "created_at": ts_str(thesis.created_at),
        "status": thesis.status,
    }


def decision_dict(decision) -> dict:
    return {
        "id": decision.id,
        "instrument_id": decision.instrument_id,
        "rationale": decision.rationale,
        "valuation_context": decision.valuation_context,
        "time_horizon": decision.time_horizon,
        "review_date": decision.review_date.isoformat(),
        "invalidation_criteria": decision.invalidation_criteria,
        "thesis_id": decision.thesis_id,
        "thesis_claim_snapshot": decision.thesis_claim_snapshot,
        "created_at": ts_str(decision.created_at),
        "status": decision.status,
    }


def review_dict(entry) -> dict:
    return {
        "id": entry.id,
        "decision_id": entry.decision_id,
        "note": entry.note,
        "created_at": ts_str(entry.created_at),
        "status": entry.status,
    }


def queue_item_dict(item) -> dict:
    return {
        "kind": item.kind,
        "instrument_id": item.instrument_id,
        "title": item.title,
        "detail": item.detail,
        "evidence_id": item.evidence_id,
        "thesis_id": item.thesis_id,
        "ts": ts_str(item.ts) if item.ts is not None else None,
    }


def _clear_provider_env() -> None:
    for name in (
        "OPENROUTER_API_KEY",
        "OPENAI_API_KEY",
        "ANTHROPIC_API_KEY",
        "CUSTOM_API_KEY",
    ):
        os.environ.pop(name, None)


def dump_expected() -> dict:
    _clear_provider_env()
    with tempfile.TemporaryDirectory() as td:
        copy = Path(td) / "golden_seed.db"
        shutil.copyfile(SEED, copy)
        engine = init_engine(copy)  # create_all finds every table: no writes

        out: dict = {"seed": "fixtures/golden_seed.db", "now": ts_str(NOW)}

        pool = evidence(engine)
        out["evidence_all"] = [item_dict(item) for item in pool]
        out["evidence_aapl"] = [item.id for item in evidence(engine, target="US:AAPL")]
        out["evidence_msft"] = [item.id for item in evidence(engine, target="US:MSFT")]
        out["evidence_since"] = [item.id for item in evidence(engine, since="2026-09-14")]
        for kind in ("bar", "news", "filing", "event", "fundamental", "web"):
            out[f"evidence_kind_{kind}"] = [item.id for item in evidence(engine, kind=kind)]
        out["evidence_search_services"] = [
            item.id for item in evidence(engine, search="services")
        ]
        out["evidence_search_upper"] = [
            item.id for item in evidence(engine, search="CLOUD")
        ]
        out["evidence_limit_3"] = [item.id for item in evidence(engine, limit=3)]
        out["evidence_limit_0"] = [item.id for item in evidence(engine, limit=0)]
        out["evidence_combined"] = [
            item.id
            for item in evidence(engine, target="US:AAPL", since="2026-09-15", kind="news")
        ]
        out["evidence_by_ids"] = [
            item.id
            for item in evidence_by_ids(
                engine,
                [
                    "news:news-aapl-10q",
                    "event:event-aapl-earnings",
                    "fundamental:1",
                    "bar:1",
                    "news:nope",
                    "no-prefix",
                    "filing:news-aapl-chip",
                ],
            )
        ]

        theses_rows = theses.list_theses(engine)
        out["theses"] = [thesis_dict(row) for row in theses_rows]
        out["thesis_evidence"] = {
            row.id: [
                {
                    "thesis_id": link.thesis_id,
                    "evidence_id": link.evidence_id,
                    "side": link.side,
                    "note": link.note,
                    "accepted": link.accepted,
                }
                for link in theses.evidence_for(engine, row.id, accepted_only=False)
            ]
            for row in theses_rows
        }
        out["accepted_items"] = {
            row.id: [
                {"item": item_dict(item), "link": link.note, "side": link.side}
                for item, link in theses.accepted_items(engine, row.id)
            ]
            for row in theses_rows
        }

        decision_rows = decisions.list_decisions(engine)
        out["decisions"] = [decision_dict(row) for row in decision_rows]
        out["decision_reviews"] = {
            row.id: [review_dict(entry) for entry in decisions.review_history(engine, row.id)]
            for row in decision_rows
        }
        out["due_reviews"] = [
            row.id for row in decisions.due_reviews(engine, as_of=NOW.date())
        ]

        audit = review.evidence_audit(engine, "US:AAPL", now=NOW, primary_sources={"sec_edgar"})
        out["audit_aapl"] = {
            "price_at": ts_str(audit.price_at) if audit.price_at else None,
            "news_at": ts_str(audit.news_at) if audit.news_at else None,
            "primary_at": ts_str(audit.primary_at) if audit.primary_at else None,
            "primary_coverage": audit.primary_coverage,
            "non_price_items": audit.non_price_items,
            "source_count": audit.source_count,
            "warnings": list(audit.warnings),
        }


        class Rig:
            def __init__(self, engine) -> None:
                self.engine = engine
                self._universe = [AAPL, MSFT]
                self.plugins = {"sec_edgar": SimpleNamespace(enabled=True)}

            def universe(self):
                return self._universe

        rig = Rig(engine)
        out["review_queue"] = [
            queue_item_dict(item)
            for item in review.review_queue(rig, since=NOW - timedelta(days=7), now=NOW)
        ]

        # setup_checks in a bare temp cwd (no config.toml, no .env): the
        # outcome depends only on the seed and the cleared environment.
        old_cwd = os.getcwd()
        os.chdir(td)
        try:
            rig.cfg = SimpleNamespace(
                llm_provider="openrouter",
                llm_base_url="",
                llm_routing={},
                plugins={"sec_edgar": {}},
            )
            rig.settings = SimpleNamespace(
                openrouter_api_key="", openai_api_key="", anthropic_api_key=""
            )
            out["setup_checks"] = [
                {"name": check.name, "ok": check.ok, "fix": check.fix}
                for check in services.setup_checks(rig)
            ]
            out["env_openrouter_empty"] = read_env_value("OPENROUTER_API_KEY") == ""
        finally:
            os.chdir(old_cwd)

        # Raw rows for the write/read round-trip (the Rust tests re-write the
        # same encodings and compare).
        import sqlite3

        conn = sqlite3.connect(copy)
        try:
            raw: dict = {}
            for table in ("thesis", "thesis_evidence", "decision", "decision_review"):
                columns = [row[1] for row in conn.execute(f"PRAGMA table_info({table})")]
                order = ", ".join(f'"{c}"' for c in columns)
                raw[table] = {
                    "columns": columns,
                    "rows": conn.execute(f'SELECT * FROM "{table}" ORDER BY {order}').fetchall(),
                }
            out["raw"] = raw
        finally:
            conn.close()
        engine.dispose()
    return out


def verify_written(db_path: Path) -> None:
    """Read a Rust-written DB through the Python models and assert the rows."""
    _clear_provider_env()
    with tempfile.TemporaryDirectory() as td:
        copy = Path(td) / "written.db"
        shutil.copyfile(db_path, copy)
        engine = init_engine(copy)

        rows = theses.list_theses(engine)
        assert len(rows) == 1, rows
        thesis = rows[0]
        assert thesis.claim == "Rust writes read back in Python", thesis
        assert thesis.scope == "parity"
        assert thesis.assumptions == ["encoding matches"], thesis
        assert thesis.falsifiers == ["mismatch"], thesis
        assert thesis.targets == ("US:AAPL",), thesis
        assert thesis.time_horizon == "3m", thesis
        assert thesis.created_at == datetime(2026, 9, 20, 12, 0, tzinfo=UTC), thesis
        assert thesis.status == "active", thesis

        links = theses.evidence_for(engine, thesis.id, accepted_only=False)
        assert [(link.evidence_id, link.side, link.note, link.accepted) for link in links] == [
            ("news:roundtrip", "support", "linked by Rust", True)
        ], links

        rows = decisions.list_decisions(engine)
        assert len(rows) == 1, rows
        decision = rows[0]
        assert decision.instrument_id == "US:AAPL", decision
        assert decision.rationale == "Written through the Rust API", decision
        assert decision.valuation_context == "10x forward", decision
        assert decision.time_horizon == "1y", decision
        assert decision.review_date == datetime(2026, 10, 1).date(), decision
        assert decision.invalidation_criteria == "Parity breaks", decision
        assert decision.thesis_id == thesis.id, decision
        assert decision.thesis_claim_snapshot == thesis.claim, decision
        assert decision.created_at == datetime(2026, 9, 20, 12, 0, tzinfo=UTC), decision
        assert decision.status == "reviewed", decision

        history = decisions.review_history(engine, decision.id)
        assert [(entry.note, entry.status) for entry in history] == [
            ("First look.", None),
            ("Still on plan.", "reviewed"),
        ], history
        assert history[0].created_at == datetime(2026, 9, 20, 13, 0, tzinfo=UTC), history
        assert history[1].created_at == datetime(2026, 9, 21, 8, 0, tzinfo=UTC), history

        due = decisions.due_reviews(engine, as_of=datetime(2026, 10, 2).date())
        assert [row.id for row in due] == [decision.id], due
    print("verify-written: OK — Rust-written thesis/decision rows read back through Python")


def main() -> None:
    parser = argparse.ArgumentParser()
    parser.add_argument("--verify-written", type=Path, default=None)
    args = parser.parse_args()
    if args.verify_written is not None:
        verify_written(args.verify_written)
        return
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(dump_expected(), indent=1, sort_keys=True) + "\n")
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
