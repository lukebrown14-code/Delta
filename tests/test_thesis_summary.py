"""AI running summary for a thesis, grounded in accepted evidence."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

import pytest
from sqlmodel import Session

from delta.core.db import NewsItemTable
from delta.theses import add_evidence, create_thesis
from delta.thesis_summary import summarize_thesis
from tests.conftest import FakeConfig

NOW = datetime.now(UTC)


class FakeRig:
    def __init__(self, engine, llm, cfg=None):
        self.engine = engine
        self.llm = llm
        self.cfg = cfg if cfg is not None else FakeConfig({"thesis_summary": "test/model"})


def _seed_news(engine, ids, days=1):
    with Session(engine) as session:
        for nid in ids:
            session.add(
                NewsItemTable(
                    id=nid,
                    instrument_ids='["US:AAPL"]',
                    published=NOW - timedelta(days=days),
                    title=f"Headline {nid}",
                    url=f"https://example.com/{nid}",
                    source="rss",
                )
            )
        session.commit()


def _make_thesis(engine, *, targets=("US:AAPL",), accept=4):
    thesis = create_thesis(engine, "Apple services revenue keeps growing", targets=targets)
    _seed_news(engine, [f"n{i}" for i in range(6)])
    for i in range(accept):
        add_evidence(
            engine, thesis.id, f"news:n{i}", "support", note=f"supports {i}", accepted=True
        )
    add_evidence(engine, thesis.id, "news:n5", "against", note="cuts against", accepted=True)
    return thesis


def _run(delta, thesis_id):
    return asyncio.run(summarize_thesis(delta, thesis_id))


def test_summarize_cites_only_accepted_ids(tmp_engine, fake_llm):
    thesis = _make_thesis(tmp_engine, accept=4)
    draft = {
        "summary": "The evidence leans supportive [news:n0] [news:n1].",
        "strongest_support": "n0 supports [news:n0]",
        "strongest_counter": "n5 cuts against [news:n5]",
        "unknowns": ["What about competition?"],
        "citations": ["news:n0", "news:n1", "news:ghost", "news:notaccepted"],
    }
    fake_llm._responses = {"thesis_summary": draft}
    summary = _run(FakeRig(tmp_engine, fake_llm), thesis.id)
    assert summary.state == "building"
    assert summary.citations == ("news:n0", "news:n1")
    assert "news:ghost" not in summary.citations
    assert summary.strongest_support


def test_summarize_no_accepted_evidence_skips_llm(tmp_engine, fake_llm):
    thesis = create_thesis(tmp_engine, "No evidence yet", targets=("US:AAPL",))
    summary = _run(FakeRig(tmp_engine, fake_llm), thesis.id)
    assert summary.state == "emerging"
    assert "No accepted evidence" in summary.summary
    assert fake_llm.calls == []


def test_summarize_prompt_contains_state_and_items(tmp_engine, fake_llm):
    thesis = _make_thesis(tmp_engine, accept=4)
    fake_llm._responses = {
        "thesis_summary": {"summary": "s [news:n0]", "citations": ["news:n0"], "unknowns": []}
    }
    _run(FakeRig(tmp_engine, fake_llm), thesis.id)
    prompt = fake_llm.calls[0]["prompt"]
    assert "building" in prompt
    assert "news:n0" in prompt
    assert "Do not recommend buying" in prompt


def test_summarize_unrouted_task_raises(tmp_engine, fake_llm):
    thesis = _make_thesis(tmp_engine, accept=4)
    delta = FakeRig(tmp_engine, fake_llm, cfg=FakeConfig({}))
    with pytest.raises(KeyError, match="thesis_summary"):
        _run(delta, thesis.id)


def test_screen_summarise_button_renders_summary(tmp_engine, fake_llm):
    from textual.app import App

    from delta.tui.screens.theses import Theses

    thesis = _make_thesis(tmp_engine, accept=4)
    fake_llm._responses = {
        "thesis_summary": {
            "summary": "Services momentum is intact [news:n0].",
            "strongest_support": "n0 highlights growth [news:n0]",
            "strongest_counter": "",
            "unknowns": ["competitive pressure"],
            "citations": ["news:n0"],
        }
    }

    async def run():
        app = App()
        async with app.run_test():
            screen = Theses(FakeRig(tmp_engine, fake_llm))
            await app.push_screen(screen)
            screen.selected = thesis.id
            await screen.render_detail()
            await screen._summarise()
            detail_text = "\n".join(str(widget.render()) for widget in screen.query("Static"))
            assert "Services momentum is intact" in detail_text
            assert "competitive pressure" in detail_text

    asyncio.run(run())
