"""Layout snapshots: every panel at the three terminal sizes the TUI is designed for.

A snapshot is the rendered SVG of a screen, compared byte-for-byte with the
approved copy under ``tests/__snapshots__/``. A layout change shows up as a
visual diff in the pytest report instead of slipping through assertion tests.

After an intended layout change, review the new frames and accept them with::

    uv run pytest tests/test_snapshots.py --snapshot-update

Determinism: the clock is frozen (UTC), paths are relative to a temp cwd, bars
sit on fixed dates, and the quote feed and metrics fetch never touch the
network.
"""

from __future__ import annotations

import asyncio
import math
from datetime import UTC, datetime, timedelta
from zoneinfo import ZoneInfo

import pytest
import time_machine
import tomli_w
from conftest import seed_bars
from test_tui import AAPL, FakeRig

from delta.asset_metrics import AssetMetrics
from delta.core.db import init_engine
from delta.tui.app import DeltaApp

NOW = datetime(2026, 9, 21, 9, 30, tzinfo=ZoneInfo("UTC"))
SIZES = [(80, 24), (120, 40), (200, 50)]
BAR_START = datetime(2026, 7, 3, tzinfo=UTC)
BAR_COUNT = 80


def _price(i: int) -> float:
    return round(200.0 + i * 0.4 + 6 * math.sin(i / 4), 2)


def _metric() -> AssetMetrics:
    days = range(BAR_COUNT - 30, BAR_COUNT)
    series = [_price(i) for i in days]
    times = [(BAR_START + timedelta(days=i)).isoformat() for i in days]
    return AssetMetrics(
        AAPL.id,
        "equity",
        values={"Current price": f"{series[-1]:.2f}", "Market cap": "3.4T", "P/E": "31.2"},
        series=series,
        series_times=times,
        change_label=f"{(series[-1] / series[0] - 1) * 100:+.1f}%",
        history_start=times[0],
        history_end=times[-1],
        period_high=max(series),
        period_low=min(series),
    )


@pytest.fixture
def snapshot_app(tmp_path, monkeypatch):
    """A DeltaApp over a seeded temp DB, with time and the network pinned."""
    from delta import tui
    from delta.quotes import YahooQuotes

    async def offline(self):
        self.on_state("offline test")
        await asyncio.Event().wait()

    monkeypatch.setattr(YahooQuotes, "run", offline)
    monkeypatch.setattr(
        tui.screens.targets, "fetch_asset_metrics", lambda *args, **kwargs: _metric()
    )
    # Pin the settings panel's two environment-dependent values: the provider
    # connection status (depends on OPENROUTER_API_KEY being present) and the
    # on-disk SQLite size (differs by OS/SQLite version). Without these the
    # Config snapshot drifts between a dev machine and CI.
    monkeypatch.setattr(
        "delta.tui.screens.config.read_env_value", lambda name: "test-key" if name else ""
    )
    monkeypatch.setattr("delta.tui.screens.config._human_size", lambda size: "4 KB")
    # Relative paths only: the Config and Home panels print the DB name.
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {"targets": {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}}
        ),
        encoding="utf-8",
    )
    engine = init_engine(tmp_path / "delta.db")
    seed_bars(engine, AAPL.id, n=BAR_COUNT, start=BAR_START, price_fn=_price)
    rig = FakeRig(engine, [AAPL])
    rig.cfg.db_path = "delta.db"
    rig.cfg.reports_dir = "reports"

    with time_machine.travel(NOW, tick=False):
        yield lambda: DeltaApp(rig)


PANELS = [
    ("home", ()),
    ("watchlist", ("2", "enter")),
    ("research", ("3",)),
    ("theses", ("4",)),
    ("ask", ("5",)),
    ("decisions", ("6",)),
    ("settings", ("c",)),
]


@pytest.mark.parametrize("size", SIZES, ids=lambda s: f"{s[0]}x{s[1]}")
@pytest.mark.parametrize(("panel", "keys"), PANELS, ids=[name for name, _ in PANELS])
def test_panel_snapshot(snap_compare, snapshot_app, panel, keys, size):
    async def open_panel(pilot) -> None:
        await pilot.press(*keys)
        # Workers (metrics fetch, refreshes) land over a few frames.
        await pilot.pause(0.3)

    assert snap_compare(snapshot_app(), terminal_size=size, run_before=open_panel)
