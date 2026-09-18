"""Provider picker modals: connect or switch the LLM provider without files.

Pressing ``p`` opens :class:`ProviderPicker`; choosing a provider collects
its key (or the custom endpoint form), verifies the key with one live call,
writes it to .env, switches ``[llm] provider``, and hot-reloads the runtime —
the user never edits a file or restarts the app.
"""

from __future__ import annotations

from collections.abc import Callable
from typing import Any
from urllib.parse import urlparse

from textual.app import ComposeResult
from textual.widgets import DataTable, Input, Static

from delta.core.config import read_env_value, set_env_value
from delta.llm.catalog import set_llm_custom, set_llm_provider
from delta.llm.providers import PROVIDERS, ProviderSpec, verify_key
from delta.tui.widgets import DeltaTable, Dialog, hint_markup


def provider_key_status(delta: Any) -> dict[str, bool]:
    """Which providers are already usable: key present (and base_url for custom)."""
    status: dict[str, bool] = {}
    for name, spec in PROVIDERS.items():
        if name == "custom":
            env = getattr(delta.cfg, "llm_api_key_env", "") or spec.env_var
            status[name] = bool(read_env_value(env)) and bool(getattr(delta.cfg, "llm_base_url", ""))
        else:
            status[name] = bool(read_env_value(spec.env_var))
    return status


def mask_key(key: str) -> str:
    """A key safe to display: prefix, ellipsis, last four characters."""
    if not key:
        return "not set"
    if len(key) < 12:
        return "set"
    return f"{key[:7]}…{key[-4:]}"


def mismatched_routes(cfg: Any, provider: str) -> list[str]:
    """Route tasks whose model id looks wrong for ``provider`` (soft warning)."""
    routing: dict[str, str] = getattr(cfg, "llm_routing", {}) or {}

    def fits(model: str) -> bool:
        if provider == "openrouter":
            return "/" in model
        if provider == "openai":
            return "/" not in model or model.startswith("openai/")
        if provider == "anthropic":
            return model.startswith("claude")
        return True  # custom: vendor unknown, no heuristic

    return [task for task, model in sorted(routing.items()) if not fits(model)]


def _normalize_base_url(base_url: str) -> str:
    if "://" not in base_url:
        return f"http://{base_url}"
    return base_url


class ProviderPicker(Dialog):
    """Browse providers with their key status; enter connects, escape cancels."""

    dialog_title = "select a provider"
    dialog_hint = hint_markup(("enter", "connect"), ("esc", "cancel"))

    DEFAULT_CSS = """
    ProviderPicker #pp-table {
        height: auto;
        max-height: 12;
    }
    ProviderPicker #pp-status {
        height: 1;
        margin: 1 0 0 0;
        color: $text-muted;
    }
    """

    def __init__(self, on_select: Callable[[str], None], *, key_status: dict[str, bool]) -> None:
        super().__init__()
        self.on_select = on_select
        self.key_status = key_status

    def compose_dialog(self) -> ComposeResult:
        yield DeltaTable(id="pp-table")
        yield Static("keys are stored in .env, never in config.toml", id="pp-status", markup=False)

    def on_mount(self) -> None:
        table = self.query_one("#pp-table", DataTable)
        table.add_columns("Provider", "Endpoint", "Key")
        for name, spec in PROVIDERS.items():
            host = urlparse(spec.base_url).hostname or (
                "from config.toml" if name == "custom" else "—"
            )
            if name == "custom":
                key = "set" if self.key_status.get(name) else "base_url + key"
            else:
                key = "set" if self.key_status.get(name) else "not set"
            table.add_row(name, host or "—", key, key=name)

    def on_data_table_row_selected(self, event: DataTable.RowSelected) -> None:
        if event.row_key.value:
            self._select(event.row_key.value)

    def _select(self, name: str) -> None:
        self.dismiss(None)
        self.on_select(name)


class KeyEntryModal(Dialog):
    """Masked input for one provider's API key; dismisses with the key or None."""

    dialog_hint = hint_markup(("enter", "save"), ("esc", "skip"))

    DEFAULT_CSS = """
    KeyEntryModal #ke-hint {
        height: auto;
        margin: 0 0 1 0;
        color: $text-muted;
    }
    """

    def __init__(self, spec: ProviderSpec, *, existing: str = "") -> None:
        super().__init__()
        self.spec = spec
        self.existing = existing
        self.dialog_title = f"enter {spec.env_var}"

    def compose_dialog(self) -> ComposeResult:
        hint = (
            f"existing: {mask_key(self.existing)} · enter a new key to overwrite"
            if self.existing
            else f"get one at {self.spec.name}.com — stored in .env, never config.toml"
        )
        yield Static(hint, id="ke-hint", markup=False)
        yield Input(placeholder="api key", password=True, id="ke-input")

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "ke-input":
            self._save(event.value.strip())

    def _save(self, value: str) -> None:
        if not value:
            self.notify("enter a key first (or skip to cancel)", severity="warning")
            return
        self.dismiss(value)


class CustomFormModal(Dialog):
    """Three-field form for a custom OpenAI-compatible endpoint.

    Dismisses with ``(base_url, api_key_env, key)`` or None; the key is
    optional for local servers like Ollama.
    """

    dialog_title = "connect a custom endpoint"
    dialog_hint = hint_markup(("enter", "save"), ("esc", "cancel"))

    DEFAULT_CSS = """
    CustomFormModal #cf-blurb {
        height: auto;
        margin: 0 0 1 0;
        color: $text-muted;
    }
    CustomFormModal Input {
        margin: 0 0 1 0;
    }
    """

    def compose_dialog(self) -> ComposeResult:
        yield Static(
            "any OpenAI-compatible server: Ollama, Groq, Together…",
            id="cf-blurb",
            markup=False,
        )
        yield Input(placeholder="base URL, e.g. http://localhost:11434/v1", id="cf-base")
        yield Input(placeholder="env var name for the key (default CUSTOM_API_KEY)", id="cf-env")
        yield Input(placeholder="api key (optional for local servers)", password=True, id="cf-key")

    def on_input_submitted(self, event: Input.Submitted) -> None:
        """Enter saves from any field: two of the three are optional."""
        self._save()

    def _save(self) -> None:
        base_url = self.query_one("#cf-base", Input).value.strip()
        if not base_url:
            self.notify("base URL is required", severity="warning")
            return
        env_var = self.query_one("#cf-env", Input).value.strip() or "CUSTOM_API_KEY"
        key = self.query_one("#cf-key", Input).value.strip()
        self.dismiss((base_url, env_var, key))


async def connect_provider(app: Any, delta: Any, name: str) -> None:
    """Connect and switch to ``name``: collect the key, verify, write, reload.

    A provider whose key is already set switches immediately; a missing key
    opens the entry modal first. Verification is one live GET — a rejected
    key is still saved so setup checks keep surfacing it.
    """
    spec = PROVIDERS.get(name)
    if spec is None:
        app.notify(f"unknown provider {name!r}", severity="error")
        return
    if name == "custom":
        result = await app.push_screen_wait(CustomFormModal())
        if result is None:
            return
        base_url, env_var, key = result
        base_url = _normalize_base_url(base_url)
        set_llm_custom(base_url=base_url, api_key_env=env_var)
        if key:
            set_env_value(env_var, key)
        if key and not await verify_key(spec, key, base_url=base_url):
            app.notify(f"key rejected by {name} — saved, unverified", severity="warning")
        else:
            app.notify(f"{name} connected at {base_url}")
        _activate(app, delta, name)
        return
    key = read_env_value(spec.env_var)
    if not key:
        entered = await app.push_screen_wait(KeyEntryModal(spec))
        if not entered:
            return
        key = entered
        set_env_value(spec.env_var, key)
        if not await verify_key(spec, key):
            app.notify(f"key rejected by {name} — saved, unverified", severity="warning")
            _activate(app, delta, name)
            return
        app.notify(f"{name} connected")
    _activate(app, delta, name)


def _activate(app: Any, delta: Any, name: str) -> None:
    """Write the provider switch, hot-reload, and warn on mismatched routes."""
    set_llm_provider(name)
    reload = getattr(delta, "reload_llm", None)
    if callable(reload):
        reload()
    bad = mismatched_routes(delta.cfg, name)
    if bad:
        app.notify(
            f"routes may not match {name}: {', '.join(bad)} — press m to re-pick",
            severity="warning",
        )
