"""Tests for the optional thesis layer: CRUD, candidates vs accepted, discovery."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

import pytest
from sqlmodel import Session
from textual.app import App

from rigger import theses
from rigger.core.db import NewsItemTable
from rigger.core.json import to_json
from rigger.evidence import evidence
from rigger.tui.screens.theses import Theses
from tests.conftest import FakeConfig, FakeLLM, seed_bars

INST = "US:AAPL"
OTHER = "US:MSFT"
NOW = datetime(2026, 3, 22, 12, 0, tzinfo=UTC)

CLAIM = "Solar panels grow as a share of electricity."
SCOPE = "Global electricity generation"


class FakeRig:
    """Duck-typed stand-in for Rigger: engine, routed config, LLM client."""

    def __init__(self, engine, llm=None, cfg=None):
        self.engine = engine
        self.llm = llm
        self.cfg = cfg if cfg is not None else FakeConfig()


def _seed_news(engine) -> None:
    """Two AAPL news items and one MSFT item, all published at NOW."""
    with Session(engine) as session:
        for row_id, inst_ids in (("n1", [INST]), ("n2", [INST]), ("m1", [OTHER])):
            session.add(
                NewsItemTable(
                    id=row_id,
                    instrument_ids=to_json(inst_ids),
                    published=NOW,
                    title=f"Title {row_id}",
                    url=f"https://example.com/{row_id}",
                    source="rss",
                )
            )
        session.commit()


def test_create_list_get_round_trip(tmp_engine):
    thesis = theses.create_thesis(
        tmp_engine,
        CLAIM,
        scope=SCOPE,
        assumptions=["Panel costs keep falling"],
        falsifiers=["guidance", "regulatory"],
        targets=(INST,),
        time_horizon="10y",
    )

    assert thesis.id == theses.thesis_id(CLAIM, SCOPE)
    assert thesis.status == "active"
    assert thesis.created_at.tzinfo is UTC
    assert thesis.targets == (INST,)
    assert thesis.assumptions == ["Panel costs keep falling"]
    assert thesis.falsifiers == ["guidance", "regulatory"]
    assert theses.list_theses(tmp_engine) == [thesis]
    assert theses.get_thesis(tmp_engine, thesis.id) == thesis

    with pytest.raises(KeyError):
        theses.get_thesis(tmp_engine, "nope")
    with pytest.raises(ValueError, match="already exists"):
        theses.create_thesis(tmp_engine, CLAIM, scope=SCOPE)


def test_status_transitions(tmp_engine):
    thesis = theses.create_thesis(tmp_engine, CLAIM)

    paused = theses.set_status(tmp_engine, thesis.id, "paused")
    assert paused.status == "paused"
    concluded = theses.set_status(tmp_engine, thesis.id, "concluded")
    assert concluded.status == "concluded"
    assert theses.get_thesis(tmp_engine, thesis.id).status == "concluded"
    assert theses.set_status(tmp_engine, thesis.id, "active").status == "active"

    with pytest.raises(ValueError, match="status must be one of"):
        theses.set_status(tmp_engine, thesis.id, "nonsense")
    with pytest.raises(KeyError):
        theses.set_status(tmp_engine, "nope", "paused")


def test_candidates_are_invisible_until_accepted(tmp_engine):
    thesis = theses.create_thesis(tmp_engine, CLAIM)
    candidate = theses.add_evidence(
        tmp_engine, thesis.id, "news:n1", "support", "Cloud deal reported."
    )

    assert candidate.accepted is False
    assert theses.evidence_for(tmp_engine, thesis.id) == []
    rows = theses.evidence_for(tmp_engine, thesis.id, accepted_only=False)
    assert [row.evidence_id for row in rows] == ["news:n1"]
    assert rows[0].side == "support"

    accepted = theses.set_accepted(tmp_engine, thesis.id, "news:n1", True)
    assert accepted.accepted is True
    visible = theses.evidence_for(tmp_engine, thesis.id)
    assert [row.evidence_id for row in visible] == ["news:n1"]

    theses.set_accepted(tmp_engine, thesis.id, "news:n1", False)
    assert theses.evidence_for(tmp_engine, thesis.id) == []

    with pytest.raises(ValueError, match="side must be one of"):
        theses.add_evidence(tmp_engine, thesis.id, "news:n1", "up", "note")
    with pytest.raises(KeyError):
        theses.add_evidence(tmp_engine, "nope", "news:n1", "support", "note")
    with pytest.raises(KeyError):
        theses.set_accepted(tmp_engine, thesis.id, "news:missing", True)
    with pytest.raises(KeyError):
        theses.remove_evidence(tmp_engine, thesis.id, "news:missing")


def test_propose_evidence_stores_candidates_never_accepts(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,), time_horizon="10y")
    llm = FakeLLM(
        {
            "thesis": {
                "candidates": [
                    {"evidence_id": "news:n1", "side": "support", "note": "Deal reported."},
                    {"evidence_id": "made-up", "side": "support", "note": "Hallucinated."},
                    {"evidence_id": "news:n2", "side": "against", "note": "Rival gains."},
                    {"evidence_id": "news:n1", "side": "support", "note": "Repeat."},
                ]
            }
        }
    )
    rig = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    stored = asyncio.run(theses.propose_evidence(rig, thesis.id))

    assert [row.evidence_id for row in stored] == ["news:n1", "news:n2"]
    assert {row.side for row in stored} == {"support", "against"}
    assert all(row.accepted is False for row in stored)
    assert theses.evidence_for(tmp_engine, thesis.id) == []
    rows = theses.evidence_for(tmp_engine, thesis.id, accepted_only=False)
    assert {row.evidence_id: (row.side, row.note) for row in rows} == {
        "news:n1": ("support", "Deal reported."),
        "news:n2": ("against", "Rival gains."),
    }

    assert len(llm.calls) == 1
    call = llm.calls[0]
    assert call["task"] == "thesis"
    assert call["model"] == "fake/thesis-model"
    assert call["prompt_version"] == "thesis_v1"
    prompt = call["prompt"]
    assert CLAIM in prompt and "10y" in prompt
    assert "news:n1" in prompt and "news:n2" in prompt
    assert "Title n1" in prompt
    assert "m1" not in prompt
    assert "Do not rely on prior knowledge" in prompt
    assert "Only cite ids from the items above" in prompt


def test_propose_evidence_skips_linked_ids(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    llm = FakeLLM(
        {
            "thesis": {
                "candidates": [
                    {"evidence_id": "news:n1", "side": "support", "note": "Deal reported."},
                    {"evidence_id": "news:n2", "side": "against", "note": "Rival gains."},
                ]
            }
        }
    )
    rig = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    first = asyncio.run(theses.propose_evidence(rig, thesis.id))
    assert {row.evidence_id for row in first} == {"news:n1", "news:n2"}
    theses.set_accepted(tmp_engine, thesis.id, "news:n1", True)

    second = asyncio.run(theses.propose_evidence(rig, thesis.id))
    assert second == []
    assert len(llm.calls) == 1
    assert theses.evidence_for(tmp_engine, thesis.id)[0].evidence_id == "news:n1"


def test_propose_evidence_without_evidence_makes_no_call(tmp_engine):
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    llm = FakeLLM({"thesis": {"candidates": []}})
    rig = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    assert asyncio.run(theses.propose_evidence(rig, thesis.id)) == []
    assert llm.calls == []


def test_propose_evidence_without_targets_gathers_all(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM)
    llm = FakeLLM({"thesis": {"candidates": []}})
    rig = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    stored = asyncio.run(theses.propose_evidence(rig, thesis.id))

    assert stored == []
    assert "news:m1" in llm.calls[0]["prompt"]


def test_public_functions_create_tables_idempotently(tmp_engine):
    assert theses.list_theses(tmp_engine) == []

    thesis = theses.create_thesis(tmp_engine, CLAIM)
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "note")
    theses.set_accepted(tmp_engine, thesis.id, "news:n1", True)

    assert theses.list_theses(tmp_engine) == [thesis]
    assert len(theses.evidence_for(tmp_engine, thesis.id)) == 1
    assert theses.get_thesis(tmp_engine, thesis.id) == thesis


def test_theses_screen_smoke(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(
        tmp_engine, "Apple grows cloud revenue.", targets=(INST,), time_horizon="5y"
    )
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "Cloud deal reported.")

    class ThesesApp(App):
        def on_mount(self) -> None:
            self.push_screen(Theses(FakeRig(tmp_engine)))

    async def run():
        app = ThesesApp()
        async with app.run_test() as pilot:
            screen = app.screen
            assert isinstance(screen, Theses)
            assert screen.name == "theses"
            assert screen.query_one("#thesis-table").row_count == 1

            screen.query_one("#th-claim").value = "Microsoft gains cloud share."
            screen.query_one("#th-targets").value = f"{OTHER},{INST}"
            screen.query_one("#th-horizon").value = "5y"
            await pilot.click("#th-add")
            await pilot.pause()
            assert screen.query_one("#thesis-table").row_count == 2
            assert screen.selected is not None
            assert theses.get_thesis(tmp_engine, screen.selected).targets == (OTHER, INST)

            screen.query_one("#thesis-table").focus()
            await pilot.press("enter")
            await pilot.pause()
            assert screen.selected == thesis.id

            assert len(screen.query("#th-accept-0")) == 1
            await pilot.click("#th-accept-0")
            await pilot.pause()
            accepted = theses.evidence_for(tmp_engine, thesis.id)
            assert [row.evidence_id for row in accepted] == ["news:n1"]
            assert len(screen.query("#th-accept-0")) == 0
            detail_text = "\n".join(str(widget.render()) for widget in screen.query("Static"))
            assert "Cloud deal reported." in detail_text
            assert "Supporting" in detail_text

    asyncio.run(run())


def test_get_thesis_accepts_the_truncated_id_the_ui_shows(tmp_engine):
    """The CLI and TUI print ids at 12 chars, so a prefix must resolve."""
    thesis = theses.create_thesis(tmp_engine, CLAIM, scope=SCOPE)

    assert theses.get_thesis(tmp_engine, thesis.id[:12]) == thesis
    with pytest.raises(KeyError, match="unknown thesis"):
        theses.get_thesis(tmp_engine, "nope")


def test_re_linking_evidence_keeps_it_accepted(tmp_engine):
    """Correcting a side or note must not silently drop the item from the thesis."""
    thesis = theses.create_thesis(tmp_engine, CLAIM)
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "note")
    theses.set_accepted(tmp_engine, thesis.id, "news:n1", True)

    updated = theses.add_evidence(tmp_engine, thesis.id, "news:n1", "against", "corrected note")

    assert updated.accepted is True
    visible = theses.evidence_for(tmp_engine, thesis.id)
    assert [(row.side, row.note) for row in visible] == [("against", "corrected note")]
    assert (
        theses.add_evidence(
            tmp_engine, thesis.id, "news:n1", "against", "note", accepted=False
        ).accepted
        is False
    )
    assert theses.evidence_for(tmp_engine, thesis.id) == []


def test_screen_keeps_accepted_evidence_that_aged_out_of_the_pool(tmp_engine):
    """Evidence is fetched by linked id, so the newest-200 pool window can't hide it."""
    _seed_news(tmp_engine)
    seed_bars(tmp_engine, INST, n=250, start=NOW + timedelta(days=1))
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "Cloud deal reported.")
    theses.set_accepted(tmp_engine, thesis.id, "news:n1", True)
    assert "news:n1" not in {item.id for item in evidence(tmp_engine)}

    class ThesesApp(App):
        def on_mount(self) -> None:
            self.push_screen(Theses(FakeRig(tmp_engine)))

    async def run():
        app = ThesesApp()
        async with app.run_test() as pilot:
            app.screen.query_one("#thesis-table").focus()
            await pilot.press("enter")
            await pilot.pause()
            detail_text = "\n".join(str(widget.render()) for widget in app.screen.query("Static"))
            assert "no accepted evidence" not in detail_text
            assert "Title n1 <https://example.com/n1>" in detail_text

    asyncio.run(run())
