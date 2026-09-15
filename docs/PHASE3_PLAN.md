# Phase 3 Plan — Evaluation Loop

> Split of Phase 3 (`PROJECT_SPEC.md` §10, §13) into a foundation step and four
> parallel workstreams. Agent prompts live in `docs/agents/phase3/`. Same
> working rules as Phase 2: no behaviour change without a test, offline
> `pytest`, one PR per workstream.

## Goal

Every signal Rigger has ever produced gets scored against what actually
happened, broken down by strategy, model, prompt version, market, sector and
evidence source, so the harness can tell which parts are predictive. The same
machinery replays history offline so a new strategy or prompt can be judged
before it trades.

## What exists

- `Evaluation` model and `EvaluationTable` (signal_id, horizon_return, hit, benchmark_return, excess_return). Never written.
- `LLMClient` caches by sha256(model, prompt_version, prompt) in `LLMCallTable`, so a replay with an identical brief costs nothing.
- `build_brief(ctx, inst, as_of)` already accepts `as_of`; every caller passes `None`.
- `Signal.metadata` JSON column and the `_ADDED_COLUMNS` / `_ADDED_UNIQUE_INDEXES` migration hooks in `core/db.py`.
- `[schedule].evaluate = "0 18 * * 5"` in config, unused until Phase 4.

## What blocks parallel work (Step 0)

1. **No replay clock.** `llm_analyst`, `critic`, `ensemble`, `momentum`, `yfinance_calendar` and `brief` all read `datetime.now(UTC)`. Backtest cannot exist until strategies take their "now" from the context.
2. **No benchmark.** `excess_return` needs an index series per market. Nothing ingests one.
3. **No way to separate replayed signals from live ones.** Backtest signals in `SignalTable` would leak into the live scorecard.
4. **Return lookup is needed by evaluate, backtest and attribution.** If three agents each write "close at entry, close N bars later" they will disagree.

## Step 0 — Foundation (one agent, run first)

1. **`Context.as_of: datetime | None = None`** in `core/plugin.py`, plus `Context.now() -> datetime` returning `as_of or datetime.now(UTC)`. Replace every `datetime.now(UTC)` in `brief.py`, the four strategies and `yfinance_calendar.py` with `ctx.now()`. Signal `ts` becomes `ctx.now()`. `build_brief` callers pass `as_of=ctx.now()`.
2. **`Signal.run_id: str | None = None`** on the model and a nullable indexed column on `SignalTable` via `_ADDED_COLUMNS`. Live pipeline leaves it `None`; backtest stamps a run id. `_store_signal` round-trips it.
3. **`MarketPlugin.benchmark: Instrument | None`** — `US:^GSPC` (S&P 500) for `us`, `ASX:^AXJO` (S&P/ASX 200) for `asx`. `rig ingest` fetches bars for each market's benchmark alongside the universe (yfinance handles `^` symbols). Benchmarks are never in `ctx.universe`.
4. **`rigger/eval/__init__.py` and `rigger/eval/returns.py`** with one pure function everything shares:
   ```python
   @dataclass
   class HorizonReturn:
       entry_ts: datetime; entry_close: float
       exit_ts: datetime; exit_close: float
       @property
       def ret(self) -> float: ...

   def horizon_return(session, instrument_id: str, after: datetime, horizon_days: int) -> HorizonReturn | None:
       """Entry = first bar with ts > after (next session). Exit = the bar horizon_days bars later. None if the exit bar does not exist yet."""
   ```
   `horizon_days` counts trading bars, not calendar days. Tests with synthetic bars including a gap.
5. **CLI stubs** in `cli.py`: `rig evaluate`, `rig scorecard [--by ...] [--json]`, `rig attribution`, `rig backtest --from DATE [--to DATE] [--strategy ...] [--no-llm]`. Each logs "not implemented" and exits 0. Agents fill bodies only.
6. **`Report.scorecard: dict | None = None`** on the report dataclass so the Markdown plugin can render a section when present.
7. **LLM concurrency guard.** `asyncio.Semaphore` in `LLMClient.complete` sized from `[llm].max_concurrency` (default 4). Backtest will otherwise fire hundreds of calls at once on a cache miss.
8. Existing tests still pass with `as_of=None`. Add `tests/test_returns.py`.

Exit: `pytest` green, `rig run` unchanged for the live path, `Context(as_of=X)` makes analyst, momentum and brief produce output as of `X`.

## Workstreams (after Step 0)

| ID | Workstream | Owns | Reads |
|---|---|---|---|
| A | Evaluate | `rigger/eval/evaluate.py`, body of `rig evaluate` | `eval/returns.py` |
| B | Scorecard + calibration | `rigger/eval/scorecard.py`, body of `rig scorecard`, scorecard section in `plugins/reports/markdown.py` | `EvaluationTable`, `SignalTable` |
| C | Attribution | `rigger/eval/attribution.py`, body of `rig attribution` | `EvaluationTable`, evidence tables |
| D | Backtest | `rigger/eval/backtest.py`, body of `rig backtest` | everything above, via imports |

### A — Evaluate
- `evaluate_signals(engine, markets, as_of=None) -> list[Evaluation]`: for every live signal (`run_id IS NULL`) with no evaluation row and `direction != "flat"`, call `horizon_return` for the instrument and for its market's benchmark over the same window. `hit` = sign(horizon_return) matches direction. `excess_return = horizon_return - benchmark_return` (for shorts, negate both first). Skip if either return is `None` (not aged yet). Insert `Evaluation` rows idempotently (unique on `signal_id`; add via `_ADDED_UNIQUE_INDEXES`).
- Flat signals get evaluated too, with `hit = abs(horizon_return) < flat_band` (config `[eval].flat_band_pct = 2`). They matter for calibration.
- `rig evaluate [--as-of DATE]` prints counts: evaluated, still aging, skipped for missing bars.
- Tests: seed bars for an instrument and its benchmark, seed signals long/short/flat, assert returns, hit logic, idempotency, and that a signal whose exit bar is missing is left alone.

### B — Scorecard and calibration
- `scorecard(engine, by: str, run_id: str | None = None) -> list[Row]` where `by ∈ {strategy, model, prompt, market, sector, source}`. Row: group key, n, hit_rate, mean_excess, sharpe (mean/stdev of excess, annualised by horizon), max_drawdown of cumulative excess in signal order, mean_conviction.
- `calibration(engine, buckets=5, run_id=None) -> list[Bucket]`: conviction decile → n, mean_conviction, hit_rate. A well-calibrated strategy has hit_rate ≈ conviction.
- `by=source` groups on the evidence source of each signal (join through `evidence_ids`; reuse Workstream C's `evidence_sources()` if it lands first, otherwise a local version with the same signature and C reconciles).
- `rig scorecard --by strategy` Rich table; `--json` machine output; `--calibration` prints the buckets.
- Markdown report: when `report.scorecard` is set, render "## Scorecard" with the strategy table and calibration buckets. `rig report --with-scorecard` populates it.
- Tests: seed `SignalTable` + `EvaluationTable` directly; assert each metric on a small hand-computed set, including a run_id filter.

### C — Attribution
- `evidence_sources(session, evidence_ids) -> set[str]`: maps ids to source labels: bar ids → `"prices"`, `fundamental:<id>` → `"fundamental:<source>"`, news ids → `"news:<source>"`, event ids → `"event:<kind>"`.
- `attribution(engine, min_n=10) -> list[SourceEffect]`: for each source label, mean excess return of evaluated signals whose evidence includes it vs those whose evidence does not, difference, Welch t-statistic, n_with, n_without. Sorted by t. No scipy: implement Welch by hand with `statistics`.
- `rig attribution [--min-n N] [--json]` Rich table; flag sources with t < 1 as "no measurable contribution".
- Tests: synthetic evaluations where one source is constructed to be predictive and another is noise; assert ordering and sign.

### D — Backtest
- `backtest(rig, start, end, strategies, run_id, use_llm=True) -> BacktestResult`:
  - For each session day `d` in `[start, end]` (days with at least one bar in the universe): `ctx = rig.context(universe); ctx.as_of = d at 23:59 UTC`. Run each strategy. Stamp `run_id`, store signals.
  - Strategies see only data with `ts <= as_of` because `build_brief`, momentum and the calendar section already filter on `as_of` (Step 0). Verify with a test that a bar dated after `as_of` never appears in a brief.
  - `--no-llm` restricts to strategies without an LLM (`momentum`). With LLM, cache hits are free; a miss costs money, so print the estimated number of misses and ask for confirmation unless `--yes`.
  - After the loop, evaluate run signals with `evaluate_signals(..., run_id=run_id, as_of=end)` — coordinate with A so the function accepts `run_id`.
  - Print `scorecard(by="strategy", run_id=run_id)` and calibration.
  - Equity curve: equal notional per non-flat signal, held for its horizon, cumulative excess return by day. Written to `reports/backtest-<run_id>.md` via the Markdown plugin's scorecard section.
- `rig backtest --from 2025-01-01 --to 2025-06-30 --strategy momentum --no-llm` completes offline against ingested bars.
- Tests: seed 300 synthetic bars for three instruments plus a benchmark, run `momentum` over a 20-day window with `FakeLLM` unused, assert one signal per instrument per day, all stamped with `run_id`, none leaking into `scorecard(run_id=None)`, and evaluations present for aged signals.

## Agent assignment (four agents)

| Agent | Workstream |
|---|---|
| 1 | A Evaluate |
| 2 | B Scorecard + calibration + report section |
| 3 | C Attribution |
| 4 | D Backtest |

D depends on A's `evaluate_signals(run_id=...)` and B's `scorecard(run_id=...)` at integration time only. D develops against the signatures above and imports them; if a name differs at merge, the integrator reconciles.

## Rules (unchanged from Phase 2)

- Own worktree, branch `phase3/<id>-<name>`, one PR, conventional commits, never push `main`.
- Do not edit `core/plugin.py`, `core/models.py`, `core/db.py`, `brief.py`, `eval/returns.py`, or any strategy. Fill only your CLI stub body in `cli.py`.
- Offline tests with `respx` and `FakeLLM`; `uv run pytest`, `ruff check .`, `ruff format --check .`, `mypy rigger/core rigger/llm rigger/eval` clean.
- No financial-advice language in output. Scorecard output states sample sizes next to every rate.

## Integration

1. Merge A, B, C, D. Reconcile `evidence_sources` if both B and C shipped one.
2. `rig ingest --since 2024-09-01` (a year plus horizon), `rig backtest --from 2025-06-01 --to 2025-08-31 --strategy momentum --no-llm`, confirm a scorecard prints with non-zero n.
3. `rig evaluate` then `rig scorecard --by strategy` on the live book.
4. README: new commands and Phase 3 marked done.

## Exit criterion

`rig evaluate` scores every aged live signal against its market benchmark. `rig scorecard --by strategy|model|prompt|source` prints hit rate, mean excess return, Sharpe and drawdown with sample sizes, and `--calibration` shows conviction versus realised hit rate. `rig attribution` ranks evidence sources by measured contribution. `rig backtest --no-llm` replays momentum over stored history offline and produces the same scorecard for the run, with no leakage of future bars into any brief.

## Deferred to Phase 4+

- Scheduling the weekly evaluate and briefing (`core/scheduler.py`).
- Per-run brief cache on `Context` so critic does not rebuild the analyst's brief.
- Prompt promotion: auto-switch `[llm.routing]` to the best `prompt_version`.
