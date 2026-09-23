"""TUI smoke tests: mount the app offline against a seeded engine."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace

import pytest
from textual.widgets import Input, OptionList, Select

from delta import services
from delta.core.models import Instrument
from delta.quotes import SearchResult
from delta.tui.app import DeltaApp
from delta.tui.screens.targets import TargetAddModal
from delta.tui.shell import ALL_ITEMS, OFF_BAR_ITEMS, ScreenFooter, StatusBar
from delta.tui.widgets import (
    MODAL_WIDTH,
    MODAL_WIDTH_WIDE,
    Dialog,
    KeyHint,
    binding_key,
    shown_bindings,
)
from tests.conftest import seed_bars

AAPL = Instrument(id="US:AAPL", market="us", symbol="AAPL", currency="USD", sector="Tech")


class FakeRig:
    def __init__(self, engine, universe):
        self.engine = engine
        self.llm = None
        self._universe = universe
        self.settings = SimpleNamespace(
            openrouter_api_key="", openai_api_key="", anthropic_api_key=""
        )
        self.cfg = SimpleNamespace(
            base_currency="AUD",
            llm_provider="openrouter",
            llm_routing={},
            plugins={"sec_edgar": {}},
            universe={},
            targets={},
        )
        self.plugins = {"sec_edgar": SimpleNamespace(enabled=True)}

    def universe(self):
        return self._universe

    def context(self, universe):
        from delta.core.plugin import Context

        return Context(
            engine=self.engine,
            settings=self.settings,
            config=self.cfg,
            llm=self.llm,
            universe=universe,
            plugins=self.plugins,
        )


@pytest.fixture
def delta(tmp_engine, tmp_path):
    seed_bars(tmp_engine, AAPL.id, price_fn=lambda i: 100.0)
    fake = FakeRig(tmp_engine, [AAPL])
    # Keep the last-seen state file out of the repo: the app stamps a visit on start.
    fake.cfg.db_path = str(tmp_path / "delta.db")
    return fake


def test_app_mounts_and_navigates(delta):
    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            assert app.screen.name == "home"
            for key, name, _label in ALL_ITEMS:
                await pilot.press(key)
                assert app.screen.name == name
            # Research is the one shared desk: no Reports route remains, by
            # key, by Go picker entry, or in the installed screens.
            assert "reports" not in app.screens_by_name
            assert any(
                key == "4" and name == "theses" for key, name, _label in ALL_ITEMS
            )
            await pilot.press("4")
            assert app.screen.name == "theses"

    asyncio.run(run())


def test_status_bar_is_one_row(delta):
    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 24)) as pilot:
            # Home is a DeltaScreen like every panel: the bar is there where a
            # new user lands, so the 1/2/3/4/5/6 rail never disappears.
            assert app.screen.name == "home"
            bar = app.screen.query_one(StatusBar)
            assert not app.screen.query("#nav-console")
            # Off-bar screens stay reachable by key but earn no columns.
            for _key, name, _label in OFF_BAR_ITEMS:
                assert not app.screen.query(f"#nav-{name}")
            # Panels, status cells and chrome all share the one row.
            for sel in ("#nav-targets", "#nav-config", "#sl-dot", "#sl-keys"):
                assert bar.query(sel), sel
            # The bar itself is one row; the footer adds a second as padding so
            # the bar does not sit flush against the pane border above it.
            assert bar.size.height == 1
            # ``size`` is the content box, which excludes the padding row, so
            # the two-row claim has to be made against the laid-out region.
            footer = app.screen.query_one(ScreenFooter)
            assert footer.outer_size.height == 2
            assert footer.region.height == 2
            assert bar.region.y == footer.region.y + 1
            await pilot.press("c")
            assert app.screen.name == "config"

    asyncio.run(run())


def test_status_bar_sheds_cells_when_narrow(delta):
    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(70, 24)) as pilot:
            await pilot.press("2")
            bar = app.screen.query_one(StatusBar)
            # Below MINIMAL_WIDTH the hint and provider give up their columns;
            # freshness and spend are the cells worth keeping.
            assert not bar.query_one("#sl-keys").display
            assert bar.query_one("#sl-dot").display

    asyncio.run(run())


def test_targets_panel_adds_target(delta, monkeypatch, tmp_path):
    import tomli_w

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps({"universe": {"us": ["AAPL"]}}), encoding="utf-8"
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2")
            assert app.screen.name == "targets"
            await pilot.press("a")
            assert app.screen.query_one("#tg-form").display
            app.screen.query_one("#tg-name").value = "mining"
            app.screen.query_one("#tg-kind").value = "industry"
            app.screen.query_one("#tg-market").value = "asx"
            app.screen.query_one("#tg-tickers").value = "BHP,RIO"
            await pilot.press("enter")
            assert "mining" in services.target_specs()
            assert app.screen.query_one("#target-table").row_count == 1

    asyncio.run(run())


def test_question_mark_opens_help_then_closes(delta):
    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            assert app.screen.name == "home"
            await pilot.press("?")
            assert app.screen.name == "help"
            await pilot.press("?")
            assert app.screen.name == "home"

    asyncio.run(run())


def test_targets_panel_remove_with_no_targets_notifies(delta, monkeypatch, tmp_path):
    """An empty DataTable reports cursor_row == 0, so row_count is the real guard."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2")
            assert app.screen.query_one("#target-table").row_count == 0
            await pilot.press("d")
            await pilot.pause()

    asyncio.run(run())


def test_home_pulse_reference_point_is_stable_across_refreshes(delta):
    """last_seen is captured once per session; re-reading it would zero the panel."""

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            home = app.screen
            captured = home.last_seen
            await home.refresh_view()
            await pilot.pause()
            await home.refresh_view()
            await pilot.pause()
            assert home.last_seen == captured

    asyncio.run(run())


@pytest.fixture(autouse=True)
def offline_quotes(monkeypatch):
    from delta.quotes import YahooQuotes

    async def run(self):
        self.on_state("offline test")
        await asyncio.Event().wait()

    monkeypatch.setattr(YahooQuotes, "run", run)


@pytest.mark.parametrize("size", [(80, 24), (120, 40)])
@pytest.mark.parametrize("theme", ["delta-dark", "delta-light"])
def test_ledger_groups_quotes_and_focus(delta, monkeypatch, tmp_path, size, theme):
    import tomli_w

    from delta.quotes import parse_quote

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {
                "targets": {
                    "mining": {
                        "kind": "sector",
                        "market": "asx",
                        "tickers": ["BHP", "RIO"],
                        "tags": ["resources"],
                    },
                    "whole-market": {"kind": "market", "market": "us"},
                }
            }
        )
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=size) as pilot:
            app.theme = theme
            await pilot.press("2")
            screen = app.screen
            table = screen.query_one("#target-table")
            assert table.row_count == 2
            await pilot.press("enter")
            # Enter refreshes the always-open inspector; grouped assets are
            # no longer expanded into child rows.
            assert table.row_count == 2
            # A multi-ticker target steps through its members with the arrows.
            assert screen._members_by_target["mining"] == ["ASX:BHP", "ASX:RIO"]
            assert screen._selected_instrument.id == "ASX:BHP"
            await pilot.press("right")
            assert screen._selected_instrument.id == "ASX:RIO"
            await pilot.press("left")
            assert screen._selected_instrument.id == "ASX:BHP"
            assert len(screen.feed.symbols) == 2
            screen.feed.quotes["ASX:BHP"] = parse_quote(
                {"price": 42.18, "time": 1789516800000, "change_percent": -0.4}, "AUD"
            )
            selected = screen._selected()
            screen._paint_quotes()
            assert screen._selected() == selected
            assert "child:mining:ASX:BHP" not in screen.rows
            await pilot.press("d")
            assert "mining" not in services.target_specs()
            await pilot.press("/", "r", "e", "s")
            assert table.row_count == 0
            await pilot.press("escape")
            assert table.row_count == 1
            assert table.region.right <= size[0]
            task = screen.feed_task
            await pilot.press("1")
            assert task.done()
            assert screen.feed_task is None

    asyncio.run(run())


@pytest.mark.parametrize("field", ["name", "tickers", "tags"])
def test_company_form_enter_submits(delta, monkeypatch, tmp_path, field):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            screen = app.screen
            screen.query_one("#tg-name").value = "apple"
            screen.query_one("#tg-market").value = "us"
            screen.query_one("#tg-tickers").value = "AAPL"
            screen.query_one(f"#tg-{field}").focus()
            await pilot.press("enter")
            assert services.target_specs()["apple"].tickers == ("AAPL",)
            assert app.screen.name == "targets"
            assert not app.screen.query("#tg-form")
            assert app.screen.query_one("#target-table").has_focus

    asyncio.run(run())


def test_company_form_enter_keeps_invalid_form_open(delta, monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            app.screen.query_one("#tg-name").value = "apple"
            await pilot.press("enter")
            assert not services.target_specs()
            assert app.screen.query_one("#tg-form").display
            assert app.screen.query_one("#tg-name").value == "apple"

    asyncio.run(run())


def _fake_yahoo(results: list[SearchResult], calls: list[str] | None = None):
    async def search(query: str, max_results: int = 8) -> list[SearchResult]:
        if calls is not None:
            calls.append(query)
        return list(results)

    return search


def test_add_modal_yahoo_dropdown_keyboard_and_online_state(delta, monkeypatch, tmp_path):
    """Arrows browse the dropdown without leaving the name field, enter picks
    the canonical ticker, and escape closes the dialog itself."""
    from delta import tui

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def yahoo(query):
        assert query == "bhp"
        return [SearchResult("BHP.AX", "BHP Group Limited", "asx", "AUD", "ASX")]

    monkeypatch.setattr(tui.screens.targets, "yahoo_search", yahoo)
    monkeypatch.setattr(TargetAddModal, "SEARCH_DEBOUNCE", 0.01)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            modal = app.screen
            assert isinstance(modal, TargetAddModal)
            name = modal.query_one("#tg-name", Input)
            name.focus()
            for char in "bhp":
                await pilot.press(char)
            await pilot.pause(0.2)
            suggestions = modal.query_one("#tg-suggestions", OptionList)
            assert len(suggestions.options) == 1
            assert modal.query_one("#tg-network").has_class("-online")
            await pilot.press("down")
            assert modal.focused is name  # browsing never steals the input
            await pilot.press("enter")  # pick
            assert modal.query_one("#tg-tickers", Input).value == "BHP"
            assert modal.query_one("#tg-market", Select).value == "asx"
            assert not suggestions.options  # dropdown closed after the pick
            await pilot.press("escape")  # nothing left to close: the dialog goes
            assert app.screen.name == "targets"

    asyncio.run(run())


def test_add_modal_browse_keeps_focus_then_pick_saves(delta, monkeypatch, tmp_path):
    """The full pick-then-confirm flow writes the target the result named."""

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    monkeypatch.setattr(
        "delta.tui.screens.targets.yahoo_search",
        _fake_yahoo([SearchResult("BHP.AX", "BHP Group", "asx", "AUD", "ASX")]),
    )
    monkeypatch.setattr(TargetAddModal, "SEARCH_DEBOUNCE", 0.01)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            modal = app.screen
            name = modal.query_one("#tg-name", Input)
            name.focus()
            for char in "bhp":
                await pilot.press(char)
            await pilot.pause(0.2)
            assert len(modal.query_one("#tg-suggestions", OptionList).options) == 1
            await pilot.press("down", "enter")  # pick
            assert modal.query_one("#tg-tickers", Input).value == "BHP"
            assert modal.query_one("#tg-market", Select).value == "asx"
            assert modal.query_one("#tg-asset-class", Select).value == "equity"
            await pilot.press("enter")  # confirm
            specs = services.target_specs()
            assert specs["BHP Group"].markets == ("asx",)
            assert specs["BHP Group"].tickers == ("BHP",)

    asyncio.run(run())


def test_add_modal_debounces_the_yahoo_lookup(delta, monkeypatch, tmp_path):
    """One lookup per finished thought, never one per keystroke."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    calls: list[str] = []
    monkeypatch.setattr("delta.tui.screens.targets.yahoo_search", _fake_yahoo([], calls))
    monkeypatch.setattr(TargetAddModal, "SEARCH_DEBOUNCE", 0.5)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            name = app.screen.query_one("#tg-name", Input)
            name.focus()
            for char in "bhp":
                await pilot.press(char)
                await pilot.pause(0.05)
            assert calls == []  # the thought is not finished yet
            await pilot.pause(0.7)
            assert calls == ["bhp"]

    asyncio.run(run())


def test_add_modal_merges_local_and_remote_results(delta, monkeypatch, tmp_path):
    """Remote answers join local matches instead of replacing them."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")
    monkeypatch.setattr(
        "delta.tui.screens.targets.yahoo_search",
        _fake_yahoo(
            [
                SearchResult("AAPL", "Apple Inc.", "us", "USD", "NASDAQ"),
                SearchResult("MSFT", "Microsoft", "us", "USD", "NASDAQ"),
            ]
        ),
    )
    monkeypatch.setattr(TargetAddModal, "SEARCH_DEBOUNCE", 0.01)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            name = app.screen.query_one("#tg-name", Input)
            name.focus()
            for char in "aapl":
                await pilot.press(char)
            await pilot.pause(0.2)
            options = app.screen.query_one("#tg-suggestions", OptionList)
            ids = [str(option.id) for option in options.options]
            assert ids == ["us:aapl", "us:msft"]  # local pinned, remote added, no dup

    asyncio.run(run())


def test_add_modal_stale_pick_keeps_the_result_market(delta, monkeypatch, tmp_path):
    """A pick whose result object is gone recovers from the option key —
    it must never silently write a US target for a non-US listing."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            modal = app.screen
            modal._results_by_key.clear()
            modal._pick("asx:BHP")
            assert modal.query_one("#tg-tickers", Input).value == "BHP"
            assert modal.query_one("#tg-market", Select).value == "asx"

    asyncio.run(run())


def test_add_modal_marks_watched_and_saves_dupes_anyway(delta, monkeypatch, tmp_path):
    import tomli_w

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {"targets": {"bhp": {"kind": "company", "market": "asx", "tickers": ["BHP"]}}}
        ),
        encoding="utf-8",
    )

    async def fake_search(query: str, max_results: int = 8) -> list[SearchResult]:
        raise ConnectionError("offline")

    monkeypatch.setattr("delta.tui.screens.targets.yahoo_search", fake_search)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            await pilot.press("2", "a")
            modal = app.screen
            name = modal.query_one("#tg-name", Input)
            name.focus()
            for char in "bhp":
                await pilot.press(char)
            await pilot.pause(0.1)
            options = modal.query_one("#tg-suggestions", OptionList)
            assert len(options.options) == 1
            assert "watched" in str(options.options[0].prompt)
            # Warn, allow: the duplicate ticker still saves under a new target.
            modal.query_one("#tg-name", Input).value = "big australian"
            modal.query_one("#tg-tickers", Input).value = "BHP"
            await pilot.press("enter")
            assert "big australian" in services.target_specs()

    asyncio.run(run())


def test_theme_tokens_are_readable_on_black():
    """Base tokens are ink; anything with ``color:`` must use a ``text-`` token."""
    import re
    from pathlib import Path

    from textual.color import Color

    from delta.tui.theme import DELTA_DARK

    tui = Path(__file__).resolve().parents[1] / "delta" / "tui"
    pattern = re.compile(r"color: \$(primary|secondary|accent|success|error|warning)\b")
    # A graphic is ink, not text: the braille graph's low end is the brand
    # blue on purpose, and reading it never depends on that contrast — the
    # bright end and the figures above the chart carry the meaning.
    graphic = re.compile(r"--(low|high)-color")
    offenders = [
        f"{path.relative_to(tui)}:{n}"
        for path in list(tui.rglob("*.py")) + list(tui.rglob("*.tcss"))
        if path.name != "theme.py"
        for n, line in enumerate(path.read_text().splitlines(), 1)
        if pattern.search(line) and not graphic.search(line)
    ]
    assert not offenders, offenders

    def luminance(color: Color) -> float:
        def channel(value: int) -> float:
            c = value / 255
            return c / 12.92 if c <= 0.03928 else ((c + 0.055) / 1.055) ** 2.4

        return 0.2126 * channel(color.r) + 0.7152 * channel(color.g) + 0.0722 * channel(color.b)

    black = luminance(Color.parse(DELTA_DARK.background))
    for token in ("text-primary", "text-error", "text-success", "text-warning", "text-muted"):
        ratio = (luminance(Color.parse(DELTA_DARK.variables[token])) + 0.05) / (black + 0.05)
        assert ratio >= 4.5, (token, ratio)


def _routed_delta(delta):
    delta.cfg.llm_routing = {"chat": "x/sonnet", "extract": "x/haiku", "report": "x/sonnet"}
    delta.cfg.targets = {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}
    return delta


def test_config_two_columns_fold_and_focus_keys(delta):
    """Wide: diagnostics open beside the stack; d folds; l moves focus; enter opens the picker."""
    from delta.tui.screens.model_picker import ModelPicker

    async def run():
        app = DeltaApp(_routed_delta(delta))
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("c")
            screen = app.screen
            assert screen.name == "config"
            assert not screen.query("Collapsible")
            assert screen.query_one("#cfg-diag-body").display
            assert not screen.query_one("#cfg-diag-summary").display
            assert screen.query_one("#cfg-left").display
            assert screen.focused is screen.query_one("#cfg-model")
            await pilot.press("d")
            assert screen.query_one("#cfg-diag-summary").display
            assert not screen.query_one("#cfg-diag-body").display
            await pilot.press("d")
            assert screen.query_one("#cfg-diag-body").display
            await pilot.press("l")
            assert screen.focused is screen.query_one("#cfg-plugins")
            # The targets pane is gone from Settings: watching is managed on
            # the watchlist alone, and nothing here may reference it.
            assert not screen.query("#cfg-targets-pane")
            # Enter acts on the highlighted row: provider opens the provider picker…
            from delta.tui.screens.provider_picker import ProviderPicker

            screen.query_one("#cfg-model").focus()
            await pilot.press("enter")
            await pilot.pause()
            assert isinstance(app.screen, ProviderPicker)
            await pilot.press("escape")
            await pilot.pause()
            assert app.screen is screen
            # …and the model row opens the model picker.
            await pilot.press("down")
            await pilot.press("enter")
            await pilot.pause()
            assert isinstance(app.screen, ModelPicker)

    asyncio.run(run())


def test_config_model_row_sets_one_model_for_all_tasks(delta, monkeypatch, tmp_path):
    """The AI pane is two rows; picking the model row writes [llm] model, never a route."""
    import tomllib

    from delta.llm.catalog import ModelInfo

    monkeypatch.chdir(tmp_path)
    delta = _routed_delta(delta)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("c")
            screen = app.screen
            table = screen.query_one("#cfg-model")
            assert table.row_count == 2  # provider + model, nothing else
            table.focus()
            await pilot.press("down")  # land on the model row
            await pilot.press("enter")
            await pilot.pause()
            picker = app.screen
            picker._select(
                ModelInfo(
                    id="test/model",
                    name="Test Model",
                    context_length=None,
                    prompt_price=0.0,
                    completion_price=0.0,
                )
            )
            await pilot.pause(0.2)

    asyncio.run(run())
    raw = tomllib.loads((tmp_path / "config.toml").read_text(encoding="utf-8"))
    assert raw["llm"]["model"] == "test/model"
    assert "routing" not in raw["llm"]


def test_model_picker_autocomplete_in_the_real_flow(delta, monkeypatch, tmp_path):
    """Open the picker the way the app does and just type: the dropdown must
    open with the field already focused — no manual .focus() help."""
    import tomli_w

    from delta.llm.catalog import ModelInfo, _write_cache
    from delta.tui.screens.model_picker import ModelPicker

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(tomli_w.dumps({}), encoding="utf-8")
    _write_cache(
        "openrouter",
        [
            ModelInfo(
                id="anthropic/claude-sonnet-4",
                name="Claude Sonnet 4",
                context_length=200000,
                prompt_price=0.0,
                completion_price=0.0,
            ),
            ModelInfo(
                id="openai/gpt-4o",
                name="GPT-4o",
                context_length=128000,
                prompt_price=0.0,
                completion_price=0.0,
            ),
        ],
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("c")
            screen = app.screen
            screen.query_one("#cfg-model").focus()
            await pilot.press("down", "enter")  # model row
            await pilot.pause()
            picker = app.screen
            assert isinstance(picker, ModelPicker)
            for char in "gpt":
                await pilot.press(char)
            await pilot.pause()
            filt = picker.query_one("#mp-filter", Input)
            assert filt.has_focus, f"filter never took focus (focused={picker.focused!r})"
            assert filt.value == "gpt"
            suggestions = picker.query_one("#mp-suggestions")
            assert suggestions.display, "dropdown did not open while typing"

    asyncio.run(run())


def test_config_narrow_folds_diagnostics_until_d(delta):
    """80x24: one column, diagnostics folded; d expands full-height, esc closes."""

    async def run():
        app = DeltaApp(_routed_delta(delta))
        async with app.run_test(size=(80, 24)) as pilot:
            await pilot.press("c")
            screen = app.screen
            assert screen.has_class("-narrow")
            assert screen.query_one("#cfg-diag-summary").display
            assert not screen.query_one("#cfg-diag-body").display
            assert screen.query_one("#cfg-left").display
            # Nothing may fall off the bottom: the body ends above the footer.
            body = screen.query_one("#cfg-body")
            assert body.region.bottom <= screen.size.height - 1
            await pilot.press("d")
            assert not screen.query_one("#cfg-left").display
            assert screen.query_one("#cfg-diag-body").display
            await pilot.press("escape")
            assert screen.query_one("#cfg-left").display
            assert not screen.query_one("#cfg-diag-body").display

    asyncio.run(run())


def test_home_refreshes_twice_without_duplicate_ids(delta, monkeypatch, tmp_path):
    """Home's boxes update in place.

    A remove-then-mount rebuild raced Textual's async ``remove_children`` and
    crashed with DuplicateIds on the second refresh — the first repaint after
    the screen had already been drawn once.
    """
    import tomli_w

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {"targets": {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}}
        ),
        encoding="utf-8",
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test() as pilot:
            home = app.screen
            assert home.name == "home"
            for _ in range(3):
                await home.refresh_view()
                await pilot.pause()
            assert len(app.screen.query("#system-db")) == 1
            assert len(app.screen.query("#system-plugins")) == 1

    asyncio.run(run())


def _home_config(tmp_path, monkeypatch, delta):
    """Chdir to a config with one watched ticker, so Home has a watchlist row."""
    import tomli_w

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {"targets": {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}}
        ),
        encoding="utf-8",
    )
    delta.cfg.reports_dir = str(tmp_path / "reports")
    return tmp_path


def _frame(app):
    """The exported frame as plain text: the SVG writes spaces as ``&#160;``."""
    return app.export_screenshot().replace("&#160;", " ")


def _quote(price=1234.5, change_pct=2.5, age_seconds=0.0):
    from datetime import UTC, datetime, timedelta

    from delta.quotes import Quote

    now = datetime.now(UTC)
    return Quote(
        price=price,
        currency="USD",
        change_pct=change_pct,
        timestamp=now,
        received_at=now - timedelta(seconds=age_seconds),
    )


def test_home_watchlist_paints_a_live_quote(delta, monkeypatch, tmp_path):
    """A quote reaches the screen, not just the feed dict.

    Asserting on ``feed.quotes`` alone passes for a row that never repaints,
    so this drives a real app and reads the exported frame: the live price,
    the live dot and the ``last`` header must all be there.
    """
    _home_config(tmp_path, monkeypatch, delta)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            home = app.screen
            head = home.query_one("#watch-head")
            # The price column names what it holds: chars 11..22 of the header.
            assert str(head.render())[11:22].strip() == "close"
            assert "●" not in str(head.render())

            home.feed.quotes["US:AAPL"] = _quote()
            home._paint_quotes()
            await pilot.pause()

            assert str(head.render())[11:22].strip() == "last"
            assert "● live" in str(head.render())
            frame = _frame(app)
            assert "1,234.50" in frame
            assert "+2.50%" in frame
            assert "● live" in frame
            # The age column says how fresh the price is: the dot in the header
            # and the row's own age cell are two separate "live"s on screen.
            assert frame.count("live") >= 2

    asyncio.run(run())


def test_home_quote_age_falls_back_to_the_stored_close(delta, monkeypatch, tmp_path):
    """No quote: the row shows the last stored bar under a ``close`` header, and
    an old quote is marked stale rather than shown as live."""
    from delta.tui.screens.home import WatchRow

    _home_config(tmp_path, monkeypatch, delta)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            home = app.screen
            row = home.query_one(WatchRow)
            assert row.quote is None
            assert "100.00" in str(row.query_one(".w-close").render())
            assert str(row.query_one(".w-age").render()) == ""

            row.set_quote(_quote(age_seconds=60_000))
            await pilot.pause()
            assert str(row.query_one(".w-age").render()) == "16h"
            assert row.query_one(".w-age").has_class("-stale")

    asyncio.run(run())


def test_home_quote_feed_starts_on_resume_and_is_cancelled_on_unmount(delta, monkeypatch, tmp_path):
    """A leaked feed task keeps a websocket alive after the screen is gone."""
    _home_config(tmp_path, monkeypatch, delta)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            home = app.screen
            assert home.active
            assert home.feed is not None
            assert list(home.feed.symbols) == ["AAPL"]
            task = home.feed_task
            assert task is not None

            # Leaving Home suspends the screen: the task must go with it.
            await pilot.press("2")
            await pilot.pause()
            assert home.feed_task is None
            assert task.cancelled() or task.done()
            assert not home.active

            await pilot.press("h")
            await pilot.pause()
            assert home.active and home.feed_task is not None

    asyncio.run(run())


def test_home_warns_that_a_company_report_is_stale(delta, monkeypatch, tmp_path):
    """Per-company report age, which Home could not say before.

    No report at all is not a warning — nothing is out of date until something
    has been written.
    """
    _home_config(tmp_path, monkeypatch, delta)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            home = app.screen
            stale = home.query_one("#since-stale")
            assert "report" not in str(stale.render())

            folder = tmp_path / "reports" / "US:AAPL"
            folder.mkdir(parents=True)
            (folder / "2020-01-01.md").write_text("# report\n", encoding="utf-8")
            await home.refresh_view()
            await pilot.pause()

            assert "AAPL report" in str(stale.render())
            assert "old" in str(stale.render())
            assert "AAPL report" in _frame(app)

    asyncio.run(run())


def test_home_survives_several_seconds_of_ticks(delta, monkeypatch, tmp_path):
    """The 1s clock and the 0.5s quote repaint must not race the box rebuild.

    Home crashed with DuplicateIds once because ``remove_children`` is async;
    a green helper test hid it, so this lets the real timers fire repeatedly.
    """
    _home_config(tmp_path, monkeypatch, delta)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            home = app.screen
            home.feed.quotes["US:AAPL"] = _quote()
            for _ in range(12):
                await pilot.pause(0.25)
            assert app._exception is None
            assert len(app.screen.query("#system-db")) == 1
            assert len(app.screen.query("#watch-head")) == 1
            assert "1,234.50" in _frame(app)

    asyncio.run(run())


def test_braille_graph_paints_in_a_running_app():
    """The graph reaches the screen, not just the renderable.

    Asserting on ``rows()`` alone passes for a widget Textual never paints;
    this drives a real app and looks for braille in the exported frame.
    """
    from textual.app import App, ComposeResult

    from delta.tui.widgets import BrailleGraph

    class GraphApp(App):
        CSS = "BrailleGraph { width: 40; height: 6; }"

        def compose(self) -> ComposeResult:
            yield BrailleGraph([float(i) for i in range(120)], id="g")

    async def run():
        app = GraphApp()
        async with app.run_test(size=(60, 12)) as pilot:
            await pilot.pause()
            frame = app.export_screenshot()
            assert sum(1 for ch in frame if 0x2800 <= ord(ch) <= 0x28FF) > 20

    asyncio.run(run())


def test_braille_graph_resolution_and_shape():
    """The graph uses the whole box: 4 dot rows per cell, 2 sample columns."""
    from delta.tui.widgets import BrailleGraph

    rising = BrailleGraph(list(range(100)))
    rows = rising.rows(20, 4)
    assert len(rows) == 4
    assert all(len(row) == 20 for row in rows)
    # A rising series ends high and starts low: the first cell of the top row
    # is blank, the last is not, and the bottom row is the other way round.
    assert rows[0][0] == BrailleGraph.EMPTY and rows[0][-1] != BrailleGraph.EMPTY
    assert rows[-1][0] != BrailleGraph.EMPTY and rows[-1][-1] == BrailleGraph.EMPTY

    # Filled draws every dot below the line, so a full box is denser than a line.
    line = BrailleGraph(list(range(100)))
    area = BrailleGraph(list(range(100)), fill=True)
    dots = lambda g: sum(  # noqa: E731
        bin(ord(ch) - 0x2800).count("1") for row in g.rows(20, 4) for ch in row
    )
    assert dots(area) > dots(line) * 3

    # The widget must actually paint. Textual's Widget.BLANK means "render
    # nothing", so a constant of that name on the subclass silently blanks it
    # before render() is ever called — which is how this shipped broken once.
    assert BrailleGraph.BLANK is False

    # Degenerate input must not raise or divide by zero.
    assert BrailleGraph([]).rows(10, 2) == [BrailleGraph.EMPTY * 10] * 2
    assert BrailleGraph([5.0, 5.0, 5.0]).rows(10, 2)
    assert BrailleGraph([1.0]).rows(0, 0) == []


# --- dialogs -------------------------------------------------------------------


def _assert_dialog(screen, size: tuple[int, int], width: int) -> None:
    """A dialog is framed, dimmed, centred and inside the terminal.

    Checked against the laid-out regions rather than the widget tree: a green
    tree has twice hidden a box that never painted a column.
    """
    frame = screen.query_one("#dialog-frame")
    region = frame.region
    assert region.width == width, (region, width)
    assert region.height > 0
    # Framed: a border on every edge, not a bare box of text.
    edges = screen.query_one("#dialog-frame").styles.border
    assert all(edge[0] == "solid" for edge in edges), edges
    # Dimmed backdrop: the modal's own background is translucent.
    assert screen.styles.background.a < 1.0
    # Centred, and clipping nothing at either size.
    w, h = size
    assert region.x > 0 and region.right <= w
    assert abs(region.x - (w - region.right)) <= 1, (region, w)
    assert region.y >= 0 and region.bottom <= h, (region, h)
    # The key hint is the last row inside the frame and the first thing a
    # too-tall dialog loses: it slid under the bottom border once, with the
    # frame itself still measuring as unclipped.
    hint = screen.query_one("#dialog-hint")
    assert frame.content_region.contains_region(hint.region), (hint.region, frame.content_region)


@pytest.mark.parametrize(
    ("key", "width"),
    [("m", MODAL_WIDTH_WIDE), ("p", MODAL_WIDTH), ("g", MODAL_WIDTH), ("?", MODAL_WIDTH_WIDE)],
)
@pytest.mark.parametrize("size", [(80, 24), (120, 40)])
def test_modals_are_framed_centred_and_escapable(delta, key, width, size):
    """Every modal is a Dialog: framed, dimmed, centred, and closed by escape."""

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=size) as pilot:
            await pilot.press(key)
            await pilot.pause()
            modal = app.screen
            assert isinstance(modal, Dialog), modal
            _assert_dialog(modal, size, width)
            # The dialog owns escape: one press and the app is back home.
            await pilot.press("escape")
            await pilot.pause()
            assert app.screen.name == "home"

    asyncio.run(run())


def test_provider_key_modals_are_dialogs(delta):
    """The two provider forms are Dialogs too, framed and closed by escape.

    Pushed onto the real app, not a bare one: the app stylesheet is what makes
    an Input one row rather than Textual's three, and these forms hold three
    of them.
    """
    from delta.llm.providers import PROVIDERS
    from delta.tui.screens.provider_picker import CustomFormModal, KeyEntryModal

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(80, 24)) as pilot:
            for modal in (KeyEntryModal(PROVIDERS["openai"]), CustomFormModal()):
                app.push_screen(modal)
                await pilot.pause()
                assert isinstance(app.screen, Dialog)
                _assert_dialog(app.screen, (80, 24), MODAL_WIDTH)
                await pilot.press("escape")
                await pilot.pause()
                assert app.screen is not modal

    asyncio.run(run())


def test_help_lists_per_screen_keys(delta):
    """The keymap covers the screens, not just the app-level bindings."""
    from delta.tui.screens.research import Research
    from delta.tui.screens.theses import Theses
    from delta.tui.widgets import KeyGrid

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("?")
            await pilot.pause()
            help_screen = app.screen
            await pilot.press("k")
            await pilot.pause()
            assert help_screen.query_one("#help-tabs").active == "help-keys"
            groups = [str(s.render()) for s in help_screen.query(".help-group")]
            assert "Anywhere" in groups and "Theses" in groups
            # A key only Theses binds must be on screen, and the grid painted.
            keys = {
                str(hint.render())
                for grid in help_screen.query(KeyGrid)
                for hint in grid.query(KeyHint)
            }
            thesis_keys = {binding_key(b) for b in shown_bindings(Theses.BINDINGS)}
            assert thesis_keys and thesis_keys <= keys, thesis_keys - keys
            # Data is a thin subclass of Research: its keymap is
            # inherited, so reading the class dict alone left it out entirely.
            research_keys = {binding_key(b) for b in shown_bindings(Research.BINDINGS)}
            assert research_keys <= keys, research_keys - keys
            # …and it is listed once, not once per alias screen.
            assert len(groups) == len(set(groups))
            assert all(grid.region.height > 0 for grid in help_screen.query(KeyGrid))
            # t switches back to the tour.
            await pilot.press("t")
            await pilot.pause()
            assert help_screen.query_one("#help-tabs").active == "help-tour"

    asyncio.run(run())


def test_space_folds_and_reopens_an_asset_class_group(delta, monkeypatch, tmp_path):
    """``space`` on a group header must work, and must be able to undo itself.

    The header is not a target row, so the key cannot go through ``_selected``;
    and the cursor has to be put back on the header after the rebuild, or a
    collapsed group could never be reopened.
    """
    import tomli_w

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {"targets": {"apple": {"kind": "company", "market": "us", "tickers": ["AAPL"]}}}
        ),
        encoding="utf-8",
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2")
            screen = app.screen
            table = screen.query_one("#target-table")
            table.highlighted = table.get_option_index("group:equity")
            await pilot.pause()
            expanded = table.option_count

            await pilot.press("space")
            await pilot.pause()
            assert screen._collapsed_groups == {"group:equity"}
            assert table.option_count < expanded
            assert table.highlighted == table.get_option_index("group:equity")

            await pilot.press("space")
            await pilot.pause()
            assert screen._collapsed_groups == set()
            assert table.option_count == expanded

    asyncio.run(run())


# --- watchlist inspector chart ---------------------------------------------------


def _stub_fetch(monkeypatch, metric) -> None:
    """Pin the inspector's metrics fetch to a canned :class:`AssetMetrics`."""

    from delta import tui

    def fetch(instrument, range_name="month", engine=None, suffixes=None):
        return metric

    monkeypatch.setattr(tui.screens.targets, "fetch_asset_metrics", fetch)


async def _await_metric(pilot, screen) -> None:
    """The fetch runs on a worker over ``to_thread``; wait for it to land."""
    for _ in range(40):
        instrument = screen._selected_instrument
        if instrument and (instrument.id, screen._range) in screen._metrics:
            return
        await pilot.pause(0.05)
    raise AssertionError("metrics never rendered")


def _series_with_times(count=40, first_day=10):
    """A rising daily series with parallel ISO stamps ending 2026-09-18."""
    from datetime import UTC, datetime, timedelta

    start = datetime(2026, 8, first_day, tzinfo=UTC)
    series = [100.0 + float(i) for i in range(count)]
    times = [(start + timedelta(days=i)).isoformat() for i in range(count)]
    return series, times


def test_targets_inspector_paints_price_chart_with_axes(delta, monkeypatch, tmp_path):
    """The inspector chart is a PriceChart fed the windowed series and its times."""
    from delta.asset_metrics import AssetMetrics
    from delta.tui.widgets import PriceChart

    _home_config(tmp_path, monkeypatch, delta)
    series, times = _series_with_times()
    _stub_fetch(
        monkeypatch,
        AssetMetrics(
            "US:AAPL",
            "equity",
            values={"Current price": "139.00"},
            series=series,
            series_times=times,
            change_label="+39.0%",
            history_start=times[0],
            history_end=times[-1],
            period_high=139.0,
            period_low=100.0,
        ),
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2", "enter")
            screen = app.screen
            chart = screen.query_one("#target-chart")
            assert isinstance(chart, PriceChart)
            await _await_metric(pilot, screen)
            assert chart.data == series[-30:]
            assert chart.times == times[-30:]
            assert chart.y_format(1234.5) == "1,234.50"
            frame = _frame(app)
            # The first windowed date is an X tick label; 120 is a Y gutter tick.
            assert "20 Aug" in frame
            assert "120.00" in frame

    asyncio.run(run())


def test_targets_inspector_header_names_currency_and_range(delta, monkeypatch, tmp_path):
    """The header is ``price · range · CURRENCY``, and ``r`` rewrites both parts."""
    from delta.asset_metrics import AssetMetrics

    _home_config(tmp_path, monkeypatch, delta)
    series, times = _series_with_times()
    _stub_fetch(
        monkeypatch,
        AssetMetrics(
            "US:AAPL",
            "equity",
            values={"Current price": "139.00"},
            series=series,
            series_times=times,
        ),
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2", "enter")
            screen = app.screen
            await _await_metric(pilot, screen)
            label = screen.query_one("#target-chart-label")
            assert str(label.render()) == "price · month · USD"
            await pilot.press("r")
            assert str(label.render()) == "price · all time · USD"
            await _await_metric(pilot, screen)
            chart = screen.query_one("#target-chart")
            assert chart.data == series
            assert chart.times == times

    asyncio.run(run())


def test_targets_inspector_bond_chart_labels_yield(delta, monkeypatch, tmp_path):
    """A bond plots yield: the header says so and Y labels skip the separator."""
    import tomli_w

    from delta.asset_metrics import AssetMetrics

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        tomli_w.dumps(
            {
                "targets": {
                    "letters": {
                        "kind": "company",
                        "market": "us",
                        "tickers": ["TLT"],
                        "asset_class": "bond",
                    }
                }
            }
        ),
        encoding="utf-8",
    )
    series, times = _series_with_times(count=30)
    _stub_fetch(
        monkeypatch, AssetMetrics("US:TLT", "bond", values={"Current yield": "4.35"}, series=series)
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2", "enter")
            screen = app.screen
            await _await_metric(pilot, screen)
            assert str(screen.query_one("#target-chart-label").render()) == "yield · month"
            chart = screen.query_one("#target-chart")
            assert chart.y_format(1234.5) == "1234.50"

    asyncio.run(run())


def test_targets_inspector_empty_metrics_leave_a_blank_chart(delta, monkeypatch, tmp_path):
    """Metrics with no series clear the chart instead of crashing the pane."""
    from delta.asset_metrics import AssetMetrics

    _home_config(tmp_path, monkeypatch, delta)
    _stub_fetch(monkeypatch, AssetMetrics("US:AAPL", "equity"))

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2", "enter")
            screen = app.screen
            await _await_metric(pilot, screen)
            chart = screen.query_one("#target-chart")
            assert chart.data == []
            assert chart.times == []
            assert app._exception is None
            _frame(app)

    asyncio.run(run())


def test_targets_inspector_without_selection_resets_the_header(delta, monkeypatch, tmp_path):
    """No targets: the bare chart stays and the header keeps its default label."""
    from delta.tui.widgets import PriceChart

    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("", encoding="utf-8")

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2")
            screen = app.screen
            chart = screen.query_one("#target-chart")
            assert isinstance(chart, PriceChart)
            assert chart.data == []
            assert chart.times == []
            assert str(screen.query_one("#target-chart-label").render()) == "price · month"
            assert "no targets yet" in str(screen.query_one("#target-inspector-empty").render())

    asyncio.run(run())


def test_targets_inspector_builds_eight_metric_cards(delta, monkeypatch, tmp_path):
    """The pane carries eight group cards, one per equity group."""
    _home_config(tmp_path, monkeypatch, delta)

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2")
            screen = app.screen
            assert screen.query_one("#metric-card-7")
            assert not screen.query("#metric-card-8")

    asyncio.run(run())


def test_targets_inspector_caches_metrics_per_range(delta, monkeypatch, tmp_path):
    """``r`` must not evict what the provider already answered for other ranges."""
    from delta.asset_metrics import AssetMetrics

    _home_config(tmp_path, monkeypatch, delta)
    series, times = _series_with_times()
    _stub_fetch(
        monkeypatch,
        AssetMetrics(
            "US:AAPL",
            "equity",
            values={"Current price": "139.00"},
            series=series,
            series_times=times,
        ),
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2", "enter")
            screen = app.screen
            await _await_metric(pilot, screen)
            assert set(screen._metrics) == {("US:AAPL", "month")}
            await pilot.press("r")
            await _await_metric(pilot, screen)
            await pilot.press("r")
            await _await_metric(pilot, screen)
            assert set(screen._metrics) == {
                ("US:AAPL", "month"),
                ("US:AAPL", "all"),
                ("US:AAPL", "day"),
            }

    asyncio.run(run())


def test_targets_inspector_glossary_opens_and_closes(delta, monkeypatch, tmp_path):
    """``i`` opens the glossary for the selected profile; escape returns."""
    from delta.asset_metrics import AssetMetrics
    from delta.tui.screens.targets import MetricHelpModal

    _home_config(tmp_path, monkeypatch, delta)
    series, times = _series_with_times()
    _stub_fetch(
        monkeypatch,
        AssetMetrics(
            "US:AAPL",
            "equity",
            values={"Current price": "139.00"},
            series=series,
            series_times=times,
        ),
    )

    async def run():
        app = DeltaApp(delta)
        async with app.run_test(size=(120, 40)) as pilot:
            await pilot.press("2", "enter")
            screen = app.screen
            await _await_metric(pilot, screen)
            await pilot.press("i")
            assert isinstance(app.screen, MetricHelpModal)
            frame = _frame(app)
            assert "what these metrics mean" in frame
            assert "How fast sales grew in the most recent year." in frame
            await pilot.press("escape")
            assert app.screen is screen

    asyncio.run(run())
