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
from datetime import UTC, date, datetime, timedelta
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
_STATES = (
    ("home", (), "A"),
    ("default", ("2", "enter"), "A"),
    ("range-cycled", ("2", "enter", "r"), "A"),
    ("glossary", ("2", "enter", "i"), "B"),
    ("research", ("3",), "A"),
    ("theses", ("4",), "A"),
    ("ask", ("5",), "A"),
    ("decisions", ("6",), "A"),
    ("settings", ("c",), "A"),
    ("live-theses", ("4",), "A"),
    ("live-decisions", ("6",), "A"),
    ("live-ask", ("5",), "A"),
    ("live-research", ("3", "r"), "A"),
    ("live-settings", ("c",), "A"),
)

#: The full three-size matrix (R4); `narrow` stays the 80x24 detail-open state.
SCENARIOS = tuple(
    Scenario(name, size, tier, keys)
    for name, keys, tier in _STATES
    for size in ((80, 24), (120, 40), (200, 50))
) + (Scenario("narrow", (80, 24), "A", ("2", "enter")),)


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


def _seed_live(rig: FakeRig, tmp_path: Path, mp: pytest.MonkeyPatch) -> dict:
    """Canonical domain operations; returned metadata recreates the seed in Rust."""
    from uuid import UUID

    from delta.chat import ChatMessage
    from delta.core.config import build_config
    from delta.core.db import store_items
    from delta.core.models import Event, NewsItem
    from delta.decisions import append_review, create_decision
    from delta.evidence import cite, evidence_by_ids
    from delta.plugins.data.sec_edgar import SECEdgar
    from delta.reports import Claim, Report, write_report
    from delta.theses import add_evidence, create_thesis

    config = {
        "base_currency": "AUD",
        "db_path": "delta.db",
        "reports_dir": "reports",
        "targets": {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}},
        "llm": {"provider": "openrouter", "model": "test-model"},
        "plugins": {"sec_edgar": {"enabled": True, "contact": "oracle@example.test"}},
        "markets": {
            "us": {"label": "United States", "currency": "USD", "yahoo_suffix": ""},
            "asx": {
                "label": "Australian Securities Exchange",
                "currency": "AUD",
                "yahoo_suffix": ".AX",
            },
        },
    }
    import tomli_w

    tmp_path.joinpath("config.toml").write_text(tomli_w.dumps(config), encoding="utf-8")
    rig.cfg = build_config(config)
    rig._universe = [AAPL.model_copy(update={"watchlists": ("apple",)})]
    rig.plugins = {"sec_edgar": SECEdgar()}
    news = NewsItem(
        id="oracle-news",
        instrument_ids=[AAPL.id],
        published=NOW - timedelta(days=1),
        title="Apple reports durable services demand",
        url="https://example.test/apple-demand",
        body="Services revenue grew while customer retention remained stable.",
        source="rss",
    )
    event = Event(
        id="oracle-event",
        instrument_id=AAPL.id,
        ts=NOW - timedelta(hours=2),
        kind="guidance",
        summary="Management expects services growth to continue",
        sentiment=0.4,
        evidence_ids=["news:oracle-news"],
        extracted_by="test-model",
        prompt_version="extract_v1",
    )
    store_items(rig.engine, [news, event])
    thesis = create_thesis(
        rig.engine,
        "Services demand remains resilient",
        scope="US technology",
        assumptions=["Customer retention stays stable"],
        falsifiers=["Services revenue declines"],
        targets=[AAPL.id],
        time_horizon="12 months",
    )
    links = [
        add_evidence(
            rig.engine,
            thesis.id,
            "news:oracle-news",
            "support",
            "Published report supports recurring demand",
            accepted=True,
        ),
        add_evidence(
            rig.engine,
            thesis.id,
            "event:oracle-event",
            "neutral",
            "Guidance needs confirmation",
            accepted=False,
        ),
    ]
    mp.setattr("delta.decisions.uuid4", lambda: UUID("0123456789abcdef0123456789abcdef"))
    decision = create_decision(
        rig.engine,
        AAPL.id,
        "Track recurring demand before changing exposure",
        "P/E 31.2",
        "12 months",
        date(2026, 10, 21),
        "Services revenue declines",
        thesis_id=thesis.id,
        created_at=NOW - timedelta(days=2),
    )
    review = append_review(
        rig.engine,
        decision.id,
        "Demand remains stable; retain original framing",
        status="open",
        created_at=NOW - timedelta(days=1),
    )
    items = evidence_by_ids(rig.engine, ["news:oracle-news", "event:oracle-event"])
    report = Report(
        target_id=AAPL.id,
        as_of=NOW,
        prompt_version="report_v2",
        summary="Stored reports support resilient services demand.",
        sentiment=0.4,
        bull=[
            Claim(
                text="Services revenue grew with stable retention.",
                evidence_ids=["news:oracle-news"],
            )
        ],
        risks=[
            Claim(
                text="Forward guidance still needs confirmation.",
                evidence_ids=["event:oracle-event"],
            )
        ],
        unknowns=["Whether growth persists over the next year"],
        citations={item.id: cite(item) for item in items},
    )
    write_report(report, tmp_path / "reports")
    messages = [
        ChatMessage(role="user", text="What supports the services thesis?", source="user"),
        ChatMessage(
            role="assistant",
            text="Services revenue grew with stable retention.\n\nGuidance needs confirmation.",
            citations=("news:oracle-news", "event:oracle-event"),
            source="stored",
        ),
    ]
    return {
        "version": 1,
        "now": NOW.isoformat(),
        "bars": {
            "instrument_id": AAPL.id,
            "start": BAR_START.isoformat(),
            "count": BAR_COUNT,
            "price_formula": "round(200 + i * 0.4 + 6 * sin(i / 4), 2)",
            "source": "test",
            "closes": [_price(i) for i in range(BAR_COUNT)],
            "open_equals_close": True,
            "high_offset": 1.0,
            "low_offset": -1.0,
            "volume": 1000.0,
        },
        "config": config,
        "instrument": rig.universe()[0].model_dump(mode="json"),
        "news": [news.model_dump(mode="json")],
        "events": [event.model_dump(mode="json")],
        "theses": [thesis.model_dump(mode="json")],
        "thesis_evidence": [link.model_dump(mode="json") for link in links],
        "decisions": [decision.model_dump(mode="json")],
        "decision_reviews": [review.model_dump(mode="json")],
        "report": report.model_dump(mode="json"),
        "chat": [message.model_dump(mode="json") for message in messages],
        "chat_meta": {
            "0": {"at": NOW.isoformat()},
            "1": {"at": NOW.isoformat(), "seconds": 2.0, "cost": 0.0},
        },
        "selected_company": AAPL.id,
        "selected_target": "apple",
        "provider_connected": True,
        "database_size": "4 KB",
    }


def export_scenario(scenario: Scenario, tmp_path: Path, out_dir: Path) -> Path:
    import time_machine
    import tomli_w

    from delta import tui
    from delta.quotes import YahooQuotes

    async def offline(self):
        self.on_state("offline test")
        await asyncio.Event().wait()

    seed = None
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
                if scenario.name.startswith("live-"):
                    seed = _seed_live(rig, tmp_path, mp)
                app = DeltaApp(rig)

                async def run() -> list[list[dict]]:
                    async with app.run_test(size=scenario.size) as pilot:
                        await pilot.press(*scenario.keys)
                        # Workers (metrics fetch, refreshes) land over a few
                        # frames; mirror test_snapshots.py's run_before.
                        await pilot.pause(0.3)
                        if scenario.name == "live-ask":
                            from delta.chat import ChatMessage
                            from delta.tui.screens.chat import TurnMeta

                            screen = app.screen
                            screen.history = [
                                ChatMessage.model_validate(message) for message in seed["chat"]
                            ]
                            screen.meta = {
                                0: TurnMeta(at=NOW),
                                1: TurnMeta(at=NOW, seconds=2.0, cost=0.0),
                            }
                            screen.selected_answer = 1
                            screen.selected_citation = 0
                            await screen._render_transcript()
                            await pilot.pause(0.1)
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
    if seed is not None:
        payload["seed"] = seed
        payload["ui"] = {
            "keys": list(scenario.keys),
            "focus": {
                "live-theses": "claims",
                "live-decisions": "list",
                "live-ask": "transcript",
                "live-research": "report",
                "live-settings": "model",
            }[scenario.name],
            "selected_index": 0,
            "chat_selected_answer": 1,
            "chat_selected_citation": 0,
            "research_kind": "all",
            "research_limit": 200,
            "research_prices_folded": True,
            "diagnostics_open": scenario.size[0] >= 100,
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
        if (state is None or s.name == state or (state == "live" and s.name.startswith("live-")))
        and (size is None or s.size_label == size)
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
    manifest_name = (
        "live-manifest.json"
        if all(s.name.startswith("live-") for s in selected)
        else "manifest.json"
    )
    (out_dir / manifest_name).write_text(
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


def test_live_golden_seed_and_capture_are_deterministic(tmp_path):
    """The populated oracle must be reproducible, including generated journal IDs."""
    export_all(tmp_path / "a", state="live-theses", size="120x40")
    export_all(tmp_path / "b", state="live-theses", size="120x40")
    name = "live-theses-120x40.json"
    first = (tmp_path / "a" / name).read_bytes()
    assert first == (tmp_path / "b" / name).read_bytes()
    data = json.loads(first)
    assert data["seed"]["decisions"][0]["id"] == "0123456789abcdef0123456789abcdef"
    assert data["seed"]["thesis_evidence"][0]["accepted"] is True
    assert data["seed"]["thesis_evidence"][1]["accepted"] is False
    assert data["seed"]["instrument"]["watchlists"] == ["apple"]
    assert data["seed"]["news"][0]["source"] == "rss"
    visible = "\n".join("".join(cell["ch"] for cell in row) for row in data["rows"])
    assert "moved by  Apple reports durable servi" in visible
    assert "+1 support" in visible
    assert len(data["rows"]) == 40
    assert all(len(row) == 120 for row in data["rows"])


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--state", help="scenario name or live for populated scenarios (default: all)"
    )
    parser.add_argument("--size", help="WxH filter (default: all)")
    parser.add_argument("--out", default="fixtures/golden_screens", help="output directory")
    args = parser.parse_args()
    export_all(Path(args.out), state=args.state, size=args.size)


if __name__ == "__main__":
    main()
