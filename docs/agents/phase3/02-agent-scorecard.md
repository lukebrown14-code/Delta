# Phase 3 — Agent 2: Scorecard, calibration and report section

**Branch:** `phase3/B-scorecard`. One PR.

Build Workstream B from `docs/PHASE3_PLAN.md`.

- `rigger/eval/scorecard.py`:
  - `@dataclass Row`: `key: str, n: int, hit_rate: float, mean_excess: float, sharpe: float | None, max_drawdown: float, mean_conviction: float`.
  - `scorecard(engine, by: Literal["strategy","model","prompt","market","sector","source"], run_id: str | None = None) -> list[Row]`. Join `SignalTable` to `EvaluationTable` on `signal_id`; group; Sharpe = mean(excess)/stdev(excess) × sqrt(252/mean horizon), `None` when n < 2 or stdev = 0; drawdown on the cumulative sum of excess in `ts` order. `market` is the instrument id prefix; `sector` comes from `InstrumentTable` (fall back to `"unknown"`).
  - `by="source"`: a signal contributes to every source label in its evidence. Implement `evidence_sources(session, evidence_ids: list[str]) -> set[str]` with the label scheme in the plan (`prices`, `fundamental:<source>`, `news:<source>`, `event:<kind>`). Agent 3 ships the same function; whichever merges second deletes its copy and imports the other.
  - `calibration(engine, buckets: int = 5, run_id: str | None = None) -> list[Bucket]` with `Bucket(lo, hi, n, mean_conviction, hit_rate)`; equal-width conviction buckets; flat signals included.
- Fill `rig scorecard [--by strategy] [--json] [--calibration] [--run-id ID]`: Rich table with n in its own column; `--json` dumps rows as a list of dicts; `--calibration` prints buckets instead.
- `rigger/plugins/reports/markdown.py`: when `report.scorecard` is not `None`, render "## Scorecard" with a strategy table and a calibration table. Expect the dict shape `{"rows": [Row as dict], "calibration": [Bucket as dict], "by": "strategy"}`. Add `--with-scorecard` to `rig report` that fills it (this is your one extra CLI edit).
- Tests `tests/test_scorecard.py`: seed six signals across two strategies with hand-computed evaluations; assert every Row field; assert `run_id` filtering; calibration buckets on a designed set where high conviction hits more; source grouping counts a signal once per label; the report renders the section.
## Ground rules (same for every Phase 3 agent)

- Repo: Rigger. Read `PROJECT_SPEC.md` §10, `docs/PHASE3_PLAN.md` (your workstream section and the shared signatures) and skim `rigger/eval/returns.py`, `rigger/core/plugin.py` (`Context.as_of`, `Context.now()`), `rigger/core/db.py` (`EvaluationTable`, `SignalTable.run_id`, migration hooks, `store_items`) and `tests/conftest.py` (`FakeLLM`, `FakeConfig`, `seed_bars`).
- Work in a git worktree on the branch named below. One PR. Conventional commits. Never push `main`.
- Do NOT edit `rigger/core/plugin.py`, `rigger/core/models.py`, `rigger/core/db.py`, `rigger/brief.py`, `rigger/eval/returns.py` or any file under `rigger/plugins/strategies/`. In `rigger/cli.py` replace only the body of your own stub command. If you need a change elsewhere, stop and report exactly what and why.
- Offline tests only. Seed tables directly or use `seed_bars`. Before opening the PR, all of these must be clean:
  `uv run pytest` · `uv run ruff check .` · `uv run ruff format --check .` · `uv run mypy rigger/core rigger/llm rigger/eval`
- Every printed rate or mean is accompanied by its sample size. No language that reads as investment advice.
- Finish with a short report: what was built, the CLI commands to exercise it, and anything left out and why.
