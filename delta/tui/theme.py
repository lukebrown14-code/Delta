"""Delta theme: pure-black terminal with a deep blue accent.

Two rules keep the palette readable on ``#000000``:

* A base token is **ink**: ``$primary`` fills the cursor row, the active nav
  tab and the wordmark; ``$success``/``$error`` fill bars and stripes. They
  are the brand hexes exactly and are too dark to read as text on black
  (2.5:1 and 3.3:1).
* The ``text-`` variant is what you **read**: ``$text-primary`` for hotkeys
  and titles, ``$text-success``/``$text-error`` for gains and losses. Every
  ``color:`` rule in the TUI uses a ``text-`` token; a test enforces it.

``variables`` overrides the derivations Textual gets wrong on a pure-black
background (borders that come out black, hover that comes out transparent,
scrollbars that vanish). Token names are identical across dark and light so
the stylesheet never branches.
"""

from textual.theme import Theme

DELTA_DARK = Theme(
    name="delta-dark",
    primary="#264b96",
    secondary="#264b96",
    accent="#264b96",
    success="#15803d",
    warning="#d97706",
    error="#b91c1c",
    foreground="#d4d4d4",
    background="#000000",
    surface="#0d0d0d",
    panel="#1a1a1a",
    dark=True,
    variables={
        # What you read: brighter siblings of the ink colours, same hue.
        "text-primary": "#5b8def",
        "text-secondary": "#5b8def",
        "text-accent": "#5b8def",
        "text-error": "#f87171",
        "text-success": "#22c55e",
        "text-warning": "#f59e0b",
        "text-muted": "#8a8a8a",
        "text-disabled": "#5c5c5c",
        # Lines: separators (``$border-blurred``) want a quiet grey, focus
        # (``$border``) wants the readable blue.
        "border": "#5b8def",
        "border-blurred": "#333333",
        # Selection and cursor: white on the brand blue; the unfocused
        # cursor stays visible instead of Textual's 30% fade.
        "block-cursor-foreground": "#ffffff",
        "block-cursor-background": "#264b96",
        "block-cursor-blurred-background": "#264b96 50%",
        "block-hover-background": "#ffffff 8%",
        # Chrome that would otherwise inherit the 2.5:1 blue or vanish.
        "footer-key-foreground": "#5b8def",
        "scrollbar": "#3a3a3a",
        "scrollbar-hover": "#525252",
        "scrollbar-active": "#5b8def",
        "scrollbar-background": "#0d0d0d",
        "markdown-h1-color": "#5b8def",
        "markdown-h2-color": "#5b8def",
        "markdown-h3-color": "#5b8def",
        "button-color-foreground": "#ffffff",
        # Evidence kinds. Five hues that are not the semantic four, so a
        # filing never reads as a warning; all >= 9.8:1 on black. The quietest
        # is ``price``, which is the noise kind.
        "kind-news": "#5ccfe6",
        "kind-filing": "#ffd580",
        "kind-event": "#d7a1ff",
        "kind-fundamental": "#7ee0c0",
        "kind-price": "#a8b2c8",
    },
)

DELTA_LIGHT = Theme(
    name="delta-light",
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
    # Rich Text cells read these tokens as colours; Textual's derived
    # "auto 60%" is not one, so pin them here as well.
    variables={
        "text-muted": "#6b6e74",
        "text-disabled": "#9a9da3",
        # The same five hues, darkened for a light ground.
        "kind-news": "#0f6d80",
        "kind-filing": "#8a5a00",
        "kind-event": "#7d3fa8",
        "kind-fundamental": "#0a6b52",
        "kind-price": "#5a6474",
    },
)

THEMES = (DELTA_DARK, DELTA_LIGHT)
