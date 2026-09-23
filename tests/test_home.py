"""Home screen: truthful hints, the agenda, first-run setup and change glyphs.

Offline like every TUI test: a seeded ``tmp_path`` engine, a canned quote feed
and no network. These assert on the rendered widget tree (and, where it
matters, the exported frame), not on snapshots.
"""

from __future__ import annotations

import asyncio
from datetime import UTC, date, datetime, timedelta

import pytest
import time_machine
import tomli_w
from sqlmodel import Session, select
from test_tui import AAPL, FakeRig

from delta import decisions
from delta.core.db import BarTable, EventTable
from delta.quotes import Quote
from delta.tui.app import DeltaApp
from delta.tui.screens.home import HomeLink
from tests.conftest import seed_bars

NOW = datetime(2026, 9, 21, 9, 30, tzinfo=UTC)


@pytest.fixture
def offline_quotes(monkeypatch):
    from delta.quotes import YahooQuotes

    async def offline(self):
        self.on_state("offline test")
        await asyncio.Event().wait()

    monkeypatch.setattr(YahooQuotes, "run", offline)


@pytest.fixture
def home_app(tmp_path, monkeypatch, tmp_engine):
    """A factory that builds a DeltaApp on a temp engine with one watched ticker."""

    def build(*, targets: dict) -> tuple[DeltaApp, FakeRig]:
        monkeypatch.chdir(tmp_path)
        (tmp_path / "config.toml").write_text(
            tomli_w.dumps({"targets": targets}),
            encoding="utf-8",
        )
        engine = tmp_engine
        seed_bars(engine, AAPL.id, n=80)
        rig = FakeRig(engine, [AAPL])
        rig.cfg.db_path = str(tmp_path / "delta.db")
        rig.cfg.reports_dir = str(tmp_path / "reports")
        return DeltaApp(rig), rig

    return build


def _quote(price: float = 1234.5, change_pct: float = 2.5) -> Quote:
    now = datetime.now(UTC)
    return Quote(
        price=price,
        currency="USD",
        change_pct=change_pct,
        timestamp=now,
        received_at=now,
    )


async def _frame_text(app: DeltaApp) -> str:
    return app.export_screenshot().replace("&#160;", " ")


def test_hints_name_the_right_keys(home_app, offline_quotes):
    """A1: the on-screen hints point at the keys that actually do the thing."""
    app, _rig = home_app(targets={"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}})

    async def run():
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.pause()
            home = app.screen
            assert "press 3, then U" in str(home.query_one("#upcoming-empty").render())
            assert "n tracks a claim" in str(home.query_one("#theses-empty").render())
            assert "3 evidence" in str(home.query_one("#since-review").render())
            # The empty-watchlist note (now superseded by the first-run setup)
            # still spells the right key.
            await home._refresh_watchlist([], {})
            assert "2 builds the watchlist" in str(home.query_one("#watch-note").render())

    asyncio.run(run())


def test_agenda_lines_and_jump_keys(home_app, offline_quotes, tmp_engine):
    """J1: the agenda shows reviews due, falsifier hits, earnings and stale sources."""
    app, rig = home_app(
        targets={"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}
    )
    engine = rig.engine
    decisions.create_decision(
        engine,
        instrument_id=AAPL.id,
        rationale="buy the dip",
        valuation_context="cheap",
        time_horizon="1y",
        review_date=date(2026, 9, 21),
        invalidation_criteria="none",
    )
    with Session(engine) as session:
        session.add(
            EventTable(
                id="earn-1",
                instrument_id=AAPL.id,
                ts=NOW + timedelta(days=3),
                kind="earnings",
                summary="Q3 earnings",
                sentiment=0.0,
                evidence_ids="[]",
                extracted_by="test",
                prompt_version="v1",
            )
        )
        # Replace the fresh bars with old ones so the source reads stale.
        for row in session.exec(select(BarTable)).all():
            session.delete(row)
        session.commit()
    seed_bars(engine, AAPL.id, n=5, start=datetime.now(UTC) - timedelta(days=60))

    async def run():
        async with app.run_test(size=(120, 40)) as pilot:
            with time_machine.travel(NOW, tick=False):
                await pilot.pause()
                home = app.screen
                reviews = home.query_one("#agenda-reviews", HomeLink)
                earnings = home.query_one("#agenda-earnings", HomeLink)
                stale = home.query_one("#agenda-stale", HomeLink)
                falsifier = home.query_one("#agenda-falsifier", HomeLink)

                assert "1 decision review due" in str(reviews.query_one(".home-link-label").render())
                assert reviews.key == "6"
                assert "1 earnings" in str(earnings.query_one(".home-link-label").render())
                assert earnings.key == "3"
                assert "stale" in str(stale.query_one(".home-link-label").render())
                assert stale.key == "2"
                assert "no falsifier hits" in str(falsifier.query_one(".home-link-label").render())
                assert falsifier.key == "4"

    asyncio.run(run())


def test_first_run_shows_setup_checklist(home_app, offline_quotes):
    """J3: no targets turns the whole grid into the setup checklist."""
    app, _rig = home_app(targets={})

    async def run():
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.pause()
            home = app.screen
            assert not home.query_one("#home-setup").has_class("-hidden")
            assert not home.query_one("#home-top").display
            assert not home.query_one("#home-bottom").display
            rows = [r for r in home.query(HomeLink) if r.id and r.id.startswith("setup-")]
            assert len(rows) == 5
            assert any(r.key == "p" for r in rows)
            assert any(r.key == "c" for r in rows)
            assert any(r.key == "2" for r in rows)

    asyncio.run(run())


def test_watchlist_change_has_direction_glyph(home_app, offline_quotes):
    """J10: green/red change is paired with ▲/▼, not colour alone."""
    app, _rig = home_app(targets={"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}})

    async def run():
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.pause()
            home = app.screen
            home.feed.quotes[AAPL.id] = _quote(change_pct=2.5)
            home._paint_quotes()
            await pilot.pause()
            frame = await _frame_text(app)
            assert "▲ +2.50%" in frame

            home.feed.quotes[AAPL.id] = _quote(change_pct=-1.1)
            home._paint_quotes()
            await pilot.pause()
            frame = await _frame_text(app)
            assert "▼ -1.10%" in frame

    asyncio.run(run())


def test_refresh_is_a_thread_worker(home_app, offline_quotes):
    """B4: Home's reload runs as a worker and still paints the desk."""
    app, _rig = home_app(targets={"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}})

    async def run():
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.pause()
            home = app.screen
            worker = home._reload()
            assert worker.name == "_reload"
            assert worker._run_threaded
            await worker.wait()
            assert home.query_one("#watch-pane")

    asyncio.run(run())
