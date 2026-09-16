# Rigger TUI Style — "One Terminal"

> **Status: not started.** Written 17 September 2026. Successor to
> `TUI_REDESIGN_PLAN.md`, which is delivered and historical. That plan built the
> design system; this one finishes the job by making every screen actually use it.
>
> **Depends on `feat/chrome-redesign`.** This branch is cut from `main`, but the
> code the plan describes is not on `main` yet — `rigger/tui/screens/research.py`,
> `rigger/quotes.py` and `rigger/asset_metrics.py` arrive with the twelve commits
> on `feat/chrome-redesign` (`b58a663`..`d5e4250`). Every file:line below was read
> at `d5e4250`. **Merge that branch before executing this plan**, then re-check the
> line numbers — they will have moved.
>
> **Visual contract:** https://claude.ai/artifact/3Hcgp7FmoWzwv5ZGTbhajt — eight
> artboards covering every panel and sixteen states: Evidence (today, populated,
> empty), Watchlist (populated, empty, add dialog), Report (populated, empty,
> generating, legacy), Theses (populated, empty, new dialog), Ask (conversation,
> empty), Settings, Dialogs (go, provider, model, help), and the primitives board.
> Build against that canvas, not against prose descriptions of it.

## Context

The app contains **two visual languages**, and only one of them is wanted.

Home and Theses read as a terminal. Home draws a bordered box with its title in the
border line (`╭─ SINCE YOU LAST LOOKED ──╮`), a block-shaded wordmark, sparklines,
and a clickable key grid where the key is the bright thing. Theses generates a
`KeyStrip` from its own `BINDINGS` and every empty state names the key that fixes it
(`"No summary yet. Press s to summarise"`).

Research/Evidence reads as a GUI bolted into a terminal: three-row shadow-slab
buttons (Textual's `hkey` border, `▔▔▔`/`▁▁▁`), `▊▔▔▔▎` dropdowns (the `panel`
border), a three-row rounded search box, and a preview pane that prints a raw Python
list of 64-character hashes. Five buttons across the top cost three rows to say five
words, and none of that screen's fourteen `BINDINGS` is advertised anywhere on it.

Rendering every screen at 130×40 shows the split is systemic:

1. **Keyboard-dead screens.** `chat.py` and `config.py` declare **no `BINDINGS` at
   all**. Chat's web-search `Switch` and config's Diagnostics `Collapsible` are
   mouse-only. Targets' group collapse is bound but advertised nowhere. **25 stock
   `Button`s remain** across research (9), targets (9), provider_picker (4),
   theses (2) and model_picker (1).
2. **The design system is bypassed.** `ProviderPicker`, `KeyEntryModal`,
   `CustomFormModal` and `ModelPicker` still subclass bare `ModalScreen`, not
   `Dialog`; they now *render* framed, but only because the frame CSS was copied
   into each one. `HelpScreen` subclasses `Screen` and re-implements `Dialog`'s CSS
   by hand. Two screens use raw `DataTable` instead of `RiggerTable`, giving three
   different cursor behaviours. `help.py` hand-rolls the grid `KeyGrid` exists to
   draw. `KeyStrip` exists and **only Theses uses it**.
3. **Constants are copy-pasted.** Six modal widths (64/64/64/72/80/84), four
   responsive breakpoints (72 / 81 / 100 / 109), five key notations (`[ a ]`,
   `<Enter>`, `**a**`, `a add`, `^p`), and nine copies of
   `color: $primary; text-style: bold` meaning "this is a title".
4. **One concept, many names.** The evidence pool is "sources", "Evidence", "stored
   evidence" and "Research · Evidence". The watchlist has five spellings in
   `targets.py` alone.
5. **Defects found while rendering** — all independent of the restyle:
   - `targets.py:318` — `#target-chart-change { color: $success; }` is **green
     regardless of sign**, so a loss renders green. Same file hardcodes Rich styles
     (`"bold bright_white"`, `"green"`, `"red"`) while elsewhere reading
     `app.current_theme` correctly.
   - **Theses' new-thesis dialog renders as five blank rounded boxes** — no labels,
     no visible placeholders, and an unlabelled `▔▔▔`/`▁▁▁` slab where Create should
     be. You cannot tell what any field is for.
   - **Duplicate evidence rows** — `US:AAPL close 332.27`, `326.57` and `315.34`
     each appear twice. A data-layer dedup bug that inflates the source count
     reports are built from.
   - Watchlist rows end in a bare `—` where the quote should be; the feed exists but
     the row is a middot-joined string with nowhere to put a number.
   - The four metric cards render as **empty rounded boxes**, half the screen wide.
   - Go picker truncates: `[ 3 ]  Research ·`.
   - `config.py` shows `llm provider:` with an empty value; plugin rows are
     double-spaced; `config.py:66` says **"press w to add one"** and `w` is bound
     nowhere.
   - Model picker: the Refresh button overlaps the "catalog empty" status text.
   - `help.py:69` tells the user to select **"Inspect source"**; no such control
     exists. `help.py:33` says to **click** save, in a keyboard-first app.

**Intended outcome:** one visual language, pushed down into the shared primitives so
screens inherit it instead of re-implementing it, with every action reachable *and
visible* as a key.

## The system, in four rules

**1. Frame.** Square btop boxes. Title in the top border with its hotkey accented,
badge right. `border: solid $panel`; `heavy $primary` when focused.

**2. Keys — advertised exactly once per screen, on the thing they act on.**

| Surface | Carries | Example |
|---|---|---|
| Tab strip | switching views | `e` `r` |
| Chip row | screen-wide actions | `n` `u` `R` |
| Pane bottom border | acts on that pane | `/` `o` `v` `esc` |
| Status bar | **never** — location and live status only | — |

So: **no separate `KeyStrip` row** (retire the one Theses has), and **no key hints in
the status bar** (`c Settings`, `? help · ^p go` come out; those keys stay bound,
just not permanently advertised). The nav entries stay — they are the only "which
screen am I on" indicator. `KeyStrip` survives only as a fallback for a screen with
no bordered pane to hang hints on.

**3. Buttons.** Gone. `[key] label` chips, one row, clickable, with resting / active
/ disabled states. A disabled chip says why (`[n] generate report — needs evidence`).

**4. Palette.** Ayu cyan unchanged. The only addition is five evidence-kind colours.

Verified on Textual 8.2.8 — `border_title` accepts markup, so the accented hotkey
needs no custom rendering:

```
┌─ e sources ──────────────────────── 24 ─┐    border: solid $panel
│ Source                    Type          │    border_title    = "[$accent]e[/] sources"
└─ / filter   enter open ─────────────────┘    border_subtitle = key hints
```

## Hard constraints

- **No hex in CSS.** Rules use theme tokens; Rich `Text` resolves them via
  `app.current_theme.to_color_system().generate()`, the pattern already in
  `theses.py:350-352`. Do not add a fifth colouring mechanism.
- **Screens that mount standalone must keep working.** Shared-widget layout lives in
  `DEFAULT_CSS` on the widget class, not `rigger.tcss` — see `widgets.py:1-11`.
- **80×24 must not clip.** A border costs 2 rows where the winbar cost 1, so every
  pane gets +1 row. Against that, retiring the `KeyStrip` row and the three-row
  `Button` bars gives rows back — Research nets *out* ahead. `theses.py:59-63`
  documents a 24-row squeeze. Check at 80×24, and drop `PaneStack` children's top
  border so nested panes never double up.
- **Tests stay green.**

---

## 1. Primitives — `widgets.py`, `theme.py`, `rigger.tcss`

**`Pane` gains a border, loses the winbar.** Replace `PaneBar` with native
`border_title` / `border_subtitle`. Keep `title`/`badge`; add `key` and `hints`.

**`ActionChip` replaces every `Button`.** `KeyHint` (`widgets.py:246`) already renders
`[ key ]` with `background: $panel; color: $primary; bold` — extend it into a
clickable one-row chip posting a message the way `NavKey.on_click` (`shell.py:111`)
does.

**`TabStrip`** for Evidence/Report, replacing the hand-toggled `-tab-active` Button
hack, which currently collides with the permanently-primary `#report-generate` so two
buttons read as primary at once.

**De-chrome the stock widgets** globally: `Button { border: none; height: 1 }`,
`Select` off the `panel` border, `Input` to the one-row left-bar treatment
`ThesisForm` (`theses.py:65-93`) already proved, and an inverted `DataTable` header.

**Theme additions:** `news #5ccfe6 · filing #ffd580 · price #7fd962 · event #c39ac9 ·
fundamental #cbccc6`.

**One source of truth** for modal width, breakpoints, `.muted`, and the title style.

## 2. Sweep the screens

1. Delete local CSS the primitives now own — the nine title copies, the duplicated
   gutter rule (`chat.py:30`), the doubled `.tg-field` blocks, the duplicate
   `#target-inspector-title`.
2. Replace `Button` with `ActionChip`; give every `Pane` its `key` and `hints`.
3. Apply the keys-once rule above. Remove the Theses `KeyStrip` and the status-bar
   key hints.
4. Add the missing `BINDINGS`: **`chat.py`** (`i` focus input, `w` web search,
   `space` toggle target, `ctrl+l` clear), **`config.py`** (`D` diagnostics,
   `R` refresh), **`research.py`** (`R` refresh company, `L` load more),
   **`targets.py`** (`space` fold group), **`help.py`** (`tab` switch tab).
5. Route the four modals through `Dialog`; swap the two raw `DataTable`s for
   `RiggerTable`; move `help.py` onto `KeyGrid`.
6. Settle the vocabulary and use one key notation (`[ a ]`) everywhere, `help.py`'s
   prose included.

## 3. Watchlist information design

- **Prices in the list.** Columns `Symbol · Last · Chg% · Age`, replacing the
  middot-joined string. This is the panel's whole job.
- **Change coloured by sign**, which also fixes `targets.py:318`.
- **No per-row sparkline.** The chart lives in the inspector; a second one per row is
  duplication. The freed columns go to the inspector (list ~470px, not 548px).
- **Group rows carry their own aggregate move** and fold with `space`.
- **Metric cards** get square borders with the title in the border, filled from
  `asset_metrics.py`'s real groups: Profitability, Valuation, Balance sheet,
  Shareholder returns.
- **Add dialog** to one row per field with a visible label and value — currently six
  three-row rounded inputs, 18 rows of chrome.

## 4. Evidence information design

- **Colour rows by kind**, the way `theses.py:414-417` colours status cells.
- **Restore the Type column** and widen the list pane.
- **Collapse the price series** into one expandable `▸ prices (180)` row.
- **Fix the preview dump.** `preview()` falls back to
  `"\n".join(f"{k}: {v}" for k, v in item.raw.items())`, printing a Python list repr
  of 64-char hashes. Render `raw` as aligned key/value rows, truncate hashes, move
  internal keys to a muted footer.
- **Deduplicate bars** before they reach the list (see the defect above).

## 5. Defects

Independent of the restyle, land them whenever: the always-green change indicator,
the blank new-thesis dialog fields, duplicate evidence rows, the dead `press w`, the
phantom "Inspect source", the `click save` instruction, the go-picker truncation, the
empty `llm provider:` value, and the model-picker button overlap.

## 6. Verification

1. `uv run pytest -q` and `uv run ruff check .`
2. **Render every screen to text at three widths.** This harness works:

   ```python
   async with app.run_test(size=(W, H)) as pilot:
       await pilot.press("3"); await pilot.pause()
       for strip in app.screen._compositor.render_strips():
           print(strip.text)
   ```

   Check **130×42**, **100×30** and **80×24**. The last is where the extra border row
   and the `theses.py` 24-row concern will bite.
3. **Regression tests, the part that stops this recurring:**
   - no screen ships a `Button`;
   - every `show=True` binding is advertised in exactly one surface, and never in the
     status bar;
   - every modal subclasses `Dialog`;
   - every `DataTable` in a screen is a `RiggerTable`.
4. **Live run** — `uv run rig`, then `1` → `2` → `3` → `4` → `5` using only the
   keyboard. Anything needing the mouse is a bug.
5. **Compare against the canvas** — https://claude.ai/artifact/3Hcgp7FmoWzwv5ZGTbhajt
6. **Confirm Home and Theses are unchanged in feel**, only unified.

## 7. Suggested commit order

1. `feat(tui): bordered Pane with border-title hotkeys` — `widgets.py`, `rigger.tcss`
2. `feat(tui): ActionChip and TabStrip, retire stock Buttons`
3. `feat(tui): evidence-kind colours and shared layout constants`
4. `refactor(tui): route every modal through Dialog and RiggerTable`
5. `feat(tui): keys once per screen, in the pane that owns them`
6. `feat(tui): watchlist rows carry prices; metric cards filled`
7. `feat(tui): evidence rows by kind, collapsed price series, readable preview`
8. `fix(tui): signed change colour, blank dialog fields, duplicate bars, dead hints`
9. `test(tui): assert no Buttons, every binding advertised once`
