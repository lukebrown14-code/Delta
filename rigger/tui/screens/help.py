"""Help screen: a getting-started tutorial plus a keymap generated from BINDINGS."""

from __future__ import annotations

from textual.app import ComposeResult
from textual.binding import Binding
from textual.containers import Horizontal, Vertical, VerticalScroll
from textual.screen import Screen
from textual.widgets import Markdown, Static, TabbedContent, TabPane

from rigger.tui.widgets import KeyHint

KEY_DISPLAY = {"question_mark": "?"}

# Plain-language tour. Kept as a module constant so the README can
# reuse the exact same words the TUI shows.
TUTORIAL = """\
# Welcome to Rigger

Rigger reads for you. It collects facts about the companies you care about,
keeps them in one place, and writes summaries where **every claim points back
to the fact it came from**.

It does not trade, it does not size positions, and it will not tell you what
to buy or sell. The point is clarity, not tips.

---

## 1. Tell it what you care about

Press **1** for Watchlist.

A watchlist entry is anything you want watched — one company, a whole
sector, or a theme. Press **a** to open the entry form, fill in the fields,
and click **save**. Press **Escape** to close the form.

Use **/** to filter, **↑/↓** to select, and **Enter** to expand a target.
Grouped targets reveal individual ticker prices. Press **d** to remove a
selected target; ticker child rows are informational.

Prices stream from Yahoo while Watchlist is open. **DAY %** is Yahoo’s
daily percentage change: **▲** up, **▼** down, **─** unchanged. Quote age
and connection state are separate; delivery may vary by market. Missing
quotes show **—**. Streaming quotes do not replace gathered historical bars.

| Field | Example |
| --- | --- |
| name | `iron-ore` |
| kind | `industry` |
| market | `asx` |
| tickers | `BHP,RIO,FMG` |

## 2. Let it go collect

Open the command palette (**ctrl+p**) and run **Gather evidence**.

`ingest` pulls prices, news, filings and earnings dates into a local database,
and `extract` turns the news into structured facts. Gather runs both.

Press **2** for Research. Choose a watch target and company, then search and
filter its evidence. Database counts and model spend are under Settings → Diagnostics.

## 3. Read what it found

Press **3** for the Research report view.

Pick a company, press **Generate report**, and wait. You get a written summary with a
citation on every claim. If a sentence could not be traced back to something
collected in step 2, it gets dropped rather than guessed.

The contents panel on the left jumps between sections. Select **Inspect source**
to read a citation in Evidence, then switch back to Report to resume reading.

## 4. Ask it questions

Press **5** for Ask.

Tick the watchlist entries you want in scope, then type a question. Answers come only
from the evidence collected in step 2 — not from the model's own memory.

Web search is a toggle, it is **off by default**, and anything it returns is
used once and never saved.

## 5. Track an idea over time

Press **4** for Theses.

Write down something you believe — *"iron ore volumes hold up through 2027"* —
and Rigger proposes evidence for and against it as new facts arrive.

You accept or reject each piece yourself. The model only ever *suggests*; it
never decides what counts, and the health badge is calculated from what you
accepted, not from an opinion.

---

## Worth knowing

- **Everything is cited.** You can always check the AI's working.
- **Nothing is a recommendation.** No buy, sell or position sizing, by design.
- **Your data stays local.** Evidence lives in a SQLite file in the project.
- Press **m** to change which model is used on the current screen.
- Press **f2** to switch between the dark and light palettes.

Press **?** or **escape** to close this help.
"""


class HelpScreen(Screen):
    name = "help"

    BINDINGS = [Binding("escape", "app.show_help", "Close help", show=False)]

    DEFAULT_CSS = """
    HelpScreen {
        align: center middle;
        background: $background 60%;
    }
    HelpScreen #help-dialog {
        width: 84;
        height: 90%;
        background: $surface;
        border: round $primary;
        padding: 1 2;
    }
    HelpScreen .help-title {
        color: $primary;
        text-style: bold;
        margin: 0 0 1 0;
    }
    HelpScreen #help-tabs {
        height: 1fr;
    }
    HelpScreen Markdown {
        background: transparent;
    }
    HelpScreen .help-row {
        height: 1;
        margin: 0 0 1 0;
    }
    HelpScreen .help-row Static {
        width: 1fr;
    }
    """

    def compose(self) -> ComposeResult:
        with Vertical(id="help-dialog"):
            yield Static("Rigger help", classes="help-title", markup=False)
            with TabbedContent(id="help-tabs"):
                with TabPane("Getting started", id="help-tour"):
                    yield Markdown(TUTORIAL)
                with TabPane("Keys", id="help-keys"):
                    yield VerticalScroll(*self._key_rows())

    def _key_rows(self) -> list[Horizontal]:
        """One row per visible binding, generated so it cannot drift from the footer."""
        rows: list[Horizontal] = []
        for binding in self.app.BINDINGS:
            if not binding.show:
                continue
            key = KEY_DISPLAY.get(binding.key) or binding.key_display or binding.key
            description = binding.description + (f" — {binding.tooltip}" if binding.tooltip else "")
            rows.append(
                Horizontal(
                    KeyHint(key),
                    Static(description, markup=False),
                    classes="help-row",
                )
            )
        return rows
