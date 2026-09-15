"""Rigger theme: dark "trading desk" palette with amber/green accents.

Token names are identical across dark and light so the stylesheet never
branches: rules reference ``$panel``, ``$primary``, ``$success`` etc. and
the active theme decides the values.
"""

from __future__ import annotations

from textual.theme import Theme

RIGGER_DARK = Theme(
    name="rigger-dark",
    primary="#ffb454",
    secondary="#5ccfe6",
    success="#7fd962",
    warning="#ffd580",
    error="#ff6b6b",
    accent="#ffb454",
    foreground="#cbccc6",
    background="#0b0e14",
    surface="#12161f",
    panel="#1a1f2b",
    dark=True,
)

RIGGER_LIGHT = Theme(
    name="rigger-light",
    primary="#b35c00",
    secondary="#0f7d93",
    success="#3f8f2b",
    warning="#8f6a00",
    error="#c0392b",
    accent="#b35c00",
    foreground="#22252a",
    background="#f4f4ef",
    surface="#e9e9e1",
    panel="#dddbcf",
    dark=False,
)

THEMES = (RIGGER_DARK, RIGGER_LIGHT)
