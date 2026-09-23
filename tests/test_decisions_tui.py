"""Smoke coverage for the decision-journal panel."""

from __future__ import annotations

import asyncio
from datetime import date

from textual.app import App

from delta import decisions
from delta.tui.screens.decisions import Decisions
from tests.conftest import FakeConfig


class FakeRig:
    def __init__(self, engine):
        self.engine = engine
        self.cfg = FakeConfig()

    def universe(self):
        from delta.core.models import Instrument

        return [Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD")]


def test_decisions_screen_lists_records_and_opens_research(tmp_engine):
    decision = decisions.create_decision(
        tmp_engine,
        "US:AAPL",
        "Services revenue grows.",
        "20x forward earnings.",
        "5 years",
        date(2026, 4, 1),
        "Services growth turns negative.",
    )

    class DecisionApp(App):
        def __init__(self):
            super().__init__()
            self.switched = ""

        def action_switch_screen(self, name: str) -> None:
            self.switched = name

        def on_mount(self) -> None:
            self.push_screen(Decisions(FakeRig(tmp_engine)))

    async def check() -> None:
        app = DecisionApp()
        async with app.run_test() as pilot:
            await pilot.pause()
            screen = app.screen
            assert screen.name == "decisions"
            assert screen.query_one("#decisions-table").row_count == 1
            detail = "\n".join(str(widget.render()) for widget in screen.query("#decision-detail Static"))
            assert "Services revenue grows" in detail
            screen.selected = decision.id
            screen.action_open_research()
            assert app.switched == "data"
            assert screen.state.company == "US:AAPL"

    asyncio.run(check())


def test_decisions_screen_filters_rows(tmp_engine):
    decisions.create_decision(
        tmp_engine,
        "US:AAPL",
        "Apple services",
        "20x forward earnings.",
        "5 years",
        date(2026, 4, 1),
        "Services growth turns negative.",
    )
    decisions.create_decision(
        tmp_engine,
        "US:MSFT",
        "Microsoft cloud",
        "30x forward earnings.",
        "5 years",
        date(2026, 4, 1),
        "Cloud growth turns negative.",
    )

    class DecisionApp(App):
        def on_mount(self) -> None:
            self.push_screen(Decisions(FakeRig(tmp_engine)))

    async def check() -> None:
        app = DecisionApp()
        async with app.run_test() as pilot:
            await pilot.pause()
            screen = app.screen
            assert screen.query_one("#decisions-table").row_count == 2
            screen.query_one("#decisions-filter").value = "msft"
            screen._fill_table()
            assert screen.query_one("#decisions-table").row_count == 1

    asyncio.run(check())


def test_decisions_screen_delete_with_confirm(tmp_engine):
    decision = decisions.create_decision(
        tmp_engine,
        "US:AAPL",
        "Services revenue grows.",
        "20x forward earnings.",
        "5 years",
        date(2026, 4, 1),
        "Services growth turns negative.",
    )

    class DecisionApp(App):
        def on_mount(self) -> None:
            self.push_screen(Decisions(FakeRig(tmp_engine)))

    async def check() -> None:
        app = DecisionApp()
        async with app.run_test() as pilot:
            await pilot.pause()
            screen = app.screen
            screen.selected = decision.id
            assert screen._confirm_delete is False
            screen.action_delete_decision()
            assert screen._confirm_delete is True
            screen.action_back()
            assert screen._confirm_delete is False
            screen.action_delete_decision()
            await screen.action_confirm_delete()
            assert decisions.list_decisions(tmp_engine) == []

    asyncio.run(check())
