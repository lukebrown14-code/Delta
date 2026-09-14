# Phase 3 — Agent 3: Attribution

**Branch:** `phase3/C-attribution`. One PR.

Build Workstream C from `docs/PHASE3_PLAN.md`.

- `rigger/eval/attribution.py`:
  - `evidence_sources(session, evidence_ids: list[str]) -> set[str]`: bar ids (numeric strings) → `"prices"`; `"fundamental:<id>"` → look up the row's `source` → `"fundamental:<source>"`; ids found in `NewsItemTable` → `"news:<source>"`; ids found in `EventTable` → `"event:<kind>"`. One query per table per call, not per id. Agent 2 ships the same function for the scorecard; whichever merges second deletes its copy and imports the other.
  - `@dataclass SourceEffect`: `source, n_with, n_without, mean_with, mean_without, diff, t_stat`.
  - `attribution(engine, *, min_n: int = 10, run_id: str | None = None) -> list[SourceEffect]`: over evaluated signals (join `SignalTable`/`EvaluationTable`), compute per source label the mean excess return with and without that source in the signal's evidence, the difference, and a Welch t-statistic implemented with `statistics` (no scipy). Drop labels where either side has fewer than `min_n`. Sort by `t_stat` descending.
- Fill `rig attribution [--min-n 10] [--json] [--run-id ID]`: Rich table with both sample sizes; mark rows with `abs(t_stat) < 1` as "no measurable contribution" and rows with `t_stat < -2` as "negative"; `--json` output.
- Tests `tests/test_attribution.py`: construct 60 evaluated signals where evidence containing `news:rss` has excess drawn around +2 % and others around 0 %, plus a noise source `event:other` split evenly; assert `news:rss` ranks first with positive diff and t > 2, the noise source has `abs(t) < 1`, `min_n` filtering removes a rare label, and `evidence_sources` maps each id class correctly.
## Ground rules (same for every Phase 3 agent)

- Repo: Rigger. Read `PROJECT_SPEC.md` §10, `docs/PHASE3_PLAN.md` (your workstream section and the shared signatures) and skim `rigger/eval/returns.py`, `rigger/core/plugin.py` (`Context.as_of`, `Context.now()`), `rigger/core/db.py` (`EvaluationTable`, `SignalTable.run_id`, migration hooks, `store_items`) and `tests/conftest.py` (`FakeLLM`, `FakeConfig`, `seed_bars`).
- Work in a git worktree on the branch named below. One PR. Conventional commits. Never push `main`.
- Do NOT edit `rigger/core/plugin.py`, `rigger/core/models.py`, `rigger/core/db.py`, `rigger/brief.py`, `rigger/eval/returns.py` or any file under `rigger/plugins/strategies/`. In `rigger/cli.py` replace only the body of your own stub command. If you need a change elsewhere, stop and report exactly what and why.
- Offline tests only. Seed tables directly or use `seed_bars`. Before opening the PR, all of these must be clean:
  `uv run pytest` · `uv run ruff check .` · `uv run ruff format --check .` · `uv run mypy rigger/core rigger/llm rigger/eval`
- Every printed rate or mean is accompanied by its sample size. No language that reads as investment advice.
- Finish with a short report: what was built, the CLI commands to exercise it, and anything left out and why.
