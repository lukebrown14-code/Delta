# Phase 2 — Step 0: Foundation

**Branch:** `phase2/00-foundation`. Run this alone. Workstreams A–I start only after this merges.

## Task

Define the seam every Phase 2 workstream plugs into, so six agents can then work concurrently without touching the same files.

1. **Create `rigger/brief.py`.**
   - `@dataclass class Section`: `title: str`, `lines: list[str]`, `evidence_ids: list[str]`.
   - `@dataclass class Brief` with sections `prices`, `news`, `events`, `fundamentals`, `calendar`, plus `instrument: Instrument`, `as_of: datetime`. Methods: `render() -> str` (a section with no lines renders as `<Title>: none available`) and property `evidence_ids -> list[str]` (ordered union, deduped).
   - `build_brief(ctx: Context, instrument: Instrument, as_of: datetime | None = None) -> Brief | None`. Returns `None` if there are no bars. One private function per section, each reading only its own table: `_prices` from `BarTable` (move the existing logic from `LLMAnalyst._build_brief` verbatim, same numbers, same wording), `_news` from `NewsItemTable` (last 14 days, newest first, max 20, format `YYYY-MM-DD [source] title`), `_events` from `EventTable` where `ts <= as_of` (last 14 days, format `YYYY-MM-DD kind: summary (sentiment +0.4)`), `_fundamentals` from `FundamentalTable` (latest value per metric), `_calendar` from `EventTable` where `ts > as_of` (format `YYYY-MM-DD kind: summary`).
2. **Refactor `rigger/plugins/strategies/llm_analyst.py`.** Extract `async def analyse_one(ctx, inst, model, brief: Brief) -> Signal | None` at module level. It renders the prompt, calls `structured()`, returns the `Signal` or `None` on `ValidationError`. `LLMAnalyst.generate` becomes: build brief, skip if `None`, call `analyse_one`. Keep `SignalDraft` and `ANALYST_TEMPLATE` public.
3. **Add `metadata: dict[str, Any] = Field(default_factory=dict)`** to `Signal` in `rigger/core/models.py` and a JSON column (`sa_column=Column(JSON)`) on `SignalTable` in `rigger/core/db.py`. Update the store/load helpers in `cli.py` so metadata round-trips.
4. **Add a `rig extract` CLI stub** in `rigger/cli.py`: a command that logs "extract: no extractor configured" and returns. Call it in `rig run` between ingest and analyse.
5. **Tests.** `tests/test_brief.py`: seed `tmp_engine` with rows in all five tables, assert each section renders and `evidence_ids` contains ids from every table. Assert the rendered price section is byte-identical to what Phase 1 produced (capture the old output in the test before refactoring). `tests/test_pipeline.py` must still pass.

## Exit

`pytest` green, `rig run` still works on the US five, brief price section unchanged, `Signal.metadata` persists.
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
