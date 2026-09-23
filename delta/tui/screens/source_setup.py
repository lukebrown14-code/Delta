"""Setup dialog for data plugins that declare a :class:`DataProviderSpec`."""

from __future__ import annotations

from collections.abc import Callable
from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.widgets import Button, Input, Label, Static

from delta.core.plugin import DataProviderSpec
from delta.tui.widgets import Dialog, hint_markup


class SourceSetupModal(Dialog):
    """Collect only the explicitly declared non-secret settings for a source."""

    dialog_title = "configure source"
    dialog_hint = hint_markup(("enter", "save"), ("esc", "cancel"))

    DEFAULT_CSS = """
    SourceSetupModal #source-notice { height: auto; margin: 0 0 1 0; }
    SourceSetupModal .form-row { height: 1; margin: 1 0 0 0; }
    SourceSetupModal .form-row Label { width: 14; padding: 1 1 0 0; color: $text-muted; }
    SourceSetupModal .form-row Input { width: 1fr; margin: 0; }
    SourceSetupModal #source-actions { height: 1; margin-top: 1; }
    SourceSetupModal #source-actions Button {
        height: 1; min-width: 0; border: none; padding: 0 1; margin: 0 1 0 0;
    }
    """

    def __init__(self, spec: DataProviderSpec, current: dict[str, Any]) -> None:
        super().__init__()
        self.spec = spec
        self.current = current
        self.dialog_title = f"configure {spec.label}"

    def compose_dialog(self) -> ComposeResult:
        if self.spec.notice:
            yield Static(self.spec.notice, id="source-notice", markup=False)
        for field in self.spec.fields:
            yield Horizontal(
                Label(field.label),
                Input(
                    value=str(self.current.get(field.name, "")),
                    placeholder=field.placeholder or field.label,
                    password=field.secret,
                    id=f"source-{field.name}",
                ),
                classes="form-row",
            )
        scope = self.current.get("scope", {})
        markets = ",".join(scope.get("markets", [])) if isinstance(scope, dict) else ""
        yield Horizontal(
            Label("markets"),
            Input(value=markets, placeholder="lse,asx (optional)", id="source-markets"),
            classes="form-row",
        )
        yield Horizontal(
            Button("Save", id="source-save", variant="primary"),
            Button("Cancel", id="source-cancel"),
            id="source-actions",
        )

    def on_mount(self) -> None:
        inputs = self.query(Input)
        if inputs:
            inputs.first().focus()

    def on_input_submitted(self, event: Input.Submitted) -> None:
        if event.input.id == "source-markets":
            self._save()

    def on_button_pressed(self, event: Button.Pressed) -> None:
        if event.button.id == "source-save":
            self._save()
        else:
            self.dismiss(None)

    def _save(self) -> None:
        values = {
            field.name: self.query_one(f"#source-{field.name}", Input).value
            for field in self.spec.fields
        }
        missing = [field.label for field in self.spec.fields if field.required and not values[field.name].strip()]
        if missing:
            self.notify(f"required: {', '.join(missing)}", severity="warning")
            return
        markets = [
            market.strip().lower()
            for market in self.query_one("#source-markets", Input).value.split(",")
            if market.strip()
        ]
        self.dismiss((values, markets))


async def configure_source(app: Any, delta: Any, name: str, refresh: Callable[[], Any]) -> None:
    """Show one adapter's setup form, save it, then refresh Settings."""
    from delta import services

    plugin = delta.plugins.get(name)
    spec = getattr(plugin, "provider_spec", None)
    if spec is None:
        app.notify(f"{name} has no setup form", severity="warning")
        return
    current = dict(getattr(delta.cfg, "plugins", {}).get(name, {}))
    result = await app.push_screen_wait(SourceSetupModal(spec, current))
    if result is None:
        return
    values, markets = result
    try:
        services.configure_data_provider(delta, name, values, markets=markets)
    except ValueError as exc:
        app.notify(str(exc), severity="error")
        return
    result = refresh()
    if hasattr(result, "__await__"):
        await result
    app.notify(f"{spec.label} configured")