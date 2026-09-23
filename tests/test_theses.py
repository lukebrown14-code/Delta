"""Tests for the optional thesis layer: CRUD, candidates vs accepted, discovery."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

import pytest
from sqlmodel import Session
from textual.app import App
from textual.containers import VerticalScroll
from textual.content import Content
from textual.widgets import Input

from delta import theses
from delta.core.db import NewsItemTable
from delta.core.json import to_json
from delta.evidence import evidence
from delta.tui.screens.theses import Theses
from delta.tui.widgets import Pane
from tests.conftest import FakeConfig, FakeLLM, seed_bars

INST = "US:AAPL"
OTHER = "US:MSFT"
NOW = datetime(2026, 3, 22, 12, 0, tzinfo=UTC)

CLAIM = "Solar panels grow as a share of electricity."
SCOPE = "Global electricity generation"


class FakeRig:
    """Duck-typed stand-in for Delta: engine, routed config, LLM client."""

    def __init__(self, engine, llm=None, cfg=None):
        self.engine = engine
        self.llm = llm
        self.cfg = cfg if cfg is not None else FakeConfig()


def _hints(screen: Theses, pane: str = "#thesis-evidence-pane") -> str:
    """A pane's bottom-border key hints as plain text (markup stripped)."""
    return Content.from_markup(screen.query_one(pane, Pane).border_subtitle or "").plain


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


def test_update_thesis_changes_claim_and_preserves_evidence(tmp_engine):
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,), time_horizon="5y")
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "Evidence", accepted=True)

    updated = theses.update_thesis(
        tmp_engine,
        thesis.id,
        claim="Solar generation keeps growing.",
        targets=(INST, OTHER),
        time_horizon="10y",
        status="paused",
    )

    assert updated.id != thesis.id
    assert updated.targets == (INST, OTHER)
    assert updated.time_horizon == "10y"
    assert updated.status == "paused"
    assert theses.evidence_for(tmp_engine, updated.id)[0].note == "Evidence"
    with pytest.raises(KeyError):
        theses.get_thesis(tmp_engine, thesis.id)


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
    delta = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    stored = asyncio.run(theses.propose_evidence(delta, thesis.id))

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
    delta = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    first = asyncio.run(theses.propose_evidence(delta, thesis.id))
    assert {row.evidence_id for row in first} == {"news:n1", "news:n2"}
    theses.set_accepted(tmp_engine, thesis.id, "news:n1", True)

    second = asyncio.run(theses.propose_evidence(delta, thesis.id))
    assert second == []
    assert len(llm.calls) == 1
    assert theses.evidence_for(tmp_engine, thesis.id)[0].evidence_id == "news:n1"


def test_propose_evidence_without_evidence_makes_no_call(tmp_engine):
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    llm = FakeLLM({"thesis": {"candidates": []}})
    delta = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    assert asyncio.run(theses.propose_evidence(delta, thesis.id)) == []
    assert llm.calls == []


def test_propose_evidence_without_targets_gathers_all(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM)
    llm = FakeLLM({"thesis": {"candidates": []}})
    delta = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    stored = asyncio.run(theses.propose_evidence(delta, thesis.id))

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

            await pilot.press("n")
            app.screen.query_one("#th-claim").value = "Microsoft gains cloud share."
            app.screen.query_one("#th-targets").value = f"{OTHER},{INST}"
            app.screen.query_one("#th-horizon").value = "5y"
            await pilot.click("#th-save")
            await pilot.pause()
            assert screen.query_one("#thesis-table").row_count == 2
            assert screen.selected is not None
            assert theses.get_thesis(tmp_engine, screen.selected).targets == (OTHER, INST)

            screen.query_one("#thesis-table").focus()
            await pilot.press("up", "enter")
            await pilot.pause()
            assert screen.selected == thesis.id

            await pilot.press("e")
            # The ledger's border hints offer what applies to the highlighted row.
            assert "a accept" in _hints(screen)
            await pilot.press("a")
            await pilot.pause()
            accepted = theses.evidence_for(tmp_engine, thesis.id)
            assert [row.evidence_id for row in accepted] == ["news:n1"]
            actions = _hints(screen)
            assert "a accept" not in actions
            assert "u un-accept" in actions
            detail_text = "\n".join(str(widget.render()) for widget in screen.query("Static"))
            assert "Cloud deal reported." in detail_text
            assert "Supporting" in detail_text

    asyncio.run(run())


def test_get_thesis_accepts_the_truncated_id_the_ui_shows(tmp_engine):
    """The TUI prints ids at 12 chars, so a prefix must resolve."""
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


def test_research_desk_resize_and_keyboard_review(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "against", "Counter evidence")
    theses.add_evidence(
        tmp_engine, thesis.id, "news:n2", "support", "Accepted evidence", accepted=True
    )

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            assert screen.query_one("#thesis-detail-pane").display
            assert screen.query_one("#thesis-evidence-pane").display
            screen.query_one("#thesis-ledger").focus()
            await pilot.press("x")
            assert [
                row.evidence_id
                for row in theses.evidence_for(tmp_engine, thesis.id, accepted_only=False)
            ] == ["news:n2"]
            await pilot.press("x")
            assert len(theses.evidence_for(tmp_engine, thesis.id)) == 1
            await pilot.resize_terminal(80, 24)
            assert not screen.query_one("#thesis-evidence-pane").display
            await pilot.press("e")
            assert screen.query_one("#thesis-evidence-pane").display
            assert not screen.query_one("#thesis-detail-pane").display
            await pilot.resize_terminal(130, 32)
            assert screen.query_one("#thesis-detail-pane").display
            assert screen.query_one("#thesis-evidence-pane").display

    asyncio.run(run())


def test_empty_desk_and_cancel_creation(tmp_engine):
    async def run():
        app = App()
        async with app.run_test() as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            assert screen.selected is None
            await pilot.press("n")
            app.screen.query_one("#th-claim").value = "Unsaved claim"
            await pilot.press("escape")
            assert app.screen is screen
            assert theses.list_theses(tmp_engine) == []
            await pilot.press("e", "a", "x")
            assert screen.query_one("#thesis-ledger").row_count == 0

    asyncio.run(run())


def test_screen_edit_button_updates_selected_thesis(tmp_engine):
    theses.create_thesis(tmp_engine, CLAIM, targets=(INST,), time_horizon="5y")

    async def run():
        app = App()
        async with app.run_test() as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()

            await pilot.press("d")
            assert app.screen.query_one("#th-claim", Input).value == CLAIM
            app.screen.query_one("#th-claim", Input).value = "Solar demand keeps rising."
            app.screen.query_one("#th-targets", Input).value = f"{INST}, {OTHER}"
            app.screen.query_one("#th-horizon", Input).value = "10y"
            await pilot.click("#th-save")
            await pilot.pause()

            assert app.screen is screen
            assert screen.selected is not None
            updated = theses.get_thesis(tmp_engine, screen.selected)
            assert updated.claim == "Solar demand keeps rising."
            assert updated.targets == (INST, OTHER)
            assert updated.time_horizon == "10y"

    asyncio.run(run())


def test_update_thesis_edits_scope_and_framing_keeping_evidence(tmp_engine):
    """Scope is part of the id, so editing it must migrate evidence like a claim edit."""
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,), scope=SCOPE)
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "Kept", accepted=True)

    updated = theses.update_thesis(
        tmp_engine,
        thesis.id,
        claim=CLAIM,
        targets=(INST,),
        time_horizon="5y",
        status="active",
        scope="Australian electricity generation",
        assumptions=("Panel costs keep falling",),
        falsifiers=("subsidy repeal",),
    )

    assert updated.id != thesis.id
    assert updated.scope == "Australian electricity generation"
    assert updated.assumptions == ["Panel costs keep falling"]
    assert updated.falsifiers == ["subsidy repeal"]
    rows = theses.evidence_for(tmp_engine, updated.id)
    assert [row.evidence_id for row in rows] == ["news:n1"]
    assert theses.list_theses(tmp_engine) == [updated]


def test_update_thesis_leaves_framing_alone_when_not_given(tmp_engine):
    """A caller that does not collect a field must not be able to erase it."""
    thesis = theses.create_thesis(
        tmp_engine, CLAIM, scope=SCOPE, assumptions=("a",), falsifiers=("b",)
    )

    updated = theses.update_thesis(
        tmp_engine, thesis.id, claim=CLAIM, targets=(), time_horizon="", status="paused"
    )

    assert updated.id == thesis.id
    assert updated.scope == SCOPE
    assert updated.assumptions == ["a"]
    assert updated.falsifiers == ["b"]


def test_screen_find_evidence_fills_the_review_queue(tmp_engine):
    """`f` is the only way the review queue can fill from the TUI."""
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
    delta = FakeRig(tmp_engine, llm, FakeConfig({"thesis": "fake/thesis-model"}))

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(delta)
            await app.push_screen(screen)
            await pilot.pause()
            assert screen.query_one("#thesis-ledger").row_count == 0

            await pilot.press("f")
            await pilot.pause()
            await pilot.pause()

            assert screen.query_one("#thesis-ledger").row_count == 2
            stored = theses.evidence_for(tmp_engine, thesis.id, accepted_only=False)
            assert all(row.accepted is False for row in stored)
            assert theses.evidence_for(tmp_engine, thesis.id) == []

    asyncio.run(run())


def test_screen_unaccept_returns_evidence_to_pending(tmp_engine):
    """Accepted evidence is removed in two steps, so `x` alone cannot delete it."""
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "Kept", accepted=True)

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            screen.query_one("#thesis-ledger").focus()

            # x on an accepted row is refused, so the evidence survives.
            await pilot.press("x")
            await pilot.pause()
            assert len(theses.evidence_for(tmp_engine, thesis.id)) == 1

            await pilot.press("u")
            await pilot.pause()
            assert theses.evidence_for(tmp_engine, thesis.id) == []
            pending = theses.evidence_for(tmp_engine, thesis.id, accepted_only=False)
            assert [row.evidence_id for row in pending] == ["news:n1"]

            # Now pending, it can be dropped.
            await pilot.press("x")
            await pilot.pause()
            assert theses.evidence_for(tmp_engine, thesis.id, accepted_only=False) == []

    asyncio.run(run())


def test_screen_filter_and_escape_return_to_the_claims_table(tmp_engine):
    theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.create_thesis(tmp_engine, "Microsoft gains cloud share.", targets=(OTHER,))

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            assert screen.query_one("#thesis-table").row_count == 2

            await pilot.press("slash")
            await pilot.pause()
            assert screen.query_one("#thesis-filter").display
            screen.query_one("#thesis-filter", Input).value = "microsoft"
            await pilot.pause()
            assert screen.query_one("#thesis-table").row_count == 1

            await pilot.press("escape")
            await pilot.pause()
            assert not screen.query_one("#thesis-filter").display
            assert screen.query_one("#thesis-table").row_count == 2
            assert screen.query_one("#thesis-table").has_focus

    asyncio.run(run())


def test_pane_hotkeys_focus_their_pane_and_escape_returns(tmp_engine):
    """`e` is the ledger's hotkey, `t` the thesis pane's; `esc` goes back to the list."""
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "A note")

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()

            await pilot.press("e")
            assert screen.query_one("#thesis-ledger").has_focus
            await pilot.press("e")
            assert screen.query_one("#thesis-ledger").has_focus
            await pilot.press("t")
            assert screen.query_one("#thesis-detail").has_focus
            await pilot.press("escape")
            assert screen.query_one("#thesis-table").has_focus

    asyncio.run(run())


def test_tab_cycles_exactly_the_three_panes(tmp_engine):
    """Tab means "next pane": nothing in the chain that is not a pane."""
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "A note")

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()

            assert [w.id for w in screen.focus_chain] == [
                "thesis-table",
                "thesis-detail",
                "thesis-ledger",
            ]
            visited = []
            for _ in range(4):
                visited.append(app.focused.id)
                await pilot.press("tab")
                await pilot.pause()
            assert visited == ["thesis-table", "thesis-detail", "thesis-ledger", "thesis-table"]

    asyncio.run(run())


def test_action_line_tracks_the_highlighted_row(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "Pending one")
    theses.add_evidence(tmp_engine, thesis.id, "news:n2", "against", "Kept", accepted=True)

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            ledger = screen.query_one("#thesis-ledger")
            ledger.focus()

            # Pending rows sort first, so the cursor starts on one.
            assert "a accept" in _hints(screen)
            assert "x reject" in _hints(screen)
            assert "f find" in _hints(screen)

            await pilot.press("down")
            await pilot.pause()
            assert "u un-accept" in _hints(screen)
            assert "a accept" not in _hints(screen)
            assert "f find" in _hints(screen)

    asyncio.run(run())


def test_shift_arrows_scroll_the_note_only_from_the_ledger(tmp_engine):
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "A long note. " * 20)

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            preview = screen.query_one("#thesis-preview", VerticalScroll)
            assert preview.max_scroll_y > 0
            assert "⇧↕ note" in _hints(screen)

            screen.query_one("#thesis-ledger").focus()
            await pilot.press("shift+down")
            await pilot.pause()
            assert preview.scroll_y == 1
            # Scrolling the note must not cost the ledger its focus.
            assert screen.query_one("#thesis-ledger").has_focus

            screen.query_one("#thesis-table").focus()
            await pilot.press("shift+down")
            await pilot.pause()
            assert preview.scroll_y == 1

    asyncio.run(run())


def test_pane_width_constants_match_the_stylesheet():
    """The breakpoint is derived from the pane widths, so the CSS must agree.

    Textual CSS cannot read a Python constant, so the numbers are written twice.
    This is the mechanism that keeps the two copies honest.
    """
    from delta.tui.screens import theses as screen

    assert f"#thesis-claims {{ width: {screen.CLAIMS_WIDTH}; }}" in screen.Theses.CSS
    assert f"#thesis-evidence-pane {{ width: {screen.EVIDENCE_WIDTH}; }}" in screen.Theses.CSS


def test_border_hints_follow_the_layout_both_ways(tmp_engine):
    """Narrow adds enter/e/esc to the borders; widening takes them away again."""
    theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))

    async def run():
        app = App()
        async with app.run_test(size=(130, 32)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            wide = _hints(screen, "#thesis-claims")
            assert "n new" in wide and "enter thesis" not in wide
            assert "esc back" not in _hints(screen)

            await pilot.resize_terminal(80, 24)
            await pilot.pause()
            narrow = _hints(screen, "#thesis-claims")
            assert "enter thesis" in narrow and "e evidence" in narrow
            assert "esc back" in _hints(screen)
            assert "esc back" in _hints(screen, "#thesis-detail-pane")

            await pilot.resize_terminal(130, 32)
            await pilot.pause()
            assert _hints(screen, "#thesis-claims") == wide

    asyncio.run(run())


def test_narrow_enter_opens_the_thesis_and_escape_steps_back(tmp_engine):
    """At 80 columns the list owns the screen; enter and e open the other panes full-width."""
    _seed_news(tmp_engine)
    thesis = theses.create_thesis(tmp_engine, CLAIM, targets=(INST,))
    theses.add_evidence(tmp_engine, thesis.id, "news:n1", "support", "A note")

    async def run():
        app = App()
        async with app.run_test(size=(80, 24)) as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            await pilot.pause()
            assert screen.query_one("#thesis-claims").display
            assert not screen.query_one("#thesis-detail-pane").display
            assert not screen.query_one("#thesis-evidence-pane").display

            await pilot.press("enter")
            await pilot.pause()
            assert screen.query_one("#thesis-detail-pane").display
            assert not screen.query_one("#thesis-claims").display
            assert screen.query_one("#thesis-detail").has_focus

            await pilot.press("e")
            await pilot.pause()
            assert screen.query_one("#thesis-evidence-pane").display
            assert not screen.query_one("#thesis-detail-pane").display
            assert screen.query_one("#thesis-ledger").has_focus

            await pilot.press("escape")
            await pilot.pause()
            assert screen.query_one("#thesis-claims").display
            assert not screen.query_one("#thesis-evidence-pane").display
            assert screen.query_one("#thesis-table").has_focus

    asyncio.run(run())
