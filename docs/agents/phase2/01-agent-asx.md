# Phase 2 — Agent 1: ASX market + ASX announcements

**Branch:** `phase2/A-asx-market` first, then `phase2/C-asx-announcements`. Two PRs.

## Workstream A — ASX market plugin

Create `rigger/plugins/markets/asx.py` with `class ASXMarket(MarketPlugin)`, `name = "asx"`, `currency = "AUD"`.

- `universe()`: read `config.universe["asx"]` (passed via `configure`; check how `USMarket` gets its tickers and mirror it). Instrument ids `ASX:<code>`, `market="asx"`, `sector=None` for now.
- `is_open(ts)` / `next_open(ts)`: Australia/Sydney, Mon–Fri 10:00–16:00, using `zoneinfo`. No holiday calendar yet; leave a TODO.
- `fee(notional: float) -> float`: 0.1 % with a $10 minimum. Check whether `rigger/plugins/brokers/paper.py` already looks for a `fee` method on the market plugin. If it does, nothing more. If not, report it rather than editing the broker.
- `yf_symbol(instrument) -> str`: `BHP` → `BHP.AX`. Check whether the yfinance data plugin asks the market for a symbol mapping. If not, report it.
- Register `asx_market = "rigger.plugins.markets.asx:ASXMarket"`. Add `asx = ["BHP","CBA","CSL","WES","FMG"]` to `[universe]` in `config.toml`.
- Tests: universe shape, open/closed at known Sydney times including a weekend, fee minimum and percentage, symbol mapping.

## Workstream C — ASX announcements data plugin

Create `rigger/plugins/data/asx_announcements.py` with `class ASXAnnouncements(DataPlugin)`, `name = "asx_announcements"`, `market = "asx"`.

- For each instrument with `market == "asx"`, GET `https://www.asx.com.au/asx/1/company/{code}/announcements?count=20&market_sensitive=false` with `httpx`. Handle 404 and rate limits with a short backoff.
- Each announcement becomes a `NewsItem`: `id` = sha256(url + published), `instrument_ids=[inst.id]`, `published` parsed to UTC, `title` prefixed `[PS] ` when `market_sensitive` is true, `url` the PDF link, `body=None`, `source="asx_announcements"`. Skip items older than `since`.
- Register the entry point and add `[plugins.asx_announcements]`.
- Tests: `respx` fixture serving a saved JSON payload; assert count, id stability, price-sensitive prefix, `since` filtering, and graceful handling of a 404.
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
