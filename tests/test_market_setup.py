"""Offline tests for the market setup modal's autocomplete."""

from __future__ import annotations

import asyncio

from textual.app import App
from textual.widgets import Input, OptionList

from delta.tui.screens.market_setup import MarketSetupModal


def test_market_autocomplete_adopts_then_saves():
    async def run():
        app = App()
        async with app.run_test() as pilot:
            modal = MarketSetupModal()
            app.push_screen(modal)
            await pilot.pause()
            ident = modal.query_one("#market-id", Input)
            ident.focus()
            for char in "lon":
                await pilot.press(char)
            await pilot.pause()
            suggestions = modal.query_one("#market-suggestions", OptionList)
            assert [option.id for option in suggestions.options] == ["lse"]
            await pilot.press("enter")  # adopt
            await pilot.pause()
            assert modal.query_one("#market-id", Input).value == "lse"
            assert modal.query_one("#market-label", Input).value == "London Stock Exchange"
            assert modal.query_one("#market-currency", Input).value == "GBP"
            assert modal.query_one("#market-suffix", Input).value == ".L"
            assert not suggestions.options  # dropdown cleared after adopt
            await pilot.press("enter")  # confirm save
            await pilot.pause()
            assert app.screen is not modal

    asyncio.run(run())


def test_market_escape_closes_dropdown_first():
    async def run():
        app = App()
        async with app.run_test() as pilot:
            modal = MarketSetupModal()
            app.push_screen(modal)
            await pilot.pause()
            ident = modal.query_one("#market-id", Input)
            ident.focus()
            for char in "lon":
                await pilot.press(char)
            await pilot.pause()
            assert modal.query_one("#market-suggestions", OptionList).options
            await pilot.press("escape")
            await pilot.pause()
            assert app.screen is modal
            assert not modal.query_one("#market-suggestions", OptionList).options
            await pilot.press("escape")
            await pilot.pause()
            assert app.screen is not modal

    asyncio.run(run())


def test_market_edit_mode_has_no_suggestions():
    async def run():
        app = App()
        async with app.run_test() as pilot:
            modal = MarketSetupModal(
                {"id": "lse", "label": "London", "currency": "GBP", "yahoo_suffix": ".L"},
                editable_id=False,
            )
            app.push_screen(modal)
            await pilot.pause()
            assert not modal.query("#market-suggestions")
            assert modal.query_one("#market-id", Input).disabled

    asyncio.run(run())
