"""App-shell scenarios: the modals and theme every pane shares.

States: the command palette open (``ctrl+p``), the help modal (``?``), the Go
picker (``g``) and Home in the light theme (``f2``). All start from the
landing Home screen; the modals float over it.
"""

from __future__ import annotations

from golden_scenarios import matrix

SCENARIOS = (
    matrix("palette-open", ("ctrl+p",), "A")
    + matrix("help-open", ("?",), "A")
    + matrix("go-open", ("g",), "A")
    + matrix("home-light", ("f2",), "A")
)
