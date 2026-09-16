# Rigger TUI Style — "One Terminal"

> **Status: not started.** Written 17 September 2026 against `feat/chrome-redesign`
> at `d7d0606`. Successor to `TUI_REDESIGN_PLAN.md`, which is delivered and
> historical. That plan established the design system; this one finishes the job by
> making every screen actually use it.
>
> **Visual contract:** https://claude.ai/artifact/3Hcgp7FmoWzwv5ZGTbhajt — four
> artboards (Evidence today vs. proposed, Report, and the primitives board). Build
> against that canvas, not against prose descriptions of it.

## Context

The app contains **two visual languages**, and only one of them is wanted.

Home and Theses read as a terminal. Home draws a bordered box with its title in the
border line (`╭─ SINCE YOU LAST LOOKED ──╮`), a block-shaded wordmark, sparklines,
and a clickable key grid where the key is the bright thing. Theses generates a
`KeyStrip` from its own `BINDINGS`, labels its buttons with the key verbatim
(`"a accept"`), and every empty state names the key that fixes it
(`"No summary yet. Press s to summarise"`).

Research/Evidence reads as a GUI bolted into a terminal. Rendering it shows why:
three-row shadow-slab buttons (Textual's `hkey` border, `▔▔▔`/`▁▁▁`), `▊▔▔▔▎`
dropdowns (the `panel` border), a three-row rounded search box, and a preview pane
that prints a raw Python list of 64-character hashes at the reader. Five buttons
across the top cost three rows to say five words, and none of the screen's eight
`BINDINGS` is advertised anywhere on screen.

The split is systemic, not local to one screen:

1. **Keyboard-dead screens.** `chat.py` and `config.py` declare **no `BINDINGS` at
   all**. Chat's web-search `Switch` (`chat.py:91`) and config's Diagnostics
   `Collapsible` (`config.py:70`) are mouse-only. `research.py:81-83` comments
   "Every button here has a key" — **`"Refresh company"` (`:151`) and
   `"Load more"` (`:178`) have none**. Targets' group collapse/expand
   (`targets.py:712-723`) is undocumented anywhere.
2. **The design system is bypassed.** Four modals — `ProviderPicker`,
   `KeyEntryModal`, `CustomFormModal` (`provider_picker.py`) and `ModelPicker`
   (`model_picker.py`) — skip `Dialog` and re-implement it, so they render with no
   frame, no backdrop dim and no centring. `HelpScreen` copies `Dialog`'s CSS by
   hand. `provider_picker.py:88` and `model_picker.py:68` use raw `DataTable`
   instead of `RiggerTable`, giving **three different table cursor behaviours**.
   `help.py` hand-rolls the exact grid `KeyGrid` exists to draw. `KeyStrip` exists
   and **only Theses uses it**.
3. **Constants are copy-pasted, not shared.** **Six modal widths**
   (64/64/64/72/80/84), **four responsive breakpoints** (`PaneRow.NARROW_WIDTH` 72,
   `StatusBar.MINIMAL_WIDTH` 81, `research.py:627` 100, `StatusBar.COMPACT_WIDTH`
   109), **five key notations** (`[ a ]`, `<Enter>`, `**a**`, `a add`, `^p`), and
   **nine copies** of `color: $primary; text-style: bold` meaning "this is a title".
4. **One concept, many names.** The evidence pool is "sources", "Evidence", "stored
   evidence" and "Research · Evidence" depending on where you look. The watchlist
   has five spellings in `targets.py` alone.
5. **Three outright defects.** `targets.py:318` sets
   `#target-chart-change { color: $success; }` — **green regardless of sign**, so a
   loss renders green. `config.py:66` says **"press w to add one"**; `w` is bound
   nowhere. `help.py:69` says to select **"Inspect source"**; no such control exists.
   `targets.py` also hardcodes Rich styles (`"bold bright_white"`, `"green"`,
   `"red"`) bypassing the theme, while elsewhere in the same file reading
   `app.current_theme` correctly.

**Intended outcome:** one visual language across the app — the Home/Theses one —
pushed down into the shared primitives so screens inherit it instead of
re-implementing it, with every action reachable *and visible* as a key.

**Direction chosen** (settled with the user, 17 Sep 2026):

| Decision | Choice |
|---|---|
| Palette | **Keep Ayu cyan** (`#5ccfe6` on `#0b0e14`) exactly as-is. Only addition: five evidence-kind colours. |
| Panel frame | **Square btop boxes** — title in the top border with the hotkey accented, key hints in the bottom border. |
| Buttons | **`[key] label` chips** (the Catppuccin style), one row, clickable. Not bare text, not stock Textual buttons. |
| Scope | Whole app, including the Evidence panel's information design. |

Target shape — verified working on Textual 8.2.8, no custom rendering needed:

```
┌─ e sources ──────────────────────── 24 ─┐    border: solid $panel
│ Source                    Type          │    border_title    = "[$accent]e[/] sources"
│ ▸ prices (180)            price         │    border_subtitle = key hints
└─ / filter   enter open ─────────────────┘

┏━ e sources ━━━━━━━━━━━━━━━━━━━━━━━━ 24 ━┓    border: heavy $primary  (focused)
```

`border_title` accepts console markup, so the accented hotkey letter is free.
`BORDER_CHARS["solid"]` is `┌─┐ │ │ └─┘`; `heavy` is `┏━┓`.

## Hard constraints

- **No hex in CSS.** Every rule uses theme tokens. Rich `Text` resolves tokens via
  `app.current_theme.to_color_system().generate()` — the pattern already in
  `theses.py:350-352`. Do not add a fifth colouring mechanism.
- **Screens that mount standalone must keep working.** Layout for shared widgets
  lives in `DEFAULT_CSS` on the widget class, not in `rigger.tcss` — see the
  docstrings in `widgets.py:1-11` and `shell.py:1-11`.
- **80×24 must not clip.** A border costs 2 rows where the winbar cost 1, so every
  pane gets **+1 row**. Against that, retiring the `KeyStrip` row (§2.3) and the
  three-row `Button` bars gives rows back — a screen like Research nets *out* ahead.
  `theses.py:59-63` already documents a 24-row squeeze. Check at 80×24 before this
  lands, and drop `PaneStack` children's top border so nested panes never double up.
- **255 tests currently pass.** They stay passing.

---

## 1. Primitives — `widgets.py`, `theme.py`, `rigger.tcss`

Everything else follows from this section. No screen should carry its own frame or
chrome after it.

**`Pane` gains a border, loses the winbar.** Replace `PaneBar` with native
`border_title` / `border_subtitle`. Keep `title` and `badge`; add `key` (hotkey to
accent) and `hints` (bottom-border key list). Title renders
`[$accent]{key}[/] {title}`, badge goes to `border_subtitle` right, hints subtitle
left. Focused pane switches to `border: heavy $primary`.

**`ActionChip` replaces every `Button`.** `KeyHint` (`widgets.py:246`) already
renders `[ key ]` with `background: $panel; color: $primary; bold` — that *is* the
`[C] Compose` chip from the reference. Extend it into a clickable one-row
`[n] generate` chip with `-active` / `-disabled` states, posting a message the way
`NavKey.on_click` (`shell.py:111`) already does.

**`TabStrip`** for Evidence/Report: active tab filled `$primary`, inactive muted.
Replaces the hand-toggled `-tab-active` Button hack at `research.py:620-624`, which
currently collides with the permanently-primary `#report-generate` (`:153`) so two
buttons read as primary at once.

**De-chrome the stock widgets** globally in `rigger.tcss`, once:
`Button { border: none; height: 1 }` (kills the `hkey` slab), `Select` flattened off
its `panel` border, `Input` to the one-row left-bar treatment `ThesisForm`
(`theses.py:65-93`) already proved, and an inverted `DataTable` header.

**Theme additions** in `theme.py`, for both light and dark:

```
news #5ccfe6 · filing #ffd580 · price #7fd962 · event #c39ac9 · fundamental #cbccc6
```

**One source of truth for the numbers:** a single `MODAL_WIDTH`, a single breakpoint
set replacing 72/81/100/109, one `.muted`, one title style.

## 2. Sweep the screens

Same pattern per screen — repetitive, not novel:

1. Delete local CSS the primitives now own: the nine title copies, the duplicated
   gutter rule (`chat.py:30`), the doubled `.tg-field` blocks
   (`targets.py:298-300` vs `:47-49`), the duplicate `#target-inspector-title`
   (`:314` and `:325`), the belt-and-braces `Pane.-auto` in `config.py`.
2. Replace `Button` with `ActionChip`; give every `Pane` its `key` and `hints`.
3. **Advertise keys in the pane borders, not in a strip.** `Pane(hints=…)` is the
   primary mechanism, and it is contextual: the keys sit on the pane they act on,
   which one bottom strip cannot express. A separate `KeyStrip` row then says
   everything twice — **do not add one to a screen whose panes carry hints**, and
   retire the one Theses has once its panes do. `KeyStrip` survives only as the
   fallback for a screen with no bordered pane to hang hints on. Every `show=True`
   binding must appear in *some* border, `escape` included.
4. Add the missing `BINDINGS`: **`chat.py`** (focus input, toggle web search, clear
   transcript), **`config.py`** (toggle diagnostics, refresh), **`research.py`**
   (`Refresh company`, `Load more`), **`targets.py`** (group collapse/expand),
   **`help.py`** (switch tabs).
5. Route the four rogue modals through `Dialog`; swap the two raw `DataTable`s for
   `RiggerTable`; move `help.py` onto `KeyGrid`.
6. Settle the vocabulary — one word each for evidence, watchlist and gather — and
   one key notation (`[ a ]`) everywhere, including `help.py`'s prose.

Home and Theses should need the least work; they are the reference. Touch them only
to drop CSS that has become shared.

## 3. Evidence panel information design

Today the panel shows 200 rows of which ~180 are `ASX:BHP close 59.30`, all one
colour, with the Type column squeezed out by the `2fr`/`3fr` split.

- **Colour rows by kind** using the section-1 tokens, the way `theses.py:414-417`
  colours status cells with a Rich `Text`.
- **Restore the Type column** and widen the list pane.
- **Collapse the price series** into one expandable `▸ prices (180)` row, so the ~20
  rows that carry information are not drowned.
- **Fix the preview dump.** `preview()` (`research.py:296-317`) falls back to
  `"\n".join(f"{k}: {v}" for k, v in item.raw.items())`, printing a Python list repr
  of 64-char hashes. Render `raw` as aligned key/value rows, truncate hashes, and
  move internal keys (`prompt_version`, `extracted_by`) to a muted footer line.

## 4. The three defects

Small, independent, worth landing regardless of the rest:

- `targets.py:318` — colour `#target-chart-change` by sign, not statically `$success`.
  While there, move `:583` and `:587-601`'s hardcoded Rich styles onto
  `app.current_theme`, which the same file already does correctly at `:673-678`.
- `config.py:66` — `press w` is bound nowhere; point it at the real key (`a`, on the
  Targets screen) or remove the instruction.
- `help.py:69` — "Inspect source" does not exist; the controls are "Open source" and
  "View in report".

---

## 5. Verification

1. **Tests and lint** — `uv run pytest -q` (255 currently pass) and
   `uv run ruff check .`.
2. **Render every screen to text at three widths.** This harness works and should be
   kept as a throwaway script:

   ```python
   async with app.run_test(size=(W, H)) as pilot:
       await pilot.press("3"); await pilot.pause()
       for strip in app.screen._compositor.render_strips():
           print(strip.text)
   ```

   Check **130×42**, **100×30** and **80×24**. The last is where the extra border row
   and the `theses.py` 24-row concern will bite.
3. **New regression tests, the part that stops this recurring:**
   - no screen ships a `Button`;
   - every `show=True` binding is advertised somewhere on its screen — a pane's
     `hints` or, failing that, a `KeyStrip` — and no screen does both;
   - every modal subclasses `Dialog`;
   - every `DataTable` in a screen is a `RiggerTable`.
4. **Live run** — `uv run rig`, then walk it: `1` watchlist → `2` evidence → `3`
   report → `4` theses → `5` ask, using only the keyboard. Anything that needs the
   mouse is a bug.
5. **Compare against the canvas** — https://claude.ai/artifact/3Hcgp7FmoWzwv5ZGTbhajt
6. **Confirm Home and Theses are unchanged in feel**, only unified.

## 6. Suggested commit order

1. `feat(tui): bordered Pane with border-title hotkeys` — `widgets.py`, `rigger.tcss`
2. `feat(tui): ActionChip and TabStrip, retire stock Buttons` — `widgets.py`, `rigger.tcss`
3. `feat(tui): evidence-kind colours and shared layout constants` — `theme.py`, `widgets.py`, `shell.py`
4. `refactor(tui): route every modal through Dialog and RiggerTable` — pickers, `help.py`
5. `feat(tui): key strips and missing bindings on every screen` — the sweep
6. `feat(tui): evidence rows by kind, collapsed price series, readable preview` — `research.py`
7. `fix(tui): signed change colour, dead key hint, phantom control name`
8. `test(tui): assert no Buttons, every binding advertised`
