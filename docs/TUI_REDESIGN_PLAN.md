# Rigger TUI Redesign — "Terminal Desk"

> **Status: delivered — historical record, not a work item.**
>
> This plan was written against the pre-redesign TUI and committed after the
> redesign had already landed (`fce9dc4`, `9bca979`, `0c3549a`). Every root
> cause in *Context* below is already fixed: `rigger/tui/theme.py`,
> `rigger/tui/shell.py` and a 337-line `rigger/tui/widgets.py` all exist,
> `rigger.tcss` is 153 lines of token-based rules, `RiggerCommands` is wired
> into the command palette, `RiggerScreen.on_screen_resume` re-runs
> `refresh_view`, and `screens/help.py` generates its keymap from `BINDINGS`.
> **Read it for the reasoning and the test contract, not as instructions.**
>
> Where the shipped design deliberately diverges from this plan, the code
> wins:
>
> | Plan says | Shipped instead |
> |---|---|
> | Persistent left `NavRail` | `NavStrip` + `GoPicker` modal ("the replacement for the nav rail") |
> | Bordered `Card` widget | Borderless `Pane` grammar — inline winbar title, gutter + faint rule |
> | Mount Textual's `Header()`/`Footer()` | Custom `StatusLine` / `ScreenFooter` in `shell.py` |
> | `StatTile`, `Sparkline`, `Digits` | Not adopted |
>
> The *Hard constraints* section is still accurate and still binding.

## Context

The TUI works but looks and feels like a debug harness. The root causes are specific, not vague:

1. **`rigger/tui/app.py` never mounts `Header()` or `Footer()`.** All 11 `BINDINGS` (`app.py:28-40`) are invisible. The only way to learn the app is to press `?`, and `screens/help.py:9` is a hand-maintained duplicate of the binding list that can silently drift.
2. **No navigation surface at all.** Eight screens reachable only by memorised digits. `action_switch_screen` (`app.py:65`) is a flat switch — no tabs, no rail, no indication of where you are or what else exists.
3. **`rigger/tui/rigger.tcss` is 39 lines of padding and margin.** No palette, no borders, no theme, no dark-mode handling. Colour appears only as inline Rich markup inside strings (`[bold cyan]RIGGER[/bold cyan]`, `home.py:21`).
4. **Half the screens are f-strings in a box.** Home, Data and Config render their entire content as one joined markup string dumped into a `Static` (`home.py:36-53`, `data.py:24-40`, `config.py:22-47`). Structured data is flattened into text that can't be sorted, selected or scrolled per-row.
5. **Textual 8.2.8 is ~10% used.** No `Header`, `Footer`, `Markdown`/`MarkdownViewer`, `Sparkline`, `Digits`, `Collapsible`, `Switch`, `Rule`, `LoadingIndicator`, no `Theme` API, no command palette provider. `widgets.py` contains a single `Panel` class that is imported nowhere.

**Intended outcome:** a dense, high-contrast "trading desk" aesthetic — dark palette with amber/green accents, a persistent left nav rail, a live status bar, and real widgets instead of text dumps — with every existing test still passing.

**Direction chosen:** Terminal desk (dark, dense) · persistent sidebar rail · redesign all eight screens.

Target shape:

```
┌─ RIGGER ───────────── ASX ● live   opus-5   $0.42 ─┐
│ WATCH    │ BHP.AX   42.18  ▲1.2%  ▁▂▃▅▆  3 new    │
│ EVIDENCE │ RIO.AX  118.40  ▼0.4%  ▇▆▅▃▂  1 new    │
│ REPORTS  │ FMG.AX   19.70  ▲0.1%  ▁▁▂▃▄  0        │
│ THESES   │────────────────────────────────────────│
│ ASK      │ LAST REPORT  iron-ore  2h ago  12 cites│
└ 1..6 move   g gather   r report   ? help   q quit ─┘
```

---

## Hard constraints (from `tests/test_tui.py`)

These are a public contract. The redesign is additive around them.

| Must not change | Where |
|---|---|
| `RiggerApp(rig)` constructor signature | `test_tui.py:58` |
| `app.screen.name` values: `home`, `data`, `config`, `targets`, `console`, `help` | asserted in every test |
| Bindings `2`, `3`, `w`, `c`, and `?`-toggles-help-closed | `test_tui.py:136-146` |
| IDs `#tg-name` `#tg-kind` `#tg-market` `#tg-tickers` `#tg-add` `#tg-remove` `#target-table` `#console-input` `#console-log` | `test_tui.py:82-159` |
| Widget **types**: `#target-table` → `DataTable` (`.row_count`), `#console-log` → `RichLog` (`.lines[].text`), `#tg-*` → `Input` (`.value`) | same |
| Screens mount offline against a `SimpleNamespace` cfg with empty `llm_routing`/`targets` | `FakeRig`, `test_tui.py:18-47` |

Also preserve the **standalone-screen property** documented in `theses.py:1-6` and `reports.py:18-22`: those screens must still mount under any App without `app.py`'s stylesheet. Keep their `DEFAULT_CSS`/`CSS`, but rewrite the rules to use `$`-tokens so they inherit the theme when run inside `RiggerApp`.

Reports, Theses, Chat, ModelPicker and Home's `refresh_view` have **no test coverage** — free rein there.

---

## 1. Design system

### `rigger/tui/theme.py` (new)

Register a `textual.theme.Theme` named `rigger-dark` and set `self.theme` in `RiggerApp.on_mount`. Ayu-Dark-derived, chosen for contrast at small sizes:

| Token | Value | Use |
|---|---|---|
| `background` | `#0b0e14` | app base |
| `surface` | `#12161f` | screen body |
| `panel` | `#1a1f2b` | cards, rail |
| `primary` | `#ffb454` | amber — brand, active nav, focus |
| `secondary` | `#5ccfe6` | cyan — links, citations |
| `success` | `#7fd962` | ✓ checks, up moves |
| `warning` | `#ffd580` | stale data |
| `error` | `#ff6b6b` | ✗ checks, down moves |
| `foreground` | `#cbccc6` | body text |

Add a matching `rigger-light` so `App.theme` can toggle; the token names stay identical so no CSS changes.

### `rigger/tui/rigger.tcss` (rewrite)

Grow from 39 lines into a real stylesheet, organised in sections: `/* tokens */`, `/* shell */`, `/* cards */`, `/* tables */`, `/* forms */`, `/* modals */`. Rules reference `$panel`, `$primary`, `$success` etc. — never hex literals. Keep the existing id rules (`#target-table`, `#console-input`, `#console-log`, `#tg-*`) so nothing regresses; restyle rather than delete.

Global table treatment: `zebra_stripes = True`, `cursor_type = "row"`, `$primary` cursor background, `$panel` header with `text-style: bold`.

### `rigger/tui/widgets.py` (replace the dead `Panel`)

`Panel` is imported nowhere — remove it and build the real kit:

- **`Card(Vertical)`** — bordered container using Textual's `border_title`. Replaces every `[bold]Title[/bold]\n…` string. This is the single highest-leverage widget; it is what makes the layout read as panels.
- **`StatTile(Vertical)`** — `Digits` for the number + muted label. For Home's top row.
- **`StatusDot(Static)`** — `●` coloured by `ok | warn | error`. Replaces `[green]✓[/green]`/`[red]✗[/red]`.
- **`Pill(Static)`** — small inline token for kinds, statuses and thesis health. Reuse the existing `state_style()` / `badge_text()` from `rigger/thesis_health.py` rather than inventing new styling — `theses.py:233-251` already does this correctly.
- **`KeyHint(Static)`** — `[ g ]` style key chips for empty states.

---

## 2. Shell

### `rigger/tui/shell.py` (new)

- **`TopBar(Horizontal)`** — brand `RIGGER`, then right-aligned status cells: data freshness (`StatusDot` + age, from `services.data_health`), active model id, session spend (from `services.llm_costs`). Refreshed on a `set_interval`.
- **`NavRail(Vertical)`** — always-visible left column, one `NavItem` per screen showing key hint + label, `-active` class driven by `app.screen.name`. Clicking posts a message the app turns into `switch_screen`. Width ~14 cells; collapses to icon-width under a CSS breakpoint so narrow terminals still work.
- **`RiggerScreen(Screen)`** — base class whose `compose()` yields `TopBar`, `Horizontal(NavRail, <content>)`, `Footer()`, and delegates the middle to an abstract `compose_content()`.

Each of the eight screens then renames `compose` → `compose_content` and deletes its own `Static(..., classes="title")` banner (the rail and top bar now carry identity). `name` class attrs stay exactly as they are.

### `rigger/tui/app.py`

- Register and apply the theme in `on_mount`.
- Add `Binding(..., tooltip=...)` to every entry so the `Footer` reads well.
- Add a `RiggerCommands(Provider)` so the built-in command palette (`ctrl+p`) can jump to any screen and run Gather/Report — discoverability without more chrome.
- **Fix the stale-data bug:** `refresh_view()` is currently only called from `on_mount`, and all eight screens are constructed eagerly at startup (`app.py:51-63`), so every screen shows data frozen at launch. Add an `on_screen_resume` hook in `RiggerScreen` that calls `refresh_view()` when defined.

---

## 3. Screens

| Screen | Change | Risk |
|---|---|---|
| **Home** (`home.py`) | The showcase. `StatTile` row (targets · evidence rows · latest bar date · spend), then Cards: *Setup* (`StatusDot` per check), *What you watch* (`DataTable` + `Sparkline` per target), *Latest report* (name, age, citation count, Open). Replaces four `Static`s. | none — untested |
| **Data** (`data.py`) | Two `Static` blobs → two `DataTable`s in Cards: *Stored evidence* (table / rows / latest) and *Model spend* (task / model / calls / $). | none — untested |
| **Config** (`config.py`) | One blob → Cards: Provider, Model routing (`DataTable`), Plugins (`DataTable` + `StatusDot`), Targets. | low |
| **Reports** (`reports.py`) | Biggest win: swap `RichLog(markup=False)` for **`MarkdownViewer`** — real headings, tables and a table of contents. Split left target list / right document. Add `LoadingIndicator` during the `@work` generation, which today gives no feedback. | none — untested |
| **Theses** (`theses.py`) | Stacked → side-by-side: table left, detail right. Health badge becomes a `Pill`. Supporting/Against/Unknown become `Collapsible` sections. Accept/Reject get `variant="success"`/`"error"`. Keep the `remove_children()`+`mount()` rebuild (`theses.py:150-226`). | none — untested |
| **Chat** (`chat.py`) | Transcript is re-rendered as one markup string (`chat.py:95-104`) → mount per-message `Card`s, user dim/right, assistant `$primary` left border, citations as `Pill`s. Targets `SelectionList` into a left Card; web toggle → `Switch`. | none — untested |
| **Console** (`console.py`) | Keep `#console-input` (`Input`) and `#console-log` (`RichLog`) **exactly** — hard test contract. Wrap in a Card, add a `❯` prompt glyph, and attach `SuggestFromList` built from the command names already parsed in `console.py:122-163`. | medium — IDs/types are asserted |
| **Targets** (`targets.py`) | Keep `#target-table`, `#tg-*`, `#tg-add`, `#tg-remove` and their types. Move the 5-field form into a Card *below* the table, give Buttons `variant="primary"`/`"error"`, kind rendered as a `Pill`. | medium — IDs/types are asserted |
| **ModelPicker** (`model_picker.py`) | Real modal: dimmed backdrop, `$primary` bordered dialog, price columns colour-graded. | low |
| **Help** (`help.py`) | Generate the keymap from `app.BINDINGS` instead of the hardcoded duplicate, so it can't drift. Preserve `?`-toggles-closed. | low — `?` behaviour asserted |

Preserve the documented empty-`DataTable` gotcha everywhere it appears (`targets.py:81-82`, `reports.py:74-76`): guard on `row_count`, never `cursor_row`. `test_tui.py:149` locks this in.

---

## 4. Verification

1. **Tests unchanged and green** — the primary gate, since the redesign must not touch the contract:
   `uv run pytest tests/test_tui.py -v` then the full `uv run pytest`.
2. **Lint** — `uv run ruff check rigger/tui`.
3. **Live run** — `uv run rig`, then walk the flow from `PROJECT_SPEC`: add a target on Targets → check Data → generate on Reports → ask on Chat. Confirm the rail highlights the active screen and the Footer shows keys on every screen.
4. **Dev tooling** — `uv run textual run --dev rigger.tui.app:RiggerApp` with `uv run textual console` in a second pane for live CSS edits and layout warnings.
5. **Narrow terminal** — resize to 80×24 and confirm the rail collapses and no content is clipped.
6. **Visual record** — a throwaway script using `App.save_screenshot()` to emit an SVG per screen, so before/after can actually be compared.
7. **Check, don't assume:** verify whether the single-letter global bindings (`q`, `c`, `w`, `m`) fire while an `Input` is focused on Targets/Console/Chat. Textual's `Input` normally consumes printable keys, so this may already be fine — test it before changing any binding, since `w`/`c`/`?` are asserted.

---

## 5. Suggested commit order

1. `feat(tui): theme + design tokens` — `theme.py`, `rigger.tcss`, no layout change
2. `feat(tui): card and stat widgets` — `widgets.py`
3. `feat(tui): nav rail, top bar, footer shell` — `shell.py`, `app.py`, screens adopt `compose_content`
4. `feat(tui): rebuild Home / Data / Config on real widgets`
5. `feat(tui): MarkdownViewer reports, split theses, chat bubbles`
6. `feat(tui): polish targets, console, model picker, generated help`
