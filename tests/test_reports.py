"""Tests for report building, rendering, persistence, and the Reports screen."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime
from types import SimpleNamespace

import pytest
from sqlmodel import Session
from textual.app import App
from textual.widgets import DataTable, MarkdownViewer

from rigger.core.db import EventTable, NewsItemTable
from rigger.core.json import to_json
from rigger.core.models import Instrument
from rigger.evidence import cite
from rigger.reports import build_report, gather, render_markdown, write_report
from rigger.tui.screens.reports import Reports
from tests.conftest import FakeConfig, FakeLLM, seed_bars

INST = "US:AAPL"
START = datetime(2026, 3, 18, tzinfo=UTC)


class FakeRig:
    def __init__(self, engine, llm, universe=None):
        self.engine = engine
        self.llm = llm
        self.cfg = FakeConfig({"report": "test/model"})
        self._universe = universe or []

    def universe(self):
        return self._universe


class ScreenRig(FakeRig):
    def __init__(self, engine, llm, universe, reports_dir):
        super().__init__(engine, llm, universe)
        self.cfg = SimpleNamespace(llm_routing={"report": "test/model"}, reports_dir=reports_dir)


def _seed(engine) -> None:
    seed_bars(engine, INST, n=5, start=START)
    with Session(engine) as session:
        session.add(
            NewsItemTable(
                id="news-1",
                instrument_ids=to_json([INST]),
                published=datetime(2026, 3, 20, 12, 0, tzinfo=UTC),
                title="Apple and Microsoft sign cloud deal",
                url="https://example.com/news-1",
                body="Both companies announced a partnership.",
                source="rss",
            )
        )
        session.add(
            EventTable(
                id="event-1",
                instrument_id=INST,
                ts=datetime(2026, 3, 19, 12, 0, tzinfo=UTC),
                kind="earnings",
                summary="Reported EPS above consensus",
                sentiment=0.4,
                evidence_ids=to_json(["news-1"]),
                extracted_by="test/model",
                prompt_version="extract_v1",
            )
        )
        session.commit()


def _claim(text: str, ids: list[str]) -> dict:
    return {"text": text, "evidence_ids": ids}


def _draft() -> dict:
    return {
        "summary": "Apple signed a cloud partnership and closed the window at 102.00.",
        "bull": [
            _claim("Apple and Microsoft announced a partnership.", ["news:news-1"]),
            _claim("Partnership revenue is already accretive.", ["news:ghost"]),
            _claim("No sources given.", []),
        ],
        "bear": [_claim("The close rose through the window.", ["bar:5", "bar:4"])],
        "risks": [],
        "catalysts": [_claim("Earnings were reported above consensus.", ["event:event-1"])],
        "unknowns": ["What guidance did management give for next quarter?"],
        "sentiment": 0.6,
        "sentiment_reasons": [_claim("EPS came in above consensus.", ["event:event-1"])],
    }


def test_build_report_returns_cited_report(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    _seed(tmp_engine)
    llm = FakeLLM({"report": _draft()})

    report = asyncio.run(build_report(FakeRig(tmp_engine, llm), INST))

    assert report.target_id == INST
    assert report.prompt_version == "report_v1"
    assert report.sentiment == 0.6
    assert report.summary == "Apple signed a cloud partnership and closed the window at 102.00."
    assert report.unknowns == ["What guidance did management give for next quarter?"]

    gathered = {item.id: item for item in gather(INST, tmp_engine)}
    for claims in (
        report.bull,
        report.bear,
        report.risks,
        report.catalysts,
        report.sentiment_reasons,
    ):
        for claim in claims:
            assert claim.evidence_ids
            assert set(claim.evidence_ids) <= set(gathered)
    assert report.citations == {item.id: cite(item) for item in gathered.values()}

    call = llm.calls[0]
    assert call["task"] == "report"
    assert call["model"] == "test/model"
    assert call["prompt_version"] == "report_v1"
    assert "Use only the information provided." in call["prompt"]
    assert "Do not rely on prior knowledge of prices, news or events." in call["prompt"]
    assert "Do not recommend buying, selling or holding." in call["prompt"]
    assert "news:news-1" in call["prompt"]
    assert cite(gathered["news:news-1"]) in call["prompt"]
    assert "Both companies announced a partnership." in call["prompt"]


def test_claims_citing_unknown_or_no_evidence_are_dropped(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    _seed(tmp_engine)
    llm = FakeLLM({"report": _draft()})

    report = asyncio.run(build_report(FakeRig(tmp_engine, llm), INST))

    assert [claim.text for claim in report.bull] == ["Apple and Microsoft announced a partnership."]
    assert "Partnership revenue is already accretive." not in {claim.text for claim in report.bull}


def test_draft_with_zero_substantive_claims_raises(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    _seed(tmp_engine)
    draft = _draft()
    for field in ("bull", "bear", "risks", "catalysts", "sentiment_reasons"):
        draft[field] = [_claim("Hallucinated.", ["news:ghost"])]
    llm = FakeLLM({"report": draft})

    with pytest.raises(ValueError, match="no claims supported by the gathered evidence"):
        asyncio.run(build_report(FakeRig(tmp_engine, llm), INST))


def test_no_evidence_raises_before_any_call(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    llm = FakeLLM({"report": _draft()})

    with pytest.raises(ValueError, match="no evidence gathered"):
        asyncio.run(build_report(FakeRig(tmp_engine, llm), INST))
    assert llm.calls == []


def test_render_markdown_cites_claims_and_sentiment(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    _seed(tmp_engine)
    llm = FakeLLM({"report": _draft()})

    report = asyncio.run(build_report(FakeRig(tmp_engine, llm), INST))
    markdown = render_markdown(report)

    assert f"sentiment {report.sentiment:.2f}" in markdown
    assert report.citations["news:news-1"] in markdown
    assert report.citations["event:event-1"] in markdown
    assert "## Summary" in markdown
    assert "## Bull case" in markdown
    assert "## Unknowns" in markdown
    assert "Partnership revenue is already accretive." not in markdown


def test_write_report_writes_target_dir_dated_file(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    _seed(tmp_engine)
    llm = FakeLLM({"report": _draft()})

    report = asyncio.run(build_report(FakeRig(tmp_engine, llm), INST))
    path = write_report(report, tmp_path)

    assert path == tmp_path / INST / f"{report.as_of:%Y-%m-%d}.md"
    assert path.exists()
    assert report.citations["news:news-1"] in path.read_text(encoding="utf-8")


def test_reports_screen_lists_targets_and_generates(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[targets.apple]\nkind = "company"\nmarket = "us"\ntickers = ["AAPL"]\n',
        encoding="utf-8",
    )
    _seed(tmp_engine)
    reports_dir = tmp_path / "reports"
    aapl = Instrument(id=INST, market="us", symbol="AAPL", currency="USD", watchlists=("apple",))
    rig = ScreenRig(tmp_engine, FakeLLM({"report": _draft()}), [aapl], str(reports_dir))

    class ReportsApp(App):
        def on_mount(self) -> None:
            self.push_screen(Reports(rig))

    async def run():
        app = ReportsApp()
        async with app.run_test() as pilot:
            table = app.screen.query_one("#report-targets", DataTable)
            assert table.row_count == 1
            assert table.get_row_at(0)[0] == "apple"

            await pilot.click("#report-generate")
            await pilot.pause()

            written = list((reports_dir / INST).glob("*.md"))
            assert len(written) == 1
            assert report_cite_line in written[0].read_text(encoding="utf-8")

    report_cite_line = "[rss] Apple and Microsoft sign cloud deal <https://example.com/news-1>"
    asyncio.run(run())


def test_reports_screen_shows_newest_report_across_instruments(tmp_engine, tmp_path, monkeypatch):
    """Paths sort by filename: a plain path sort would show the last ticker's report."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[targets.pair]\nkind = "theme"\nmarket = "us"\ntickers = ["AAPL", "MSFT"]\n',
        encoding="utf-8",
    )
    reports_dir = tmp_path / "reports"
    other = "US:MSFT"
    for instrument, day, body in ((INST, "2026-03-20", "newest"), (other, "2026-03-19", "older")):
        path = reports_dir / instrument / f"{day}.md"
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(body, encoding="utf-8")
    universe = [
        Instrument(id=INST, market="us", symbol="AAPL", currency="USD", watchlists=("pair",)),
        Instrument(id=other, market="us", symbol="MSFT", currency="USD", watchlists=("pair",)),
    ]
    rig = ScreenRig(tmp_engine, FakeLLM({}), universe, str(reports_dir))

    class ReportsApp(App):
        def on_mount(self) -> None:
            self.push_screen(Reports(rig))

    async def run():
        app = ReportsApp()
        async with app.run_test() as pilot:
            await app.screen.show_latest("pair")
            await pilot.pause()
            viewer = app.screen.query_one("#report-view", MarkdownViewer)
            assert "newest" in viewer.document.source

    asyncio.run(run())


def test_reports_screen_generate_with_no_targets_notifies(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    rig = ScreenRig(tmp_engine, FakeLLM({}), [], str(tmp_path / "reports"))

    class ReportsApp(App):
        def on_mount(self) -> None:
            self.push_screen(Reports(rig))

    async def run():
        app = ReportsApp()
        async with app.run_test() as pilot:
            assert app.screen.query_one("#report-targets", DataTable).row_count == 0
            await pilot.click("#report-generate")
            await pilot.pause()

    asyncio.run(run())
