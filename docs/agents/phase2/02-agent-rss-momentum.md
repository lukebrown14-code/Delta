# Phase 2 — Agent 2: RSS data plugin + momentum baseline

**Branch:** `phase2/B-rss` first, then `phase2/I-momentum`. Two PRs.

## Workstream B — RSS data plugin

Create `rigger/plugins/data/rss.py` with `class RSSData(DataPlugin)`, `name = "rss"`, `market = None`.

- Add `feedparser>=6.0` to dependencies (`uv add feedparser`).
- Feeds from the plugin's config table: `[plugins.rss].feeds = [...]`. Fetch each with `httpx` (so `respx` can mock it) and parse the bytes with `feedparser.parse`.
- Each entry becomes a `NewsItem`: `id` = sha256(link + published), `published` to UTC (fall back to now if missing), `title`, `url`, `body` = summary stripped of HTML, `source="rss"`. Skip entries older than `since`.
- Instrument matching: for each instrument in the passed list, match whole-word symbol (`\bAAPL\b`) or the first word of `instrument.name` (case-insensitive) against title + body. Unmatched items still get stored with `instrument_ids=[]` so macro news is kept.
- Register `rss = "rigger.plugins.data.rss:RSSData"`. Add `[plugins.rss]` with `enabled = true` and two or three real business feeds.
- Tests: `respx` serving a saved RSS XML file from `tests/fixtures/`; assert item count, matching to instruments, empty match kept, id stability, `since` filter, and a feed returning 500 is skipped without raising.

## Workstream I — Momentum baseline

Create `rigger/plugins/strategies/momentum.py` with `class Momentum(StrategyPlugin)`, `name = "momentum"`. No LLM.

- For each instrument load bars from `BarTable`. Need at least 253 bars, otherwise skip. Momentum = close[-22] / close[-253] - 1 (12-1 month, skipping the most recent month).
- Rank across the universe. Top 20 % → `direction="long"`, others → `"flat"`. `conviction` = rank percentile (0..1). `horizon_days=21`. `thesis` = one line with the momentum figure and rank. `invalidation` = "Falls out of top quintile of 12-1 momentum at next monthly rank." `evidence_ids` = the ids of the two bars used. `model=None`, `prompt_version=None`, `cost_usd=0`.
- Register `momentum = "rigger.plugins.strategies.momentum:Momentum"`. No config needed.
- Tests: seed `tmp_engine` with synthetic bars for five instruments with known trends; assert ranking, quintile cut, and skipping short histories.
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
