# Phase 2 — Agent 5: Critic strategy

**Branch:** `phase2/G-critic`. One PR.

## Workstream G — Critic

Create `rigger/plugins/strategies/critic.py` with `class Critic(StrategyPlugin)`, `name = "critic"`.

- Config: `[plugins.critic].wraps = "llm_analyst"`. In `generate`, look up `ctx.plugins[wraps]` and call its `generate(ctx)` to get the base signals.
- For each base signal: rebuild the brief with `build_brief(ctx, instrument)` from `rigger/brief.py`, then call `structured()` with task `"critique"`, model `ctx.config.llm_routing["critique"]`, template `critic_v1.j2`, schema `CritiqueDraft` with fields `counter_argument: str`, `risks: list[str]`, `revised_conviction: float (0..1)`, `verdict: Literal["hold","reduce","reject"]`.
- Produce a new `Signal` (new id, `ts` now) copying the base fields, with `strategy = f"critic:{wraps}"`, `conviction = revised_conviction`, `direction = "flat"` when `verdict == "reject"`, and `metadata["critic"] = {base_signal_id, original_conviction, counter_argument, risks, verdict, model, prompt_version: "critic_v1"}`. `cost_usd` = base cost + critique cost. Both base and critic signals are returned so both are stored and can be compared in Phase 3.
- Prompt `rigger/llm/prompts/critic_v1.j2`: inputs `symbol`, `brief`, `direction`, `conviction`, `thesis`, `invalidation`. Instruct the model to argue against the thesis as strongly as the evidence allows, using only the brief, then give a revised conviction. Include the facts-only instruction.
- Register `critic = "rigger.plugins.strategies.critic:Critic"`. Add `[plugins.critic]` with `enabled = true`, `wraps = "llm_analyst"`.
- Tests: `FakeLLM` answering both `"analyse"` and `"critique"` tasks; assert two signals per instrument, metadata populated, reject → flat, conviction overwritten, cost summed. Also test that a missing `wraps` plugin raises a clear error.
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
