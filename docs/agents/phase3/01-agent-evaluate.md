# Phase 3 — Agent 1: Evaluate

**Branch:** `phase3/A-evaluate`. One PR.

Build Workstream A from `docs/PHASE3_PLAN.md`.

- `rigger/eval/evaluate.py`: `evaluate_signals(engine, markets: dict[str, MarketPlugin], *, run_id: str | None = None, as_of: datetime | None = None, flat_band_pct: float = 2.0) -> EvaluateSummary` where the summary carries `evaluated: list[Evaluation]`, `aging: int`, `skipped: int`. Selection: signals with `run_id` equal to the argument (`None` means live), no row in `EvaluationTable`, and `ts <= as_of` when given.
- Returns via `horizon_return` from `rigger.eval.returns` for the instrument and for `markets[inst.market].benchmark` over the same `after=signal.ts, horizon_days`. Long: hit if return > 0. Short: hit if return < 0, and excess computed on negated returns. Flat: hit if `abs(return) < flat_band_pct/100`, excess = 0.
- Idempotent insert: add `("uq_evaluation_signal", "evaluation", ("signal_id",))` to `_ADDED_UNIQUE_INDEXES` — this is the one permitted `core/db.py` edit for this workstream — and insert with `on_conflict_do_nothing`.
- Fill `rig evaluate [--as-of YYYY-MM-DD]`: builds the market map from `rig.plugins`, calls `evaluate_signals`, prints evaluated / aging / skipped counts and, when any were evaluated, the mean excess return with n.
- Read `[eval].flat_band_pct` from config with the default above (add the key to `config.toml` with a comment; reading it needs a one-line addition in `core/config.py`, which is permitted for this key only).
- Tests `tests/test_evaluate.py`: seed 40 bars for `US:AAPL` and `US:^GSPC` with known closes; seed long, short and flat signals at day 10 with `horizon_days=5`; assert every field of each Evaluation by hand-computed values; run twice and assert no duplicates; a signal at day 38 is counted as aging; a signal on an instrument with no bars is skipped.
## Ground rules (same for every Phase 3 agent)

- Repo: Rigger. Read `PROJECT_SPEC.md` §10, `docs/PHASE3_PLAN.md` (your workstream section and the shared signatures) and skim `rigger/eval/returns.py`, `rigger/core/plugin.py` (`Context.as_of`, `Context.now()`), `rigger/core/db.py` (`EvaluationTable`, `SignalTable.run_id`, migration hooks, `store_items`) and `tests/conftest.py` (`FakeLLM`, `FakeConfig`, `seed_bars`).
- Work in a git worktree on the branch named below. One PR. Conventional commits. Never push `main`.
- Do NOT edit `rigger/core/plugin.py`, `rigger/core/models.py`, `rigger/core/db.py`, `rigger/brief.py`, `rigger/eval/returns.py` or any file under `rigger/plugins/strategies/`. In `rigger/cli.py` replace only the body of your own stub command. If you need a change elsewhere, stop and report exactly what and why.
- Offline tests only. Seed tables directly or use `seed_bars`. Before opening the PR, all of these must be clean:
  `uv run pytest` · `uv run ruff check .` · `uv run ruff format --check .` · `uv run mypy rigger/core rigger/llm rigger/eval`
- Every printed rate or mean is accompanied by its sample size. No language that reads as investment advice.
- Finish with a short report: what was built, the CLI commands to exercise it, and anything left out and why.
