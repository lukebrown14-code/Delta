# Phase 3 — Step 0: Foundation

**Branch:** `phase3/00-foundation`. Run alone; Workstreams A–D start after it merges.

## Task

Make the pipeline replayable and give the evaluation workstreams one shared return calculation. Follow "Step 0 — Foundation" in `docs/PHASE3_PLAN.md` item by item:

1. `Context.as_of` and `Context.now()`. Replace every `datetime.now(UTC)` in `rigger/brief.py`, `rigger/plugins/strategies/{llm_analyst,critic,ensemble,momentum}.py` and `rigger/plugins/data/yfinance_calendar.py` with the context clock. `build_brief` callers pass `as_of=ctx.now()`. `yfinance_calendar.fetch` has no context; give it a `today: date | None` keyword defaulting to today and leave a note.
2. `Signal.run_id` (model + nullable indexed column via `_ADDED_COLUMNS`; round-trip in `_store_signal` and the report loader in `cli.py`).
3. `MarketPlugin.benchmark` for `us` (`US:^GSPC`) and `asx` (`ASX:^AXJO`). `rig ingest` adds each selected market's benchmark to the instruments passed to bar-fetching plugins (`yfinance` only; news/filings plugins must not see it). Benchmarks never enter `ctx.universe`.
4. `rigger/eval/__init__.py`, `rigger/eval/returns.py` with `HorizonReturn` and `horizon_return(session, instrument_id, after, horizon_days)` exactly as specified. Trading-bar horizon. Tests in `tests/test_returns.py`: normal case, gap in bars, exit bar not yet available returns `None`, `after` exactly on a bar timestamp picks the next bar.
5. CLI stubs: `rig evaluate`, `rig scorecard`, `rig attribution`, `rig backtest` with the options listed in the plan, each printing "not implemented" and exiting 0.
6. `Report.scorecard: dict[str, Any] | None = None`.
7. `[llm].max_concurrency` (default 4) read in `core/config.py`; `LLMClient.complete` acquires an `asyncio.Semaphore` of that size around the provider call only (not around the cache lookup).
8. Update `docs/agents/phase3/0[1-4]-*.md` if any signature you shipped differs from the plan.

## Exit

`pytest` green including the new returns tests. `rig run` behaves as before. A test in `tests/test_pipeline.py` constructs `Context(as_of=<past date>)`, runs `LLMAnalyst` and `Momentum`, and asserts every signal `ts` equals `as_of` and no bar after `as_of` appears in the brief's evidence.
## Ground rules (same for every Phase 3 agent)

- Repo: Rigger. Read `PROJECT_SPEC.md` §10, `docs/PHASE3_PLAN.md` (your workstream section and the shared signatures) and skim `rigger/eval/returns.py`, `rigger/core/plugin.py` (`Context.as_of`, `Context.now()`), `rigger/core/db.py` (`EvaluationTable`, `SignalTable.run_id`, migration hooks, `store_items`) and `tests/conftest.py` (`FakeLLM`, `FakeConfig`, `seed_bars`).
- Work in a git worktree on the branch named below. One PR. Conventional commits. Never push `main`.
- Do NOT edit `rigger/core/plugin.py`, `rigger/core/models.py`, `rigger/core/db.py`, `rigger/brief.py`, `rigger/eval/returns.py` or any file under `rigger/plugins/strategies/`. In `rigger/cli.py` replace only the body of your own stub command. If you need a change elsewhere, stop and report exactly what and why.
- Offline tests only. Seed tables directly or use `seed_bars`. Before opening the PR, all of these must be clean:
  `uv run pytest` · `uv run ruff check .` · `uv run ruff format --check .` · `uv run mypy rigger/core rigger/llm rigger/eval`
- Every printed rate or mean is accompanied by its sample size. No language that reads as investment advice.
- Finish with a short report: what was built, the CLI commands to exercise it, and anything left out and why.
