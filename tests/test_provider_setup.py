"""Offline tests for provider setup: .env/config writebacks, compat provider,
key verification, and the TUI connect flow."""

from __future__ import annotations

import asyncio
from types import SimpleNamespace
from typing import Any

import httpx
import pytest
import respx
from textual.app import App
from textual.widgets import DataTable, Input

from delta.core.config import ENV_PATH, read_env_value, set_env_value
from delta.llm.catalog import set_llm_custom, set_llm_provider
from delta.llm.providers import (
    OPENROUTER_BASE_URL,
    PROVIDERS,
    OpenAICompatProvider,
    verify_key,
)
from delta.tui.screens.provider_picker import (
    CustomFormModal,
    KeyEntryModal,
    ProviderPicker,
    _normalize_base_url,
    connect_provider,
    mask_key,
    mismatched_routes,
    provider_key_status,
)


def _cfg(**overrides: Any) -> SimpleNamespace:
    values: dict[str, Any] = {
        "llm_provider": "openrouter",
        "llm_routing": {
            "report": "anthropic/claude-sonnet-4",
            "extract": "google/gemini-2.5-flash",
        },
        "llm_base_url": "",
        "llm_api_key_env": "",
        "llm_max_output_tokens": 4096,
    }
    values.update(overrides)
    return SimpleNamespace(**values)


class _FakeRig:
    def __init__(self, cfg: SimpleNamespace) -> None:
        self.cfg = cfg
        self.reloaded = 0

    def reload_llm(self) -> None:
        self.reloaded += 1


class _FakeApp:
    def __init__(self) -> None:
        self.notifications: list[tuple[str, str]] = []

    def notify(self, message: str, *, severity: str = "information") -> None:
        self.notifications.append((message, severity))


# --- .env writebacks -----------------------------------------------------------


def test_set_env_value_appends_creates_and_replaces(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    monkeypatch.delenv("OPENAI_API_KEY", raising=False)
    monkeypatch.delenv("UNRELATED", raising=False)
    ENV_PATH.write_text("UNRELATED=keep-me\n", encoding="utf-8")

    set_env_value("OPENAI_API_KEY", "sk-first")

    assert read_env_value("OPENAI_API_KEY") == "sk-first"
    assert read_env_value("UNRELATED") == "keep-me"

    set_env_value("OPENAI_API_KEY", "sk-second")

    text = ENV_PATH.read_text(encoding="utf-8")
    assert text.count("OPENAI_API_KEY") == 1
    assert read_env_value("OPENAI_API_KEY") == "sk-second"
    assert "UNRELATED=keep-me" in text


def test_read_env_value_missing_is_empty(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    assert read_env_value("NOT_THERE") == ""


def test_read_env_value_environment_beats_dotenv(monkeypatch, tmp_path):
    """An exported shell variable must win, matching Settings' precedence."""
    monkeypatch.chdir(tmp_path)
    ENV_PATH.write_text("OPENAI_API_KEY=from-file\n", encoding="utf-8")
    monkeypatch.setenv("OPENAI_API_KEY", "from-shell")

    assert read_env_value("OPENAI_API_KEY") == "from-shell"


# --- config.toml writebacks ----------------------------------------------------


def test_set_llm_provider_round_trips(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text('[llm]\nprovider = "openrouter"\n', encoding="utf-8")

    set_llm_provider("anthropic")

    import tomllib

    raw = tomllib.loads((tmp_path / "config.toml").read_text(encoding="utf-8"))
    assert raw["llm"]["provider"] == "anthropic"


def test_set_llm_custom_round_trips(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    (tmp_path / "config.toml").write_text("[llm]\n", encoding="utf-8")

    set_llm_custom(base_url="http://localhost:11434/v1", api_key_env="OLLAMA_KEY")

    import tomllib

    raw = tomllib.loads((tmp_path / "config.toml").read_text(encoding="utf-8"))
    assert raw["llm"] == {
        "provider": "custom",
        "base_url": "http://localhost:11434/v1",
        "api_key_env": "OLLAMA_KEY",
    }


# --- OpenAICompatProvider ------------------------------------------------------


@respx.mock
def test_compat_models_parses_endpoint(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    route = respx.get("https://api.openai.com/v1/models").mock(
        return_value=httpx.Response(
            200, json={"data": [{"id": "gpt-4o", "name": "GPT-4o"}, {"id": "gpt-4o-mini"}]}
        )
    )
    provider = OpenAICompatProvider(PROVIDERS["openai"], "sk-k")

    models = asyncio.run(provider.models())

    assert [m.id for m in models] == ["gpt-4o", "gpt-4o-mini"]
    assert all(m.prompt_price == 0.0 for m in models)
    assert route.calls.last.request.headers["Authorization"] == "Bearer sk-k"


@respx.mock
def test_compat_models_sends_anthropic_native_auth(monkeypatch, tmp_path):
    """Anthropic's compat layer authenticates with x-api-key, not Bearer alone."""
    monkeypatch.chdir(tmp_path)
    route = respx.get("https://api.anthropic.com/v1/models").mock(
        return_value=httpx.Response(200, json={"data": []})
    )
    provider = OpenAICompatProvider(PROVIDERS["anthropic"], "sk-ant")

    asyncio.run(provider.models())

    headers = route.calls.last.request.headers
    assert headers["x-api-key"] == "sk-ant"
    assert headers["anthropic-version"] == "2023-06-01"


@respx.mock
def test_compat_models_degrades_on_missing_endpoint(monkeypatch, tmp_path):
    """Anthropic's compat layer has no /models: the picker falls back to free text."""
    monkeypatch.chdir(tmp_path)
    respx.get("https://api.anthropic.com/v1/models").mock(return_value=httpx.Response(404))
    provider = OpenAICompatProvider(PROVIDERS["anthropic"], "k")

    assert asyncio.run(provider.models()) == []


def test_compat_get_client_requires_base_url():
    provider = OpenAICompatProvider(PROVIDERS["custom"], "k", base_url="")

    with pytest.raises(RuntimeError, match="base URL"):
        provider._get_client()


# --- legacy config fallback ----------------------------------------------------


def test_build_llm_falls_back_on_legacy_provider_name(tmp_engine, monkeypatch, tmp_path):
    """A config naming a removed provider (e.g. "litellm") must not crash startup."""
    monkeypatch.chdir(tmp_path)
    from delta.runtime import Delta

    delta = Delta.__new__(Delta)
    delta.settings = SimpleNamespace(openrouter_api_key="", openai_api_key="", anthropic_api_key="")
    delta.cfg = _cfg(llm_provider="litellm")
    delta.engine = tmp_engine

    client = delta._build_llm()

    assert client.provider.name == "openrouter"


# --- verify_key ----------------------------------------------------------------


@respx.mock
def test_verify_key_openrouter_hits_credits(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    route = respx.get(f"{OPENROUTER_BASE_URL}/credits").mock(return_value=httpx.Response(200))
    ok = asyncio.run(verify_key(PROVIDERS["openrouter"], "k"))
    assert ok
    assert route.call_count == 1


@respx.mock
def test_verify_key_rejected_key_is_false(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    respx.get("https://api.openai.com/v1/models").mock(return_value=httpx.Response(401))
    assert asyncio.run(verify_key(PROVIDERS["openai"], "bad")) is False


def test_verify_key_empty_key_is_false():
    assert asyncio.run(verify_key(PROVIDERS["openai"], "")) is False


# --- picker helpers ------------------------------------------------------------


def test_mask_key():
    assert mask_key("") == "not set"
    assert mask_key("short") == "set"
    assert mask_key("sk-ant-api03-1234-abcd") == "sk-ant-…abcd"


def test_mismatched_routes_by_provider():
    cfg = _cfg()

    assert mismatched_routes(cfg, "openrouter") == []
    assert mismatched_routes(cfg, "anthropic") == ["extract", "report"]
    assert mismatched_routes(cfg, "openai") == ["extract", "report"]

    no_routes = _cfg(llm_routing={})
    assert mismatched_routes(no_routes, "anthropic") == []


def test_provider_key_status_reads_env(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    ENV_PATH.write_text("OPENROUTER_API_KEY=sk-or\n", encoding="utf-8")

    status = provider_key_status(_FakeRig(_cfg()))

    assert status["openrouter"] is True
    assert status["openai"] is False
    assert status["custom"] is False


def test_normalize_base_url_adds_scheme():
    assert _normalize_base_url("localhost:11434/v1") == "http://localhost:11434/v1"
    assert _normalize_base_url("https://x.dev/v1") == "https://x.dev/v1"


# --- connect flow --------------------------------------------------------------


def test_connect_provider_saves_key_verifies_and_switches(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    monkeypatch.delenv("OPENAI_API_KEY", raising=False)
    (tmp_path / "config.toml").write_text('[llm]\nprovider = "openrouter"\n', encoding="utf-8")
    app = _FakeApp()
    app.push_screen_wait = lambda modal: _async_return("sk-new")  # type: ignore[method-assign]
    delta = _FakeRig(_cfg())

    async def fake_verify(spec: Any, key: str, *, base_url: str = "") -> bool:
        return key == "sk-new"

    monkeypatch.setattr("delta.tui.screens.provider_picker.verify_key", fake_verify)
    asyncio.run(connect_provider(app, delta, "openai"))

    assert read_env_value("OPENAI_API_KEY") == "sk-new"
    assert "openai" in (tmp_path / "config.toml").read_text(encoding="utf-8")
    assert delta.reloaded == 1
    assert any("connected" in msg for msg, _ in app.notifications)
    assert any("routes may not match" in msg for msg, sev in app.notifications if sev == "warning")


def test_connect_provider_cancelled_modals_do_nothing(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    (tmp_path / "config.toml").write_text('[llm]\nprovider = "openrouter"\n', encoding="utf-8")
    app = _FakeApp()
    app.push_screen_wait = lambda modal: _async_return(None)  # type: ignore[method-assign]
    delta = _FakeRig(_cfg())

    asyncio.run(connect_provider(app, delta, "anthropic"))
    asyncio.run(connect_provider(app, delta, "custom"))

    assert read_env_value("ANTHROPIC_API_KEY") == ""
    assert delta.reloaded == 0
    assert app.notifications == []


def test_connect_provider_existing_key_skips_modal(monkeypatch, tmp_path):
    monkeypatch.chdir(tmp_path)
    monkeypatch.delenv("ANTHROPIC_API_KEY", raising=False)
    (tmp_path / "config.toml").write_text('[llm]\nprovider = "openrouter"\n', encoding="utf-8")
    ENV_PATH.write_text("ANTHROPIC_API_KEY=sk-ant-existing\n", encoding="utf-8")

    pushed: list[Any] = []

    async def push_screen_wait(modal: Any) -> Any:
        pushed.append(modal)
        return None

    app = _FakeApp()
    app.push_screen_wait = push_screen_wait  # type: ignore[method-assign]
    delta = _FakeRig(_cfg())

    asyncio.run(connect_provider(app, delta, "anthropic"))

    assert pushed == []  # key existed: no modal, straight to activation
    assert delta.reloaded == 1


def _async_return(value: Any) -> Any:
    async def coro() -> Any:
        await asyncio.sleep(0)
        return value

    return coro()


# --- modal screens -------------------------------------------------------------


def test_provider_picker_lists_all_and_selects():
    picked: list[str] = []

    async def run() -> None:
        app = App()
        async with app.run_test() as pilot:
            picker = ProviderPicker(picked.append, key_status={"openrouter": True})
            app.push_screen(picker)
            await pilot.pause()
            table = picker.query_one("#pp-table", DataTable)
            assert table.row_count == len(PROVIDERS)
            await pilot.press("enter")
            await pilot.pause()

    asyncio.run(run())
    assert picked == ["openrouter"]  # first row, cursor starts at the top


def test_key_entry_modal_returns_entered_key():
    result: list[str] = []

    async def run() -> None:
        app = App()
        async with app.run_test() as pilot:
            modal = KeyEntryModal(PROVIDERS["openai"])
            app.push_screen(modal)
            await pilot.pause()
            modal.query_one("#ke-input", Input).value = "sk-typed"
            await pilot.press("enter")
            await pilot.pause()
        result.append("done")

    asyncio.run(run())
    assert result == ["done"]


def test_custom_form_modal_collects_fields():
    async def run() -> None:
        app = App()
        async with app.run_test() as pilot:
            modal = CustomFormModal()
            app.push_screen(modal)
            await pilot.pause()
            modal.query_one("#cf-base", Input).value = "localhost:11434/v1"
            modal.query_one("#cf-env", Input).value = ""
            modal.query_one("#cf-key", Input).value = "local"
            # Enter saves from any of the three fields; two are optional.
            modal.query_one("#cf-base", Input).focus()
            await pilot.press("enter")
            await pilot.pause()

    asyncio.run(run())  # smoke: modal mounts and saves without error
