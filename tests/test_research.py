"""Research workflows: company isolation, citation navigation and browse state."""

from __future__ import annotations

import asyncio
from datetime import UTC, datetime, timedelta

import pytest
from sqlmodel import Session
from textual.app import App
from textual.widgets import Button, Input, Markdown, MarkdownViewer, Select, Static

from rigger.core.db import NewsItemTable
from rigger.core.json import to_json
from rigger.core.models import Instrument
from rigger.evidence import evidence
from rigger.reports import Claim, Report, write_report
from rigger.tui.screens.data import Data
from rigger.tui.screens.reports import Reports
from rigger.tui.screens.research import ResearchState
from rigger.tui.theme import THEMES
from tests.conftest import FakeLLM
from tests.test_reports import INST, ScreenRig, _draft, _seed


def setup_rig(tmp_engine, tmp_path, monkeypatch):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[targets.pair]\nkind="theme"\nmarket="us"\ntickers=["AAPL", "MSFT"]\n'
    )
    _seed(tmp_engine)
    universe = [
        Instrument(
            id=f"US:{symbol}", market="us", symbol=symbol, currency="USD", watchlists=("pair",)
        )
        for symbol in ("AAPL", "MSFT")
    ]
    return ScreenRig(tmp_engine, FakeLLM({"report": _draft()}), universe, str(tmp_path / "reports"))


def saved_report(rig):
    report = Report(
        target_id=INST,
        as_of=datetime(2026, 3, 20, tzinfo=UTC),
        prompt_version="report_v1",
        summary="A sourced summary",
        sentiment=0,
        bull=[Claim(text="The companies partnered.", evidence_ids=["news:news-1"])],
        citations={"news:news-1": "Partnership filing", "bar:1": "Unused price"},
    )
    write_report(report, rig.cfg.reports_dir)
    return report


def test_search_filters_before_limit_and_matches_body(tmp_engine):
    with Session(tmp_engine) as session:
        for n in range(205):
            session.add(
                NewsItemTable(
                    id=f"n{n}",
                    instrument_ids=to_json([INST]),
                    published=datetime(2026, 1, 1, tzinfo=UTC) + timedelta(days=n),
                    title=f"Update {n}",
                    body="needle" if n == 0 else "other",
                    url=f"https://example.com/{n}",
                    source="rss",
                )
            )
        session.commit()
    assert (
        evidence(tmp_engine, target=INST, kind="news", search="NEEDLE", limit=1)[0].id == "news:n0"
    )
    assert len(evidence(tmp_engine, target=INST, search="rss", limit=201)) == 201
    assert evidence(tmp_engine, target="US:MSFT", search="needle") == []


def test_structured_report_round_trip_and_legacy(tmp_engine, tmp_path, monkeypatch):
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    report = saved_report(rig)
    path = write_report(report, rig.cfg.reports_dir)
    assert Report.model_validate_json(path.with_suffix(".json").read_text()) == report

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Reports(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            assert "(evidence:news:news-1)" in screen.query_one(MarkdownViewer).document.source
            assert "bar:1" not in screen.cited_ids()
            path.with_suffix(".json").unlink()
            await screen.show_latest(INST)
            assert screen.report is None
            assert "Regenerate" in str(screen.query_one("#report-legacy", Static).render())
            assert "A sourced summary" in screen.query_one(MarkdownViewer).document.source

    asyncio.run(run())


def test_company_filters_and_citation_round_trip(tmp_engine, tmp_path, monkeypatch):
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    saved_report(rig)

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Data(rig))

    async def run():
        async with TestApp().run_test(size=(120, 35)) as pilot:
            screen = pilot.app.screen
            screen.query_one("#evidence-search", Input).value = "close"
            await pilot.pause()
            assert screen.items and all(i.kind == "bar" for i in screen.items.values())
            opened = []
            monkeypatch.setattr(pilot.app, "open_url", opened.append)
            document = screen.query_one(MarkdownViewer).document
            document.post_message(Markdown.LinkClicked(document, "evidence:news:news-1"))
            await pilot.pause()
            assert opened == []
            assert screen.view.search == "close"
            assert "Both companies" in str(screen.query_one("#source-body", Static).render())
            assert not screen.query_one("#evidence-report", Button).disabled
            await pilot.click("#evidence-report")
            assert screen.tab == "report"
            await screen.inspect_evidence("news:deleted")
            assert "no longer available" in str(screen.query_one("#source-body", Static).render())
            screen.query_one("#research-company", Select).value = "US:MSFT"
            await pilot.pause()
            assert screen.state.company == "US:MSFT"
            assert not screen.items
            assert screen.report is None
            screen.query_one("#research-company", Select).value = INST
            await pilot.pause()
            assert screen.view.search == "close"
            assert screen.report.target_id == INST

    asyncio.run(run())


@pytest.mark.parametrize("theme", ["rigger-dark", "rigger-light"])
def test_aliases_share_state_and_narrow_details(tmp_engine, tmp_path, monkeypatch, theme):
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    saved_report(rig)
    state = ResearchState()

    class TestApp(App):
        def on_mount(self):
            for registered in THEMES:
                self.register_theme(registered)
            self.theme = theme
            self.install_screen(Data(rig, state), "data")
            self.install_screen(Reports(rig, state), "reports")
            self.push_screen("data")

    async def run():
        async with TestApp().run_test(size=(80, 24)) as pilot:
            screen = pilot.app.screen
            screen.query_one("#evidence-search", Input).value = "cloud"
            await pilot.pause()
            assert screen.query_one("#evidence-list-pane").display
            assert not screen.query_one("#evidence-preview-pane").display
            await screen.inspect_evidence("news:news-1")
            assert not screen.query_one("#evidence-list-pane").display
            assert screen.query_one("#evidence-preview-pane").display
            await pilot.pause()
            await pilot.click("#evidence-back")
            assert screen.query_one("#evidence-list-pane").display
            pilot.app.switch_screen("reports")
            await pilot.pause()
            assert pilot.app.screen.tab == "report"
            assert pilot.app.screen.state.company == INST
            assert pilot.app.screen.view.search == "cloud"
            assert pilot.app.screen.query_one("#report-generate").region.right <= 80

    asyncio.run(run())


def test_generation_captures_company_and_handles_failure(tmp_engine, tmp_path, monkeypatch):
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    original = saved_report(rig)
    started, release = asyncio.Event(), asyncio.Event()

    async def build(_rig, company):
        assert company == INST
        started.set()
        await release.wait()
        return original.model_copy(update={"summary": "Updated"})

    monkeypatch.setattr("rigger.tui.screens.research.build_report", build)

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Reports(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            worker = screen.generate(INST)
            await started.wait()
            assert screen.query_one("#report-generate", Button).disabled
            screen.query_one("#research-company", Select).value = "US:MSFT"
            await pilot.pause()
            release.set()
            await worker.wait()
            assert screen.state.company == "US:MSFT"
            assert "Updated" not in screen.query_one(MarkdownViewer).document.source
            screen.query_one("#research-company", Select).value = INST
            await pilot.pause()
            assert "Updated" in screen.query_one(MarkdownViewer).document.source

            async def fail(*_):
                raise ValueError("Provider unavailable")

            monkeypatch.setattr("rigger.tui.screens.research.build_report", fail)
            await screen.generate(INST).wait()
            assert "Updated" in screen.query_one(MarkdownViewer).document.source
            assert not screen.state.busy

    asyncio.run(run())


def test_report_scroll_returns_and_first_citing_claim(tmp_engine, tmp_path, monkeypatch):
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    report = saved_report(rig)
    report.bull = [Claim(text=f"Earlier claim {n}", evidence_ids=["bar:1"]) for n in range(30)]
    report.bull.append(Claim(text="Final partnership claim", evidence_ids=["news:news-1"]))
    write_report(report, rig.cfg.reports_dir)

    state = ResearchState()

    class TestApp(App):
        def on_mount(self):
            self.install_screen(Data(rig, state), "data")
            self.install_screen(Reports(rig, state), "reports")
            self.push_screen("reports")

    async def run():
        async with TestApp().run_test(size=(120, 30)) as pilot:
            screen = pilot.app.screen
            viewer = screen.query_one(MarkdownViewer)
            await pilot.pause()
            viewer.scroll_to(y=10, animate=False)
            await pilot.pause()
            position = viewer.scroll_y
            await screen.inspect_evidence("news:news-1")
            await pilot.pause()
            await pilot.click("#tab-report")
            await pilot.pause()
            assert viewer.scroll_y == position
            await screen.inspect_evidence("news:news-1")
            await pilot.pause()
            await pilot.click("#evidence-report")
            await pilot.pause()
            assert viewer.scroll_y > position
            viewer.scroll_to(y=20, animate=False)
            await pilot.pause()
            pilot.app.switch_screen("data")
            await pilot.pause()
            pilot.app.switch_screen("reports")
            await pilot.pause()
            assert viewer.scroll_y == 20

    asyncio.run(run())


def test_settings_diagnostics_and_gather_refresh(tmp_engine, tmp_path, monkeypatch):
    from textual.widgets import Collapsible

    from rigger.tui.screens.config import Config

    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    rig.plugins = {}
    calls = []

    async def ingest(_, **kwargs):
        calls.append(("ingest", kwargs.get("tickers")))

    async def extract(_, **kwargs):
        calls.append(("extract", kwargs.get("instruments")))

    monkeypatch.setattr("rigger.services.ingest", ingest)
    monkeypatch.setattr("rigger.services.extract", extract)

    class TestApp(App):
        def on_mount(self):
            self.install_screen(Config(rig), "config")
            self.push_screen(Data(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            await screen.gather_all().wait()
            assert calls == [("ingest", None), ("extract", None)]
            assert screen.items
            assert not screen.state.busy
            pilot.app.switch_screen("config")
            await pilot.pause()
            settings = pilot.app.screen
            assert settings.query_one(Collapsible).collapsed
            assert settings.query_one("#health-table").row_count > 0
            assert "US:AAPL" in str(settings.query_one("#health-latest", Static).render())

    asyncio.run(run())


def test_report_age_and_section_counts_surface_staleness(tmp_engine, tmp_path, monkeypatch):
    """A week-old report must not read the same as a fresh one."""
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    stale = Report(
        target_id=INST,
        as_of=datetime.now(UTC) - timedelta(days=9),
        prompt_version="report_v1",
        summary="An ageing summary",
        sentiment=-0.5,
        bull=[Claim(text="One bull claim.", evidence_ids=["news:news-1"])],
        bear=[Claim(text="One bear claim.", evidence_ids=["news:news-1"])],
        citations={"news:news-1": "Partnership filing"},
    )
    write_report(stale, rig.cfg.reports_dir)

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Reports(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            label, state = screen.report_age(None)
            assert label == "9d old"
            assert state == "error"
            assert screen.query_one("#report-age-dot").state == "error"
            # Sentiment is a coloured pill, not buried italic body text.
            assert "-0.50" in str(screen.query_one("#report-sentiment").render())
            assert screen.query_one("#report-sentiment").has_class("-error")
            # The badge says how much substance the report has.
            badge = str(screen.query_one("#report-doc")._bar._badge.render())
            assert "bull 1" in badge and "bear 1" in badge
            # A claim's source count is visible without counting cite lines.
            assert "(1 source)" in screen.query_one(MarkdownViewer).document.source

    asyncio.run(run())


def test_same_day_regeneration_keeps_the_previous_run(tmp_engine, tmp_path, monkeypatch):
    """The change between two runs is the signal; it must survive a rerun."""
    from rigger.reports import report_history

    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    base = rig.cfg.reports_dir
    day = datetime.now(UTC).replace(hour=1, minute=0, second=0, microsecond=0)

    def run_at(when, sentiment):
        return write_report(
            Report(
                target_id=INST,
                as_of=when,
                prompt_version="report_v1",
                summary="s",
                sentiment=sentiment,
                bull=[Claim(text="A claim.", evidence_ids=["news:news-1"])],
                citations={"news:news-1": "Partnership filing"},
            ),
            base,
        )

    first = run_at(day, -0.10)
    second = run_at(day.replace(hour=2), -0.30)
    assert first == second, "the canonical path stays the dated file"

    history = report_history(base, INST)
    assert [round(report.sentiment, 2) for report in history] == [-0.30, -0.10]

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Reports(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            # show_latest still resolves the newest run, not an archived one.
            assert screen.report is not None
            assert round(screen.report.sentiment, 2) == -0.30
            assert "was -0.10" in str(screen.query_one("#report-sentiment-delta").render())
            history = screen.query_one("#report-history")
            assert history.display
            assert "-0.30" in str(history.render()) and "-0.10" in str(history.render())

    asyncio.run(run())


def test_status_line_reports_a_price_not_a_bare_date(tmp_engine, tmp_path, monkeypatch):
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Reports(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            close = evidence(rig.engine, target=INST, kind="bar", limit=1)[0].raw["close"]
            assert f"{close:,.2f} USD" in screen.last_close(INST)
            assert f"{close:,.2f} USD" in screen.status_text
            assert screen.last_close("US:NOPE") == "last close: none"

    asyncio.run(run())


def test_every_action_is_reachable_from_the_keyboard(tmp_engine, tmp_path, monkeypatch):
    """The app is keyboard-first; this screen used to be mouse-only."""
    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    saved_report(rig)

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Reports(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            await pilot.press("e")
            assert screen.tab == "evidence"
            await pilot.press("r")
            assert screen.tab == "report"
            # The active tab is marked by class, leaving one primary button.
            assert screen.query_one("#tab-report", Button).has_class("-tab-active")
            primaries = [
                button
                for button in screen.query(Button)
                if button.variant == "primary" and button.display
            ]
            assert [button.id for button in primaries] == ["report-generate"]
            await pilot.press("slash")
            assert screen.tab == "evidence"
            assert screen.query_one("#evidence-search", Input).has_focus
            # None of the screen keys may shadow the app-level navigation keys.
            app_keys = {"1", "2", "3", "4", "5", "c", "h", "m", "p", "g", "q"}
            assert not app_keys & {key for key, _, _ in screen.BINDINGS}

    asyncio.run(run())


def test_promote_claim_creates_a_thesis_with_its_evidence(tmp_engine, tmp_path, monkeypatch):
    """A report claim becomes a thesis without re-finding its sources by hand."""
    from rigger import theses

    rig = setup_rig(tmp_engine, tmp_path, monkeypatch)
    saved_report(rig)

    class TestApp(App):
        def on_mount(self):
            self.push_screen(Reports(rig))

    async def run():
        async with TestApp().run_test() as pilot:
            screen = pilot.app.screen
            assert "(thesis:bull:0)" in screen.query_one(MarkdownViewer).document.source
            screen.promote_claim("bull:0")
            await pilot.pause()
            dialog = pilot.app.screen
            assert dialog.query_one("#th-claim", Input).value == "The companies partnered."
            assert dialog.query_one("#th-targets", Input).value == INST
            await pilot.press("enter")
            await pilot.pause()
            stored = theses.list_theses(rig.engine)
            assert [thesis.claim for thesis in stored] == ["The companies partnered."]
            linked = theses.evidence_for(rig.engine, stored[0].id)
            assert [item.evidence_id for item in linked] == ["news:news-1"]
            assert linked[0].side == "support"
            # A claim reference that no longer resolves must not raise.
            screen.promote_claim("bull:99")
            screen.promote_claim("nonsense")

    asyncio.run(run())
