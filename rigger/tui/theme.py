"""Rigger theme: dark "trading desk" palette with a cyan-blue accent.

``primary``, ``secondary`` and ``accent`` are deliberately the same blue —
the one the Home wordmark and sparklines already used — so the whole app
reads as one accent colour rather than an amber chrome around a blue
dashboard. Status meaning stays in ``success``/``warning``/``error``.

Token names are identical across dark and light so the stylesheet never
branches: rules reference ``$panel``, ``$primary``, ``$success`` etc. and
the active theme decides the values.
"""

from __future__ import annotations

from textual.theme import Theme

RIGGER_DARK = Theme(
    name="rigger-dark",
    primary="#5ccfe6",
    secondary="#5ccfe6",
    success="#7fd962",
    warning="#ffd580",
    error="#ff6b6b",
    accent="#5ccfe6",
    foreground="#cbccc6",
    background="#0b0e14",
    surface="#12161f",
    panel="#1a1f2b",
    dark=True,
    variables={
        # One new token per evidence kind; Ayu cyan itself is untouched.
        "news": "#5ccfe6",
        "filing": "#ffd580",
        "price": "#7fd962",
        "event": "#c39ac9",
        "fundamental": "#cbccc6",
    },
)

RIGGER_LIGHT = Theme(
    name="rigger-light",
    primary="#0f7d93",
    secondary="#0f7d93",
    success="#3f8f2b",
    warning="#8f6a00",
    error="#c0392b",
    accent="#0f7d93",
    foreground="#22252a",
    background="#f4f4ef",
    surface="#e9e9e1",
    panel="#dddbcf",
    dark=False,
    variables={
        "news": "#0f7d93",
        "filing": "#8f6a00",
        "price": "#3f8f2b",
        "event": "#7a4a86",
        "fundamental": "#5a5c55",
    },
)

THEMES = (RIGGER_DARK, RIGGER_LIGHT)
