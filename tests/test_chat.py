"""Tests for grounded chat: citation verification, web gating, and the chat screen.

``FakeChatLLM`` duck-types ``LLMClient.chat`` and returns queued ChatDraft JSON
payloads; ``CountingSearch`` counts every query so the web gate is assertable.
``_seed`` writes three US:AAPL bars (autoincrement ids 1-3) so "bar:1" is a real
evidence id citations can resolve to.
"""

from __future__ import annotations

import asyncio
import json
from typing import Any

import pytest
from sqlmodel import Session, select
from textual.app import App
from textual.widgets import Input

from rigger.chat import ChatMessage, OfflineSearchTool, WebHit, chat
from rigger.core.db import BarTable, EventTable, FundamentalTable, LLMCallTable, NewsItemTable
from rigger.evidence import evidence
from rigger.llm.client import LLMClient, LLMResult
from rigger.llm.providers import ProviderResult
from rigger.tui.screens.chat import Chat
from rigger.tui.widgets import ActionChip, Pane, RiggerTable
from tests.conftest import FakeConfig, seed_bars

INST = "US:AAPL"
ROUTING = {"chat": "test/chat-model"}


class FakeChatLLM:
    """Offline stand-in for LLMClient.chat returning queued draft payloads."""

    def __init__(self, drafts: list[dict[str, Any]]) -> None:
        self.drafts = list(drafts)
        self.calls: list[dict[str, Any]] = []

    async def chat(self, **kwargs: Any) -> LLMResult:
        self.calls.append(kwargs)
        payload = self.drafts.pop(0)
        return LLMResult(
            text=json.dumps(payload),
            call_id=f"fake-{len(self.calls)}",
            cost_usd=0.0,
            cached=False,
        )


class FakeRig:
    """The slices of the runtime chat reads: engine, llm, and routed config."""

    def __init__(self, engine: Any, llm: Any, routing: dict[str, str]) -> None:
        self.engine = engine
        self.llm = llm
        self.cfg = FakeConfig(llm_routing=routing)


class CountingSearch:
    """SearchTool stub that records queries and returns a fixed hit list."""

    def __init__(self, hits: list[WebHit] | None = None) -> None:
        self.queries: list[str] = []
        self.hits = hits or []

    async def search(self, query: str) -> list[WebHit]:
        self.queries.append(query)
        return list(self.hits)


class FakeProvider:
    """Provider stub for exercising LLMClient.chat itself."""

    def __init__(self) -> None:
        self.calls: list[dict[str, Any]] = []

    async def complete(self, **kwargs: Any) -> ProviderResult:
        self.calls.append(kwargs)
        return ProviderResult(text="ok", input_tokens=1, output_tokens=1, cost_usd=0.0)


def _history(*texts: str) -> list[ChatMessage]:
    return [ChatMessage(role="user", text=text, source="user") for text in texts]


def _row_counts(engine: Any) -> dict[str, int]:
    tables: dict[str, Any] = {
        "bar": BarTable,
        "newsitem": NewsItemTable,
        "event": EventTable,
        "fundamental": FundamentalTable,
        "llmcall": LLMCallTable,
    }
    with Session(engine) as session:
        return {name: len(session.exec(select(table)).all()) for name, table in tables.items()}


def test_stored_citations_resolve_to_seeded_evidence(tmp_engine):
    seed_bars(tmp_engine, INST, n=3)
    llm = FakeChatLLM([{"answer": "AAPL closed at 100.00.", "citations": ["bar:1", "bar:2"]}])
    rig = FakeRig(tmp_engine, llm, ROUTING)

    reply = asyncio.run(chat(rig, _history("How did AAPL do?"), targets=[INST]))

    assert reply.role == "assistant"
    assert reply.source == "stored"
    assert reply.citations == ("bar:1", "bar:2")
    assert "could not be supported" not in reply.text
    assert len(llm.calls) == 1
    sent = llm.calls[0]["messages"]
    assert sent[0]["role"] == "system"
    assert "Web search is disabled" in sent[0]["content"]
    assert "- bar:1: [test] US:AAPL close 100.00" in sent[1]["content"]
    assert sent[-1] == {"role": "user", "content": "How did AAPL do?"}


def test_unmatched_citations_are_dropped_and_noted(tmp_engine):
    seed_bars(tmp_engine, INST, n=3)
    llm = FakeChatLLM([{"answer": "Solar demand is booming.", "citations": ["bogus:9"]}])
    rig = FakeRig(tmp_engine, llm, ROUTING)

    reply = asyncio.run(chat(rig, _history("What about solar?"), targets=[INST]))

    assert reply.source == "inference"
    assert reply.citations == ()
    assert "Solar demand is booming." in reply.text
    assert "could not be supported from your stored data" in reply.text


def test_partially_verified_answer_is_inference_without_the_note(tmp_engine):
    seed_bars(tmp_engine, INST, n=3)
    llm = FakeChatLLM([{"answer": "Partly grounded.", "citations": ["bar:1", "bogus:9"]}])
    rig = FakeRig(tmp_engine, llm, ROUTING)

    reply = asyncio.run(chat(rig, _history("Anything?"), targets=[INST]))

    assert reply.source == "inference"
    assert reply.citations == ("bar:1",)
    assert "could not be supported" not in reply.text


def test_web_disabled_never_calls_the_search_tool(tmp_engine):
    seed_bars(tmp_engine, INST, n=3)
    llm = FakeChatLLM(
        [{"answer": "Grounded.", "citations": ["bar:1"], "web_queries": ["solar demand"]}]
    )
    tool = CountingSearch()
    rig = FakeRig(tmp_engine, llm, ROUTING)

    reply = asyncio.run(
        chat(rig, _history("Anything?"), targets=[INST], allow_web=False, search=tool)
    )

    assert tool.queries == []
    assert len(llm.calls) == 1
    assert reply.source == "stored"


def test_web_enabled_labels_web_citations_and_persists_nothing(tmp_engine):
    seed_bars(tmp_engine, INST, n=3)
    hits = [
        WebHit(
            title="Solar demand soars",
            url="https://example.com/solar",
            snippet="Quarterly demand up 20%.",
        )
    ]
    llm = FakeChatLLM(
        [
            {"answer": "Checking the web.", "citations": [], "web_queries": ["solar demand"]},
            {"answer": "Demand is up 20%.", "citations": ["https://example.com/solar"]},
        ]
    )
    tool = CountingSearch(hits)
    rig = FakeRig(tmp_engine, llm, ROUTING)
    before = _row_counts(tmp_engine)

    reply = asyncio.run(
        chat(rig, _history("Anything?"), targets=[INST], allow_web=True, search=tool)
    )

    assert tool.queries == ["solar demand"]
    assert reply.source == "web"
    assert reply.citations == ("https://example.com/solar",)
    assert _row_counts(tmp_engine) == before
    assert evidence(tmp_engine, kind="web") == []
    assert len(llm.calls) == 2
    first, second = llm.calls[0]["messages"], llm.calls[1]["messages"]
    assert "Web search is enabled" in first[0]["content"]
    assert any("- https://example.com/solar: Solar demand soars" in m["content"] for m in second)
    assert not any("Web search results" in m["content"] for m in first)


def test_unrouted_chat_task_raises_loud_keyerror(tmp_engine):
    rig = FakeRig(tmp_engine, FakeChatLLM([]), {"analyse": "test/model"})

    with pytest.raises(KeyError, match=r"no model routed for task 'chat'"):
        asyncio.run(chat(rig, _history("Anything?"), targets=[INST]))


def test_offline_search_tool_returns_no_hits():
    assert asyncio.run(OfflineSearchTool().search("anything")) == []


def test_client_chat_delegates_logs_and_caches(tmp_engine):
    provider = FakeProvider()
    client = LLMClient(provider=provider, engine=tmp_engine)
    messages = [
        {"role": "system", "content": "sys"},
        {"role": "user", "content": "hi"},
    ]

    first = asyncio.run(
        client.chat(task="chat", model="m", prompt_version="chat_v1", messages=messages)
    )
    assert first.text == "ok"
    assert first.cached is False
    assert provider.calls[0]["messages"] == messages
    with Session(tmp_engine) as session:
        rows = session.exec(select(LLMCallTable)).all()
    assert len(rows) == 1
    assert rows[0].task == "chat"
    assert rows[0].model == "m"

    second = asyncio.run(
        client.chat(task="chat", model="m", prompt_version="chat_v1", messages=messages)
    )
    assert second.cached is True
    assert len(provider.calls) == 1


def _write_targets(tmp_path, monkeypatch) -> None:
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[targets.aapl]\nkind = "company"\nmarket = "us"\ntickers = ["AAPL"]\n',
        encoding="utf-8",
    )


def test_chat_screen_round_trip(tmp_engine, monkeypatch, tmp_path):
    _write_targets(tmp_path, monkeypatch)
    seed_bars(tmp_engine, INST, n=3)
    llm = FakeChatLLM([{"answer": "AAPL closed at 100.00.", "citations": ["bar:1"]}])
    rig = FakeRig(tmp_engine, llm, ROUTING)

    async def run() -> None:
        app = App()
        async with app.run_test(size=(120, 40)) as pilot:
            screen = Chat(rig)
            app.push_screen(screen)
            await pilot.pause()
            assert screen.name == "chat"
            table = screen.query_one("#chat-targets", RiggerTable)
            assert table.row_count == 1
            # Every configured target starts in scope; the header says so.
            assert screen.scope == {"aapl"}
            assert "1 of 1 in scope" in str(screen.query_one("#chat-scope-count").render())
            await pilot.press("i")
            assert screen.query_one("#chat-input", Input).has_focus
            screen.query_one("#chat-input", Input).value = "How did AAPL do?"
            await pilot.press("enter")
            for _ in range(100):
                if len(screen.history) >= 2:
                    break
                await pilot.pause(0.05)
            assert [message.role for message in screen.history] == ["user", "assistant"]
            reply = screen.history[1]
            assert reply.source == "stored"
            assert reply.citations == ("bar:1",)
            assert screen.query_one("#chat-input", Input).value == ""
            assert screen.allow_web is False
            await pilot.pause()
            transcript = "\n".join(
                str(widget.render()) for widget in screen.query("#chat-scroll Static")
            )
            assert "How did AAPL do?" in transcript
            assert "AAPL closed at 100.00." in transcript
            assert "[1]" in transcript
            assert "1 citation" in transcript
            assert "2 messages" in screen.query_one("#chat-main", Pane).border_title

    asyncio.run(run())


def test_chat_screen_keys_drive_scope_web_citations_and_clear(tmp_engine, monkeypatch, tmp_path):
    _write_targets(tmp_path, monkeypatch)
    seed_bars(tmp_engine, INST, n=3)
    llm = FakeChatLLM([{"answer": "Two bars.", "citations": ["bar:1", "bar:2"]}])
    rig = FakeRig(tmp_engine, llm, ROUTING)

    async def run() -> None:
        app = App()
        async with app.run_test(size=(120, 40)) as pilot:
            screen = Chat(rig)
            app.push_screen(screen)
            await pilot.pause()
            # Letters typed into the prompt stay in the prompt.
            await pilot.press("i")
            await pilot.press("w", "x", "t", "space", "a")
            assert screen.query_one("#chat-input", Input).value == "wxt a"
            assert screen.allow_web is False
            await pilot.press("escape")
            assert not screen.query_one("#chat-input", Input).has_focus
            screen.query_one("#chat-input", Input).value = ""
            # Targets: t focuses, space toggles, a flips all.
            await pilot.press("t")
            assert screen.query_one("#chat-targets", RiggerTable).has_focus
            await pilot.press("space")
            assert screen.scope == set()
            await pilot.press("a")
            assert screen.scope == {"aapl"}
            # Web toggle is a chip, not a Switch.
            await pilot.press("w")
            assert screen.allow_web is True
            assert "web search: on" in str(screen.query_one("#chat-web", ActionChip).label)
            await pilot.press("w")
            # Ask, then walk the citations of the answer.
            await pilot.press("enter")
            screen.query_one("#chat-input", Input).value = "Bars?"
            await pilot.press("enter")
            for _ in range(100):
                if len(screen.history) >= 2:
                    break
                await pilot.pause(0.05)
            await pilot.press("escape")
            assert screen.citations() == ("bar:1", "bar:2")
            assert screen.selected_citation == 0
            await pilot.press("right")
            assert screen.selected_citation == 1
            await pilot.press("right")
            assert screen.selected_citation == 0
            await pilot.press("left")
            assert screen.selected_citation == 1
            # Clear is two-step: x arms, esc cancels, x then y clears.
            await pilot.press("x")
            assert screen.confirm_pending
            assert "confirm" in screen.query_one("#chat-main", Pane).border_subtitle
            await pilot.press("escape")
            assert not screen.confirm_pending
            assert len(screen.history) == 2
            await pilot.press("x", "y")
            await pilot.pause()
            assert screen.history == []
            assert screen.query_one("#chat-main", Pane).border_title.endswith("ask")

    asyncio.run(run())
