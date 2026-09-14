# Phase 2 — Agent 4: Event extraction + calendar

**Branch:** `phase2/E-extract` first, then `phase2/F-calendar`. Two PRs. Both write `Event` rows; the brief's events and calendar sections (already built in Step 0) read them from `EventTable`. You do not touch the brief.

## Workstream E — Event extraction

1. **Prompt** `rigger/llm/prompts/extract_v1.j2`. Inputs: `symbol`, `instrument_id`, `items` (list of `{id, published, source, title, body}`). Ask for a JSON object `{"events": [...]}` where each event has `kind` (one of the `Event.kind` literals), `summary` (one sentence, factual), `sentiment` (-1..1), `evidence_ids` (subset of the provided item ids). Include the facts-only instruction and tell the model to return an empty list when nothing is a discrete event.
2. **Module** `rigger/extract.py`: `async def extract_events(ctx: Context, since: datetime, batch_size: int = 20) -> list[Event]`.
   - Find `NewsItemTable` rows with `published >= since` whose id does not appear in any `EventTable.evidence_ids`.
   - Group by instrument id (items with several instruments go to each; items with none are skipped for now).
   - Per group, in batches of `batch_size`, call `structured()` with task `"extract"`, model `ctx.config.llm_routing["extract"]`, schema a Pydantic `EventBatch` model. On `ValidationError` log and continue.
   - Store `Event` rows: `id` = sha256(instrument_id + kind + summary), `ts` = latest `published` among the evidence, `extracted_by` = model, `prompt_version = "extract_v1"`. Skip duplicates by id.
3. **CLI.** Fill the body of the existing `rig extract` stub in `rigger/cli.py` — this is the one permitted edit to that file: replace the stub body with a call to `extract_events` and a Rich summary line. Add `--since` defaulting to 14 days ago.
4. **Tests** `tests/test_extract.py`: seed news rows, `FakeLLM` returning canned events for task `"extract"`; assert rows stored, already-covered items are not re-sent, invalid output is skipped, dedup by id.

## Workstream F — Calendar

Create `rigger/plugins/data/yfinance_calendar.py` with `class YFinanceCalendar(DataPlugin)`, `name = "yfinance_calendar"`, `market = None`.

- For each instrument, `yfinance.Ticker(symbol).calendar` (use the market plugin's `yf_symbol` if present, else `symbol`). Wrap in `asyncio.to_thread`. Extract next earnings date and ex-dividend date when present.
- Emit `Event` rows: `kind="earnings"` / `"dividend"`, `ts` the future date at 00:00 UTC, `summary` "Earnings expected 2026-10-28" / "Ex-dividend 2026-09-30", `sentiment=0.0`, `evidence_ids=[]`, `extracted_by="yfinance"`, `prompt_version="n/a"`, `id` = sha256(instrument_id + kind + date). Return them from `fetch`; check how `rig ingest` persists fetch results and confirm it handles `Event` — if it only handles Bar/NewsItem/Fundamental, report that rather than editing the CLI.
- Register the entry point, add `[plugins.yfinance_calendar]`.
- Tests: monkeypatch `yfinance.Ticker` to return a fake calendar; assert two events with correct kinds and future timestamps, and that a ticker with no calendar yields nothing.
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
