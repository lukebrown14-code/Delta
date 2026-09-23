"""Offline tests for the model catalog, route writeback, and picker (HTTP via respx)."""

from __future__ import annotations

import asyncio
import json
import time

import httpx
import respx
from textual.app import App
from textual.widgets import DataTable, Input, OptionList

from delta.core.config import load_toml
from delta.llm.catalog import (
    CATALOG_PATH,
    ModelInfo,
    _read_cache_entry,
    _write_cache,
    cached_catalog,
    catalog,
    set_llm_model,
    set_llm_route,
    set_plugin_model,
)
from delta.llm.providers import (
    OPENROUTER_BASE_URL,
    OpenRouterProvider,
)
from delta.tui.screens.model_picker import ModelPicker

MODELS_URL = f"{OPENROUTER_BASE_URL}/models"

MODELS_PAYLOAD = {
    "data": [
        {
            "id": "anthropic/claude-sonnet-4",
            "name": "Claude Sonnet 4",
            "context_length": 200000,
            "pricing": {"prompt": "0.000003", "completion": "0.000015"},
        },
        {
            "id": "openai/gpt-4o",
            "name": "GPT-4o",
            "context_length": 128000,
            "pricing": {"prompt": "0.0000025", "completion": "0.00001"},
        },
        {
            "id": "meta/llama-3.1-8b",
            "name": "Llama 3.1 8B",
            "context_length": None,
            "pricing": {},
        },
    ]
}

SONNET = ModelInfo(
    id="anthropic/claude-sonnet-4",
    name="Claude Sonnet 4",
    context_length=200000,
    prompt_price=3e-06,
    completion_price=1.5e-05,
)
GPT = ModelInfo(
    id="openai/gpt-4o",
    name="GPT-4o",
    context_length=128000,
    prompt_price=2.5e-06,
    completion_price=1e-05,
)
LLAMA = ModelInfo(
    id="meta/llama-3.1-8b",
    name="Llama 3.1 8B",
    context_length=None,
    prompt_price=0.0,
    completion_price=0.0,
)


@respx.mock
def test_openrouter_models_builds_modelinfo(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    route = respx.get(MODELS_URL).mock(return_value=httpx.Response(200, json=MODELS_PAYLOAD))
    provider = OpenRouterProvider(api_key="k")

    models = asyncio.run(provider.models())

    assert route.call_count == 1
    assert models == [SONNET, GPT, LLAMA]

    again = asyncio.run(provider.models())
    assert again == models
    assert route.call_count == 1


@respx.mock
def test_openrouter_failed_fetch_yields_empty_and_does_not_raise(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get(MODELS_URL).mock(return_value=httpx.Response(500))
    provider = OpenRouterProvider(api_key="k")

    assert asyncio.run(provider.models()) == []
    assert asyncio.run(catalog(provider)) == []


@respx.mock
def test_catalog_populates_disk_cache_and_reuses_it(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    route = respx.get(MODELS_URL).mock(return_value=httpx.Response(200, json=MODELS_PAYLOAD))
    provider = OpenRouterProvider(api_key="k")
    before = time.time()

    models = asyncio.run(catalog(provider))

    assert models == [SONNET, GPT, LLAMA]
    raw = json.loads(CATALOG_PATH.read_text(encoding="utf-8"))
    assert set(raw) == {"openrouter"}
    entry = raw["openrouter"]
    assert before <= entry["fetched_at"] <= time.time()
    assert [m["id"] for m in entry["models"]] == [m.id for m in models]

    fresh_provider = OpenRouterProvider(api_key="k")
    assert asyncio.run(catalog(fresh_provider)) == models
    assert route.call_count == 1


def test_disk_cache_round_trips_with_fetched_at_stamp(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    before = time.time()
    models = [SONNET, GPT]

    _write_cache("openrouter", models)

    entry = _read_cache_entry("openrouter")
    assert entry is not None
    fetched_at, loaded = entry
    assert before <= fetched_at <= time.time()
    assert loaded == models
    assert cached_catalog("openrouter") == models
    assert cached_catalog("openai") == []
    assert _read_cache_entry("openai") is None


def test_set_llm_route_round_trips(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[llm.routing]\nextract = "old/model"\n', encoding="utf-8"
    )

    set_llm_route("analyse", "anthropic/claude-sonnet-4")

    routing = load_toml()["llm"]["routing"]
    assert routing == {"analyse": "anthropic/claude-sonnet-4", "extract": "old/model"}


def test_set_llm_model_round_trips_and_leaves_routing_alone(monkeypatch, tmp_path):
    """The one-model setting writes [llm] model and keeps legacy routing rows."""
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text(
        '[llm]\nprovider = "openrouter"\n\n[llm.routing]\nextract = "old/model"\n',
        encoding="utf-8",
    )

    set_llm_model("anthropic/claude-sonnet-4")

    llm = load_toml()["llm"]
    assert llm["model"] == "anthropic/claude-sonnet-4"
    assert llm["provider"] == "openrouter"
    assert llm["routing"] == {"extract": "old/model"}


def test_set_plugin_model_round_trips_and_none_removes(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("[plugins.sec_edgar]\nenabled = true\n", encoding="utf-8")

    set_plugin_model("yfinance", "openai/gpt-4o")
    plugins = load_toml()["plugins"]
    assert plugins["yfinance"]["model"] == "openai/gpt-4o"
    assert plugins["sec_edgar"] == {"enabled": True}

    set_plugin_model("yfinance", None)
    plugins = load_toml()["plugins"]
    assert "model" not in plugins["yfinance"]


def test_model_picker_filter_and_enter_selects(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    _write_cache("openrouter", [SONNET, GPT])
    picked: list[ModelInfo] = []

    async def run():
        app = App()
        async with app.run_test() as pilot:
            picker = ModelPicker(picked.append, provider_name="openrouter")
            app.push_screen(picker)
            await pilot.pause()
            assert picker.query_one("#mp-table", DataTable).row_count == 2
            picker.query_one("#mp-filter", Input).value = "sonnet"
            await pilot.pause()
            assert picker.query_one("#mp-table", DataTable).row_count == 1
            await pilot.press("enter")
            await pilot.pause()

    asyncio.run(run())
    assert picked == [SONNET]


def test_model_picker_autocomplete_browses_and_picks(monkeypatch, tmp_path):
    """Typing opens the dropdown, arrows browse it, enter adopts the highlight."""
    monkeypatch.chdir(tmp_path)
    _write_cache("openrouter", [SONNET, GPT, LLAMA])
    picked: list[ModelInfo] = []

    async def run():
        app = App()
        async with app.run_test() as pilot:
            picker = ModelPicker(picked.append, provider_name="openrouter")
            app.push_screen(picker)
            await pilot.pause()
            picker.query_one("#mp-filter", Input).focus()
            await pilot.press("4")  # hits sonnet-4 and gpt-4o, not llama
            await pilot.pause()
            suggestions = picker.query_one("#mp-suggestions", OptionList)
            assert suggestions.display
            assert [option.id for option in suggestions.options] == [SONNET.id, GPT.id]
            await pilot.press("down")
            await pilot.pause()
            await pilot.press("enter")
            await pilot.pause()

    asyncio.run(run())
    assert picked == [GPT]


def test_model_picker_escape_closes_suggestions_first(monkeypatch, tmp_path):
    """One escape shuts the dropdown; the dialog only leaves on the second."""
    monkeypatch.chdir(tmp_path)
    _write_cache("openrouter", [SONNET, GPT])
    picked: list[ModelInfo] = []

    async def run():
        app = App()
        async with app.run_test() as pilot:
            picker = ModelPicker(picked.append, provider_name="openrouter")
            app.push_screen(picker)
            await pilot.pause()
            picker.query_one("#mp-filter", Input).focus()
            picker.query_one("#mp-filter", Input).value = "sonnet"
            await pilot.pause()
            assert picker.query_one("#mp-suggestions", OptionList).display
            await pilot.press("escape")
            await pilot.pause()
            assert app.screen is picker  # still open…
            assert not picker.query_one("#mp-suggestions", OptionList).display  # …dropdown shut
            await pilot.press("escape")
            await pilot.pause()
            assert app.screen is not picker

    asyncio.run(run())
    assert picked == []


def test_model_picker_empty_catalog_degrades_to_free_text(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    picked: list[ModelInfo] = []

    async def run():
        app = App()
        async with app.run_test() as pilot:
            picker = ModelPicker(picked.append, provider_name="openrouter")
            app.push_screen(picker)
            await pilot.pause()
            picker.query_one("#mp-filter", Input).value = "my/model"
            await pilot.pause()
            await pilot.press("enter")
            await pilot.pause()

    asyncio.run(run())
    assert picked == [
        ModelInfo(
            id="my/model",
            name="my/model",
            context_length=None,
            prompt_price=0.0,
            completion_price=0.0,
        )
    ]


@respx.mock
def test_failed_fetch_keeps_previous_catalog(monkeypatch, tmp_path):
    """A transient error must not wipe a good cache for the whole TTL."""
    monkeypatch.chdir(tmp_path)
    _write_cache("openrouter", [SONNET, GPT])
    stamped = _read_cache_entry("openrouter")
    assert stamped is not None
    respx.get(MODELS_URL).mock(return_value=httpx.Response(500))
    provider = OpenRouterProvider(api_key="k")

    assert asyncio.run(catalog(provider, force=True)) == [SONNET, GPT]
    assert cached_catalog("openrouter") == [SONNET, GPT]
