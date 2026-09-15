"""Thesis health: deterministic states over accepted evidence."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

from sqlmodel import Session

from rigger.core.db import NewsItemTable
from rigger.evidence import EvidenceItem
from rigger.theses import Thesis, add_evidence, create_thesis
from rigger.thesis_health import badge_text, compute_health, state_style
from rigger.tui.screens.theses import Theses
from tests.conftest import FakeConfig


class FakeRig:
    def __init__(self, engine):
        self.engine = engine
        self.llm = None
        self.cfg = FakeConfig()


NOW = datetime(2026, 9, 16, 12, 0, tzinfo=UTC)


def _thesis(**extra) -> Thesis:
    fields = {
        "id": "t1",
        "claim": "Solar grows for a decade",
        "created_at": NOW - timedelta(days=30),
    }
    fields.update(extra)
    return Thesis(**fields)


def _linked(
    id: str,
    days: float,
    side: str = "support",
    *,
    kind: str = "news",
    title: str = "Something happened",
) -> tuple[EvidenceItem, str]:
    item = EvidenceItem(
        id=id,
        target_ids=("US:AAPL",),
        ts=NOW - timedelta(days=days),
        kind=kind,  # type: ignore[arg-type]
        title=title,
        body=None,
        source="test",
        url=None,
        sentiment=None,
    )
    return item, side  # type: ignore[arg-type]


def test_every_state_reachable():
    cases = {
        "emerging": [_linked("a", 1)],
        "building": [_linked(f"s{i}", 1) for i in range(4)],
        "mixed": [_linked("s1", 1), _linked("s2", 2)]
        + [_linked("a1", 1, "against"), _linked("a2", 2, "against")],
        "weakening": [_linked("s1", 1)] + [_linked(f"a{i}", i, "against") for i in range(1, 4)],
        "challenged": [_linked(f"s{i}", 1, title="Guidance cut") for i in range(4)],
        "idle": [_linked(f"s{i}", 30) for i in range(4)],
    }
    cases["challenged"][0][0]  # keep lints honest about tuple access
    thesis = _thesis(falsifiers=["guidance cut"])
    for expected, linked in cases.items():
        result = compute_health(thesis, linked, now=NOW)
        assert result.state == expected, (expected, result)


def test_falsifier_beats_tilt_but_not_coverage():
    thesis = _thesis(falsifiers=["policy reversal"])
    building = [_linked(f"s{i}", 1) for i in range(4)]
    assert compute_health(thesis, building, now=NOW).state == "building"
    hit = [_linked("s0", 1, title="Policy reversal hits demand")] + building[1:]
    assert compute_health(thesis, hit, now=NOW).state == "challenged"
    sparse = [hit[0]]
    assert compute_health(thesis, sparse, now=NOW).state == "emerging"


def test_one_fresh_item_prevents_idle():
    thesis = _thesis()
    linked = [_linked("s0", 1)] + [_linked(f"s{i}", 30) for i in range(1, 4)]
    result = compute_health(thesis, linked, now=NOW)
    assert result.state == "building"
    stale = [_linked(f"s{i}", 30) for i in range(4)]
    assert compute_health(thesis, stale, now=NOW).state == "idle"


def test_recency_weighting_counts_old_items_half():
    thesis = _thesis()
    linked = [_linked("s1", 1), _linked("a1", 30, "against"), _linked("a2", 40, "against")]
    result = compute_health(thesis, linked, now=NOW)
    assert result.tilt == 0.0
    assert result.state == "mixed"
    one_old = [_linked("s1", 1), _linked("a1", 30, "against")]
    assert compute_health(thesis, one_old, now=NOW).tilt > 0.25


def test_drivers_capped_and_ordered():
    thesis = _thesis(falsifiers=["guidance cut"])
    linked = (
        [_linked("hit", 1, title="Guidance cut")]  # type: ignore[list-item]
        + [_linked(f"s{i}", i) for i in range(1, 7)]
    )
    result = compute_health(thesis, linked, now=NOW)
    assert len(result.drivers) == 5
    assert result.drivers[0] == "hit"


def test_badge_text_and_styles():
    thesis = _thesis()
    result = compute_health(thesis, [_linked(f"s{i}", 1) for i in range(4)], now=NOW)
    assert "building" in badge_text(result)
    assert "4 for / 0 against" in badge_text(result)
    assert state_style("challenged") == "red"
    assert state_style("building") == "green"


def test_screen_renders_health_badge(tmp_engine):
    thesis = create_thesis(tmp_engine, "Solar grows for a decade", targets=("US:AAPL",))
    now = datetime.now(UTC)
    with Session(tmp_engine) as session:
        for i in range(4):
            session.add(
                NewsItemTable(
                    id=f"n{i}",
                    instrument_ids='["US:AAPL"]',
                    published=now - timedelta(days=1),
                    title=f"Supporting item {i}",
                    url=f"https://example.com/{i}",
                    source="rss",
                )
            )
        session.commit()
    for i in range(4):
        add_evidence(tmp_engine, thesis.id, f"news:n{i}", "support", note="supports", accepted=True)

    async def run():
        from textual.app import App

        app = App()
        async with app.run_test() as pilot:
            screen = Theses(FakeRig(tmp_engine))
            await app.push_screen(screen)
            screen.selected = thesis.id
            await screen.render_detail()
            await pilot.pause()
            badge = str(screen.query_one("#thesis-health").render())
            assert "building" in badge
            assert "4 for / 0 against" in badge

    asyncio.run(run())
