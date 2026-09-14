# Phase 2 — Agent 6: Ensemble strategy

**Branch:** `phase2/H-ensemble`. One PR.

## Workstream H — Ensemble

Create `rigger/plugins/strategies/ensemble.py` with `class Ensemble(StrategyPlugin)`, `name = "ensemble"`.

- Models from `ctx.config.llm_ensemble_models` (check the exact attribute name in `rigger/core/config.py`; it is loaded from `[llm.ensemble].models`). Require at least two, else raise a clear error at `generate`.
- For each instrument: `brief = build_brief(ctx, inst)`; skip if `None`. Run `analyse_one(ctx, inst, model, brief)` from `rigger.plugins.strategies.llm_analyst` once per model, concurrently with `asyncio.gather`. Drop `None` results. Need at least two valid answers to emit a signal.
- Combine: `direction` = majority vote, tie → `"flat"`. `conviction` = mean of convictions from models that agreed with the majority direction. `horizon_days` = median. `thesis` = "Ensemble of N models: k long, m short, j flat." followed by the majority model's thesis. `invalidation` = from the highest-conviction agreeing model. `evidence_ids` = brief evidence ids. `model = "ensemble"`, `prompt_version` = analyst template version, `cost_usd` = sum.
- `metadata["ensemble"] = {models: [...], directions: [...], convictions: [...], dispersion: stdev(convictions), agreement: fraction agreeing with majority}`.
- `strategy = "ensemble"`. Register `ensemble = "rigger.plugins.strategies.ensemble:Ensemble"`. Add `[plugins.ensemble]` with `enabled = true`.
- Tests: extend `FakeLLM` usage so it returns a different answer per model (look at how `FakeLLM.complete` receives `model` and key responses on it; if it cannot, subclass it in your test file). Assert majority vote, tie → flat, dispersion, skip when only one model answers validly, and that the per-model calls are made concurrently (e.g. count calls).
## Ground rules (same for every Phase 2 agent)

- Repo: Rigger, an AI investment research and paper-trading harness. Read `PROJECT_SPEC.md` (§3 principles, §5 models, §6 plugin contracts, §7.4 prompt rules) and `docs/PHASE2_PLAN.md` before writing code. Skim `rigger/core/plugin.py`, `rigger/brief.py` and `tests/conftest.py`.
- Work in a git worktree on the branch named below. One PR. Conventional commit messages. Do not push to `main`.
- Do NOT edit `rigger/brief.py`, `rigger/plugins/strategies/llm_analyst.py`, `rigger/cli.py`, `rigger/core/models.py` or `rigger/core/db.py`. If you believe you need a change there, stop and report exactly what and why.
- In `pyproject.toml` touch only the `[project.entry-points."rigger.plugins"]` list (one line per plugin) and the dependency list if your workstream names a new dependency.
- Add a `[plugins.<name>]` table with `enabled = true` to `config.toml` for each plugin you create.
- Tests run offline: `respx` for HTTP, `FakeLLM` from `tests/conftest.py` for models. Before opening the PR, all of these must be clean:
  `uv run pytest` · `uv run ruff check .` · `uv run ruff format --check .` · `uv run mypy rigger/core rigger/llm`
- Facts come from the harness, not the model. Any prompt template must include: "Use only the information provided. Do not rely on prior knowledge of prices, news or events."
- Finish with a short report: what was built, how to exercise it from the CLI, anything left out and why.
