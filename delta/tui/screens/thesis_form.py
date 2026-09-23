"""The thesis create/edit form.

Extracted from :mod:`delta.tui.screens.theses` so the screen and its widgets
each live in a file of manageable size (see the C9 simplification).
"""

from __future__ import annotations

from typing import Any

from textual.app import ComposeResult
from textual.containers import Horizontal
from textual.widgets import Button, Input, Select

from delta import theses
from delta.tui.widgets import Dialog, hint_markup


def _csv(value: str) -> tuple[str, ...]:
    """Split a comma-separated input into stripped, non-empty parts."""
    return tuple(part.strip() for part in value.split(",") if part.strip())


class ThesisForm(Dialog):
    """Create or edit a thesis: one form, seeded when editing.

    New and edit differ only by seeded values, the status field and the button
    label, so they are one class. Six framing fields at the app's default
    three-row Input would overflow a 24-row terminal and clip the submit
    button, so fields here are one row with a left marker for focus instead of
    a full border.
    """

    dialog_hint = hint_markup(("tab", "next field"), ("enter", "save"), ("esc", "cancel"))

    DEFAULT_CSS = """
    ThesisForm Input, ThesisForm Select {
        height: 1;
        border: none;
        border-left: thick $panel;
        background: $panel;
        padding: 0 1;
        margin: 0 0 1 0;
    }
    ThesisForm Input:focus, ThesisForm Select:focus {
        border-left: thick $primary;
    }
    ThesisForm SelectCurrent {
        border: none;
        padding: 0;
    }
    ThesisForm Button {
        height: 1;
        min-width: 0;
        border: none;
        padding: 0 1;
    }
    ThesisForm .form-row {
        height: auto;
    }
    ThesisForm .form-row Input, ThesisForm .form-row Select {
        width: 1fr;
    }
    """

    def __init__(
        self,
        thesis: theses.Thesis | None = None,
        *,
        claim: str = "",
        targets: str = "",
    ) -> None:
        super().__init__()
        self.thesis = thesis
        self.dialog_title = "edit thesis" if thesis else "new thesis"
        self._seed = {
            "claim": thesis.claim if thesis else claim,
            "targets": ", ".join(thesis.targets) if thesis else targets,
            "horizon": thesis.time_horizon if thesis else "",
            "scope": thesis.scope if thesis else "",
            "assumptions": ", ".join(thesis.assumptions) if thesis else "",
            "falsifiers": ", ".join(thesis.falsifiers) if thesis else "",
        }

    def compose_dialog(self) -> ComposeResult:
        yield Input(value=self._seed["claim"], placeholder="Claim", id="th-claim")
        with Horizontal(classes="form-row"):
            yield Input(
                value=self._seed["targets"],
                placeholder="Targets (US:AAPL)",
                id="th-targets",
            )
            yield Input(value=self._seed["horizon"], placeholder="Horizon (5y)", id="th-horizon")
        with Horizontal(classes="form-row"):
            yield Input(
                value=self._seed["scope"],
                placeholder="Scope (what the claim is about)",
                id="th-scope",
            )
            if self.thesis is not None:
                yield Select(
                    [(status.title(), status) for status in theses.STATUSES],
                    value=self.thesis.status,
                    allow_blank=False,
                    id="th-status",
                )
        yield Input(
            value=self._seed["assumptions"],
            placeholder="Holds if — assumptions, comma separated",
            id="th-assumptions",
        )
        yield Input(
            value=self._seed["falsifiers"],
            placeholder="Breaks if — what would disprove it",
            id="th-falsifiers",
        )
        yield Button(
            "Save changes" if self.thesis else "Create thesis",
            id="th-save",
            variant="primary",
        )

    def on_mount(self) -> None:
        self.query_one("#th-claim", Input).focus()

    def on_input_submitted(self) -> None:
        self.save()

    def on_button_pressed(self) -> None:
        self.save()

    def _value(self, field: str) -> str:
        return self.query_one(f"#th-{field}", Input).value.strip()

    def save(self) -> None:
        """Dismiss with the ``create_thesis``/``update_thesis`` keywords.

        A dict rather than a tuple: the two calls take different field sets, and
        a positional contract silently mis-binds when one of them gains a field.
        """
        claim = self._value("claim")
        if not claim:
            self.notify("claim is required", severity="error")
            return
        fields: dict[str, Any] = {
            "claim": claim,
            "targets": _csv(self.query_one("#th-targets", Input).value),
            "time_horizon": self._value("horizon"),
            "scope": self._value("scope"),
            "assumptions": _csv(self.query_one("#th-assumptions", Input).value),
            "falsifiers": _csv(self.query_one("#th-falsifiers", Input).value),
        }
        if self.thesis is not None:
            fields["status"] = str(self.query_one("#th-status", Select).value)
        self.dismiss(fields)
