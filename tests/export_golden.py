"""Golden-screen exporter: the Rust rewrite oracle (docs/RUST_REWRITE_PLAN.md, Rule 1).

Boots the app over the same pinned environment as ``tests/test_snapshots.py``
(frozen clock, offline quote feed, seeded temp DB, relative paths), drives the
Watchlist inspector into its key states, and dumps each screen as JSON:
``rows × cells`` of ``{ch, fg, bg, attrs}`` with **resolved RGB** (theme token
blending happens before we read the styles, so the exported colors are the
colors a terminal would show).

Capture path: Textual 8's compositor — ``app.screen._compositor.render_strips()``
returns one :class:`~textual.strip.Strip` per row whose segments carry the
already-resolved ``rich.Style``. Nothing is monkeypatched beyond the offline
pins mirrored from ``snapshot_app``.

Scenario note: the plan's ``scrubbed`` state assumes the K9 chart scrub from
docs/UI_UX_AUDIT.md, which is **not implemented** in the Python app (no scrub
key exists in ``delta/tui/screens/targets.py``). The closest implemented
inspector state — the glossary modal (``i``) — is exported instead, under the
name ``glossary``, Tier B (prose). When K9 lands, add the scrub state here.

Usage::

    uv run python tests/export_golden.py --out fixtures/golden_screens
    uv run python tests/export_golden.py --state default --size 120x40 --out /tmp/g1
"""

from __future__ import annotations

import argparse
import asyncio
import json
import math
import sys
from dataclasses import dataclass
from datetime import UTC, datetime, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

import pytest

sys.path.insert(0, str(Path(__file__).parent))  # conftest / test_tui imports

from conftest import seed_bars  # noqa: E402
from test_tui import AAPL, FakeRig  # noqa: E402

from delta.asset_metrics import AssetMetrics  # noqa: E402
from delta.core.db import init_engine  # noqa: E402
from delta.tui.app import DeltaApp  # noqa: E402

NOW = datetime(2026, 9, 21, 9, 30, tzinfo=ZoneInfo("UTC"))
BAR_START = datetime(2026, 7, 3, tzinfo=UTC)
BAR_COUNT = 80

#: rich.Style boolean attributes recorded per cell (sorted in the output).
_STYLE_ATTRS = (
    "blink",
    "blink2",
    "bold",
    "conceal",
    "dim",
    "encircle",
    "frame",
    "italic",
    "overline",
    "reverse",
    "strike",
    "underline",
    "underline2",
)


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


@dataclass(frozen=True)
class Scenario:
    name: str
    size: tuple[int, int]
    tier: str  # "A" (status bar, chart, tables) or "B" (prose)
    keys: tuple[str, ...]

    @property
    def size_label(self) -> str:
        return f"{self.size[0]}x{self.size[1]}"

    @property
    def filename(self) -> str:
        return f"{self.name}-{self.size_label}.json"


#: Keys reach the Watchlist screen ("2", "enter") and then the state's own keys.
SCENARIOS = (
    Scenario("home", (120, 40), "A", ()),
    Scenario("default", (120, 40), "A", ("2", "enter")),
    Scenario("range-cycled", (120, 40), "A", ("2", "enter", "r")),
    Scenario("glossary", (120, 40), "B", ("2", "enter", "i")),
    Scenario("narrow", (80, 24), "A", ("2", "enter")),
)


def _hex(color) -> str | None:
    """Resolved RGB for a rich Color, or None when the style inherits."""
    if color is None:
        return None
    triplet = color.triplet
    return f"#{triplet.red:02x}{triplet.green:02x}{triplet.blue:02x}"


def _capture_screen(app: DeltaApp) -> list[list[dict]]:
    """The visible screen as rows × cells of {ch, fg, bg, attrs}."""
    strips = app.screen._compositor.render_strips()
    rows: list[list[dict]] = []
    for strip in strips:
        row: list[dict] = []
        for segment in strip._segments:
            if segment.is_control:
                continue
            style = segment.style
            attrs = sorted(a for a in _STYLE_ATTRS if getattr(style, a, None))
            fg, bg = _hex(style.color), _hex(style.bgcolor)
            for ch in segment.text:
                row.append({"ch": ch, "fg": fg, "bg": bg, "attrs": attrs})
        rows.append(row)
    return rows


def export_scenario(scenario: Scenario, tmp_path: Path, out_dir: Path) -> Path:
    import time_machine
    import tomli_w

    from delta import tui
    from delta.quotes import YahooQuotes

    async def offline(self):
        self.on_state("offline test")
        await asyncio.Event().wait()

    with pytest.MonkeyPatch.context() as mp:
        mp.setattr(YahooQuotes, "run", offline)
        mp.setattr(tui.screens.targets, "fetch_asset_metrics", lambda *a, **k: _metric())
        mp.setattr(
            "delta.tui.screens.config.read_env_value", lambda name: "test-key" if name else ""
        )
        mp.setattr("delta.tui.screens.config._human_size", lambda size: "4 KB")
        tmp_path.joinpath("config.toml").write_text(
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
        import os

        old_cwd = Path.cwd()
        os.chdir(tmp_path)
        try:
            with time_machine.travel(NOW, tick=False):
                app = DeltaApp(rig)

                async def run() -> list[list[dict]]:
                    async with app.run_test(size=scenario.size) as pilot:
                        await pilot.press(*scenario.keys)
                        # Workers (metrics fetch, refreshes) land over a few
                        # frames; mirror test_snapshots.py's run_before.
                        await pilot.pause(0.3)
                        return _capture_screen(app)

                rows = asyncio.run(run())
        finally:
            os.chdir(old_cwd)

    payload = {
        "state": scenario.name,
        "size": [scenario.size[0], scenario.size[1]],
        "tier": scenario.tier,
        "rows": rows,
    }
    out_dir.mkdir(parents=True, exist_ok=True)
    path = out_dir / scenario.filename
    path.write_text(
        json.dumps(payload, ensure_ascii=False, sort_keys=True) + "\n", encoding="utf-8"
    )
    return path


def export_all(out_dir: Path, state: str | None = None, size: str | None = None) -> None:
    import tempfile

    selected = [
        s
        for s in SCENARIOS
        if (state is None or s.name == state) and (size is None or s.size_label == size)
    ]
    if not selected:
        raise SystemExit(f"no scenario matches state={state!r} size={size!r}")
    files = []
    for scenario in selected:
        # Fresh seeded DB per scenario: the bars table has a UNIQUE(instrument, ts).
        td = Path(tempfile.mkdtemp())
        files.append(export_scenario(scenario, td, out_dir))
    manifest = {
        "scenarios": [
            {"state": s.name, "size": s.size_label, "tier": s.tier, "file": s.filename}
            for s in selected
        ]
    }
    out_dir.mkdir(parents=True, exist_ok=True)
    (out_dir / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, sort_keys=True, indent=2) + "\n", encoding="utf-8"
    )
    for f in files:
        print(f"wrote {f}")


def test_golden_export_is_deterministic(tmp_path):
    """Exporting the same scenarios twice yields byte-identical JSON."""
    export_all(tmp_path / "a")
    export_all(tmp_path / "b")
    a = sorted(p.name for p in (tmp_path / "a").iterdir())
    b = sorted(p.name for p in (tmp_path / "b").iterdir())
    assert a == b and a
    for name in a:
        assert (tmp_path / "a" / name).read_bytes() == (tmp_path / "b" / name).read_bytes(), name


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--state", help="scenario name (default: all)")
    parser.add_argument("--size", help="WxH filter (default: all)")
    parser.add_argument("--out", default="fixtures/golden_screens", help="output directory")
    args = parser.parse_args()
    export_all(Path(args.out), state=args.state, size=args.size)


if __name__ == "__main__":
    main()
