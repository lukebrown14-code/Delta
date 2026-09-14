# Phase 3 — Agent 4: Backtest

**Branch:** `phase3/D-backtest`. One PR. Largest workstream; the equity curve is the first thing to cut if time runs out.

Build Workstream D from `docs/PHASE3_PLAN.md`.

- `rigger/eval/backtest.py`:
  - `session_days(engine, instrument_ids, start: date, end: date) -> list[date]`: dates with at least one bar in the universe.
  - `async def run_backtest(rig, *, start: date, end: date, strategies: list[str], run_id: str, use_llm: bool) -> BacktestResult`. For each session day: `ctx = rig.context(universe); ctx.as_of = datetime(d, 23:59:59, UTC)`; run each named strategy plugin's `generate(ctx)`; set `signal.run_id = run_id`; store with `_store_signal` (import from `rigger.cli`). `use_llm=False` refuses any strategy other than `momentum` with a clear message.
  - After the loop: `evaluate_signals(engine, markets, run_id=run_id, as_of=end)` from `rigger.eval.evaluate`, then `scorecard(engine, by="strategy", run_id=run_id)` and `calibration(engine, run_id=run_id)` from `rigger.eval.scorecard`. Until those branches merge, code against the signatures in the plan and keep imports at module top so a mismatch fails loudly at import time.
  - Equity curve: equal notional per non-flat signal, each held for `horizon_days` bars, daily cumulative excess return. `BacktestResult` carries `run_id, days, signals: int, rows, calibration, curve: list[tuple[date, float]]`.
  - Cost preview: before running with `use_llm=True`, count instruments × days × strategies and print the estimate of LLM calls; proceed only with `--yes` or interactive confirmation.
- Fill `rig backtest --from DATE [--to DATE] [--strategy momentum] [--no-llm] [--yes]`: `run_id = f"bt-{start}-{end}-{uuid4().hex[:6]}"`; print the scorecard and calibration via Rich; write `reports/backtest-<run_id>.md` through the Markdown report plugin with `Report.scorecard` filled (`signals`, `orders`, `fills`, `positions` empty, `cash` 0).
- Leakage guard: a test that seeds bars beyond `as_of` and asserts no signal's `evidence_ids` references a bar with `ts > as_of`. This is the property the whole backtest rests on.
- Tests `tests/test_backtest.py`: 300 synthetic bars for three instruments plus `US:^GSPC`; run `momentum` over 20 session days with `use_llm=False`; assert one signal per instrument per day, all with `run_id`, `scorecard(run_id=None)` sees none of them, evaluations exist for signals older than their horizon, and the leakage guard above. Stub `evaluate_signals`/`scorecard` with monkeypatch if those branches have not merged when you test.
## Ground rules (same for every Phase 3 agent)

- Repo: Rigger. Read `PROJECT_SPEC.md` §10, `docs/PHASE3_PLAN.md` (your workstream section and the shared signatures) and skim `rigger/eval/returns.py`, `rigger/core/plugin.py` (`Context.as_of`, `Context.now()`), `rigger/core/db.py` (`EvaluationTable`, `SignalTable.run_id`, migration hooks, `store_items`) and `tests/conftest.py` (`FakeLLM`, `FakeConfig`, `seed_bars`).
- Work in a git worktree on the branch named below. One PR. Conventional commits. Never push `main`.
- Do NOT edit `rigger/core/plugin.py`, `rigger/core/models.py`, `rigger/core/db.py`, `rigger/brief.py`, `rigger/eval/returns.py` or any file under `rigger/plugins/strategies/`. In `rigger/cli.py` replace only the body of your own stub command. If you need a change elsewhere, stop and report exactly what and why.
- Offline tests only. Seed tables directly or use `seed_bars`. Before opening the PR, all of these must be clean:
  `uv run pytest` · `uv run ruff check .` · `uv run ruff format --check .` · `uv run mypy rigger/core rigger/llm rigger/eval`
- Every printed rate or mean is accompanied by its sample size. No language that reads as investment advice.
- Finish with a short report: what was built, the CLI commands to exercise it, and anything left out and why.
