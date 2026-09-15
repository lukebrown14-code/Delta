# TUI Plan — A front door for Rigger

> Replace day-to-day use of the `rig` CLI with a Textual terminal UI. The CLI
> stays as the scripting surface. Same working rules as Phase 2
> (`docs/PHASE2_PLAN.md`).

## Goal

Someone who has never seen Rigger can open it, see how the pipeline fits
together, check their setup, run a first pass and read what the model decided
and why. The author can answer "what did the analyst see for CBA and what did
the critic say" from one screen instead of four commands.

## Why a service layer first

Every command in `rigger/cli.py` builds its own `Rigger()` wiring, runs the
work inline in a nested `async def run()` and prints with `console.print`. A
TUI cannot call those. The pipeline bodies move to `rigger/services.py` with a
`log` callback; the CLI and the TUI both call the services. No behaviour
changes in this step, and every existing test must stay green.

## Framework

**Textual.** Rich is already a dependency and Textual is by the same author,
so tables render as they do in the CLI today. `App.run_test()` gives headless
tests, which keeps the "offline `pytest`" rule.

## Step 1 — Service layer

1. **`rigger/runtime.py`**: move the `Rigger` wiring class and `_store_signal`
   out of `cli.py` unchanged. `cli.py` re-exports `_store_signal` so
   `tests/test_pipeline.py` keeps importing it.
2. **`rigger/services.py`**, one async function per pipeline step. Each takes
   `rig: Rigger` and `log: Callable[[str], None]` and returns a result
   dataclass instead of printing:

   ```python
   async def ingest(rig, *, market=None, tickers=None, since=None, log) -> IngestResult
   async def extract(rig, *, since=None, log) -> ExtractResult
   async def analyse(rig, *, strategies, dry_run=False, log) -> list[Signal]
   async def execute(rig, *, since=None, all_=False, log) -> ExecuteResult   # fills, skipped[(instrument, reason)]
   def report(rig, *, date=None, fmt="markdown") -> Path
   def reset_paper(rig, *, signals=False) -> None
   def set_plugin_enabled(rig, name, value) -> None
   ```

   Bodies are today's command bodies with `console.print(...)` replaced by
   `log(...)`. Rich markup stays; both sinks render it.
3. **Read-side queries** in the same module, pure functions over the engine,
   shared by every screen:
   - `signals(engine, *, since=None, strategy=None, limit=200) -> list[Signal]`
     using the `SignalTable` → `Signal` mapping `report()` already does.
   - `signal_by_id(engine, id) -> Signal | None`.
   - `resolve_evidence(engine, evidence_ids) -> Evidence`: groups ids into
     bars (integer ids, summarised as date range and close range), news
     (`NewsItemTable`), events (`EventTable`) and fundamentals
     (`fundamental:<id>`). Phase 3 attribution builds `evidence_sources()` on
     this.
   - `brief_for(rig, instrument_id) -> str | None` via `build_brief(...).render()`
     so the UI shows exactly what the model saw.
   - `portfolio_summary(rig)`: positions with currency and base value, cash,
     equity, recent fills.
   - `data_health(rig)`: row counts per table, latest bar per instrument, FX
     rate present, last signal and last LLM call timestamps.
   - `llm_costs(engine, since=None)`.
   - `setup_checks(rig) -> list[Check]`: provider key present for
     `[llm].provider`, `config.toml` found, DB reachable,
     `[plugins.sec_edgar].contact` set, bars ingested, FX ingested. Each failed
     check carries the fix.
4. **`rigger/cli.py`** becomes wrappers: parse options, call the service with
   `log=console.print`, print the result. `rig run` chains the services.
   `rig` with no subcommand opens the TUI (`invoke_without_command=True`);
   `rig tui` also works; `rig --help` still lists every command.
5. **Tests** `tests/test_services.py`: `analyse` with `FakeLLM`; `execute`
   skips a held instrument with reason "already long"; `since` filtering;
   `resolve_evidence` on mixed ids; `setup_checks` flags a missing key.

Exit: `pytest` green, `rig run` output unchanged.

## Step 2 — Screens

Package `rigger/tui/`: `app.py` (App, screen registry, bindings), one module
per screen in `screens/`, shared widgets in `widgets.py`, styles in
`rigger.tcss`. Screens never touch SQL; they call `rigger.services`.

| Key | Screen | Content |
|---|---|---|
| `1` | Home | Pipeline diagram (the README block) as a panel. Setup checks with green/red marks and the fix for each red one. "Run the daily pipeline" button. Last-run summary per step. When there are no bars yet, a three-step first-run card (add a key to `.env`, Ingest, Analyse) replaces the summary. |
| `2` | Signals | Left: `DataTable` of signals (date, instrument, strategy, direction, conviction, model, cost) with strategy and date filters. Right: thesis, invalidation, horizon; a **critic** block (verdict, counter-argument, risks, original vs revised conviction) and an **ensemble** block (per-model votes, dispersion) when present in `metadata`; evidence grouped by kind with counts, expandable to news titles, events and fundamentals. `b` opens the rendered brief in a modal. |
| `3` | Portfolio | Positions (qty, avg price with currency, value in base), cash, equity, recent fills. `r` resets with a confirm modal and a "also delete signals" checkbox. |
| `4` | Pipeline | One row per step with a Run button and options (since date, strategy checkboxes, dry-run toggle). A `RichLog` pane receives `log()` output. Steps run as `@work` async workers so the UI stays responsive; Signals and Portfolio refresh when a worker finishes. "Run all" chains the five steps. |
| `5` | Data & Costs | Row counts per table, latest bar per instrument, FX rate, news by source, events by kind, LLM spend by task and model with a since filter. |
| `6` | Config | Plugin table with enabled toggles (writes `config.toml` via `set_plugin_enabled`), universe, model routing, risk limits, provider and whether its key is set. Read-only apart from toggles. |

Order: Home and Signals first (the priority views), then the rest.

## Step 3 — Tests and docs

- `tests/test_tui.py` with `App.run_test()` on a seeded `tmp_engine` and
  `FakeLLM`: the app mounts; `2` shows the seeded signals; selecting a row
  renders its thesis and evidence counts; `4` then Analyse fills the log and
  adds rows. No network.
- README: "Run" leads with `uv run rig` opening the TUI; CLI commands move
  under "Scripting"; layout gains `runtime.py`, `services.py`, `tui/`.
- `PROJECT_SPEC.md` §11: TUI is the primary interface, CLI the scripting
  surface.

## Rules

- Conventional commits, one commit per step above.
- Offline tests only; `uv run pytest`, `ruff check .`, `ruff format --check .`,
  `mypy rigger/core rigger/llm rigger/paper rigger/services.py` clean.
- No financial-advice language anywhere in the UI. Every rate shown next to
  its sample size once Phase 3 lands.
- Screens call services; services never import Textual.

## Verification

```bash
uv sync
uv run pytest
uv run rig                     # Home with green checks against the live DB
uv run rig run                 # CLI path unchanged
uv run rig execute --all       # still prints skip reasons
```

In the TUI against `data/rigger.db`: Signals lists the stored signals;
selecting `ASX:CBA short` shows its thesis and evidence groups; `b` shows the
brief; Pipeline → Analyse (dry-run) streams progress without blocking;
Portfolio matches `rig paper status`.

## Exit criterion

`uv run rig` opens a TUI from which every current CLI operation can be
performed, a first-time user is told what to fix before their first run, and
any stored signal can be traced from thesis to the evidence rows behind it.

## Deferred

- Charts (sparklines of bars in the Signals detail pane).
- Live refresh while the Phase 4 scheduler runs in the background.
- Scorecard and backtest screens (Phase 3 output).
