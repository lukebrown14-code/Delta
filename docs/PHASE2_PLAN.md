# Phase 2 Plan — Information Edge

> Split of Phase 2 (see `PROJECT_SPEC.md` §13) into a foundation step and six
> parallel workstreams so multiple agents can build concurrently with minimal
> merge conflicts. Agent prompts live in `docs/agents/phase2/`.

## Why a foundation step

The Phase 1 analyst builds its brief from price bars only and hard-codes the
lines "News: none available" and "Upcoming events: none available" into the
prompt. Every Phase 2 workstream either feeds the brief or consumes it. If six
agents each bolt their data onto that function, every PR conflicts.

The foundation step defines the seam once. It runs alone, then everything else
fans out.

## Step 0 — Foundation (one agent, run first)

1. **`rigger/brief.py`**
   - `@dataclass Brief` with sections: `prices`, `news`, `events`, `fundamentals`, `calendar`. Each section is `list[str]` of fact lines plus `evidence_ids: list[str]`.
   - `build_brief(ctx, instrument, as_of) -> Brief`. Calls one independent function per section, each reading only its own table (`BarTable`, `NewsItemTable`, `EventTable`, `FundamentalTable`). Missing sections render as "none available".
   - `Brief.render() -> str` and `Brief.evidence_ids -> list[str]` (union of all sections).
2. **Extract `analyse_one(ctx, inst, model, brief) -> Signal | None`** from `LLMAnalyst.generate` so the ensemble can reuse the exact analyst prompt and schema instead of copying them. `LLMAnalyst.generate` becomes a loop over `analyse_one`.
3. **Add `metadata: dict[str, Any] = {}`** to `Signal` (models.py) and a JSON column on `SignalTable` (db.py). Critic and ensemble write here.
4. **Add `rig extract`** as an empty CLI command stub and call it in `rig run` between ingest and analyse, so Workstream E adds a body rather than editing the command list.
5. Tests for `build_brief` with fake rows in every table.

Exit: `pytest` green, `rig run` still works on the US five, and the rendered brief is byte-identical to Phase 1 for the price section.

Already in place, no work needed: `Event`/`EventTable`, `Fundamental`/`FundamentalTable`, `[plugins.<name>]` config tables reach plugins via `Plugin.configure`, entry-point discovery in `core/plugin.py`.

## Workstreams (run concurrently after Step 0)

| ID | Workstream | Files it owns | Depends on |
|---|---|---|---|
| A | ASX market plugin | `rigger/plugins/markets/asx.py` | Step 0 |
| B | RSS data plugin | `rigger/plugins/data/rss.py` | Step 0 |
| C | ASX announcements data plugin | `rigger/plugins/data/asx_announcements.py` | Step 0 (market name string only from A) |
| D | SEC EDGAR data plugin | `rigger/plugins/data/sec_edgar.py` | Step 0 |
| E | Event extraction | `rigger/llm/prompts/extract_v1.j2`, `rigger/extract.py`, body of `rig extract` | Step 0 |
| F | Calendar in the brief | `rigger/plugins/data/yfinance_calendar.py` | Step 0 |
| G | Critic strategy | `rigger/plugins/strategies/critic.py`, `rigger/llm/prompts/critic_v1.j2` | Step 0 |
| H | Ensemble strategy | `rigger/plugins/strategies/ensemble.py` | Step 0 |
| I | Momentum baseline | `rigger/plugins/strategies/momentum.py` | nothing |

### A — ASX market plugin
- Universe from `config.universe["asx"]`, instrument ids `ASX:<code>`, currency AUD, sector lookup via yfinance `.info` if cheap, else `None`.
- `is_open` / `next_open` using `Australia/Sydney`, 10:00–16:00 weekdays, no holiday calendar yet (TODO note).
- Fee model: 0.1 % of notional, minimum $10. Expose as `fee(notional) -> float` on the plugin; the paper broker calls it if present.
- yfinance ticker mapping: `BHP` → `BHP.AX`. Put the mapping on the market plugin (`yf_symbol(instrument)`) so the yfinance data plugin can ask for it.

### B — RSS data plugin
- `feedparser` dependency. Feeds from `[plugins.rss].feeds`.
- Produce `NewsItem` rows. `id` = sha256(url + published). Map to instruments by symbol or company-name token match against the universe, else `instrument_ids=[]` (macro news still stored).
- Tests with `respx` fixtures serving saved feed XML. No network.

### C — ASX announcements data plugin
- Per ASX ticker, fetch the announcements JSON from the ASX public endpoint. Produce `NewsItem` rows with the PDF URL, `title`, `published`, and `body` = header text. Store `price_sensitive` in the title prefix `[PS]` until `NewsItem` grows a flag.
- Only for instruments where `market == "asx"`.
- Tests with `respx` fixtures.

### D — SEC EDGAR data plugin
- User-Agent header with contact email (SEC requirement), 10 requests/second cap.
- Ticker → CIK via `company_tickers.json`. Recent filings from `submissions/CIK##########.json`: 8-K, 10-Q, 10-K, Form 4 → `NewsItem` rows linking to the filing index.
- `companyfacts` → `Fundamental` rows for Revenues, NetIncomeLoss, EarningsPerShareDiluted, shares outstanding, latest annual and quarterly.
- Only for `market == "us"`. Tests with `respx` fixtures.

### E — Event extraction
- `extract_v1.j2`: given N `NewsItem` titles/bodies for one instrument, return a JSON list of `Event` drafts (kind, summary, sentiment, evidence_ids). Must include the facts-only instruction.
- `rigger/extract.py`: `extract_events(ctx, since)` finds `NewsItem` rows with no `Event` referencing them, batches per instrument (≤ 20 items per call), calls the `extract` route via `structured()`, stores `Event` rows with `extracted_by` and `prompt_version`.
- Fill the `rig extract` stub. `rig run` already calls it.
- Populates the `events` section of the brief through `EventTable` only. Do not edit `brief.py`.
- Tests with `FakeLLM` returning canned events.

### F — Calendar in the brief
- `yfinance_calendar` data plugin: next earnings date and ex-dividend date per instrument via yfinance `.calendar`. Store as `Event` rows with `kind` `earnings` or `dividend`, `ts` in the future, `summary` like "Earnings expected 2026-10-28", `sentiment 0`, `extracted_by "yfinance"`, `prompt_version "n/a"`.
- The brief's `calendar` section (Step 0) reads `EventTable` where `ts > as_of`. Nothing else to wire.

### G — Critic strategy
- `[plugins.critic].wraps = "llm_analyst"`. `generate` runs the wrapped strategy, then for each signal sends thesis + brief to the `critique` route asking for the strongest counter-argument, a list of risks, and a revised conviction 0..1.
- Stores `metadata["critic"] = {counter, risks, original_conviction, model, prompt_version}` and overwrites `conviction`. Keeps `strategy = "critic:llm_analyst"` so the scorecard can compare.
- `critic_v1.j2`. Tests with `FakeLLM`.

### H — Ensemble strategy
- For each instrument, call `analyse_one` (from Step 0) once per model in `config.llm_ensemble_models`. Average conviction, majority direction (ties → flat), union of evidence ids.
- `metadata["ensemble"] = {models, convictions, directions, dispersion}` where dispersion = stdev of convictions.
- `strategy = "ensemble"`. Tests with `FakeLLM` returning different answers per model.

### I — Momentum baseline
- 12-1 month momentum: return from t-252 to t-21 trading days, ranked across the universe. Long the top 20 %, flat otherwise. `conviction` = rank percentile, `horizon_days 21`, `thesis` a one-line explanation, `invalidation` "drops out of top quintile", `evidence_ids` the bars used.
- No LLM, no config. `strategy = "momentum"`.

## Agent assignment (six agents)

| Agent | Workstreams | Reason |
|---|---|---|
| 1 | A then C | ASX domain shared |
| 2 | B then I | RSS is quick; momentum fills slack |
| 3 | D | EDGAR is the largest single item |
| 4 | E then F | Both write `Event` rows |
| 5 | G | Critic |
| 6 | H | Ensemble |

## Rules for every agent

- Work in your own git worktree on branch `phase2/<workstream-id>-<name>`. One PR per workstream.
- Do not edit `rigger/brief.py`, `rigger/plugins/strategies/llm_analyst.py`, `rigger/cli.py`, `rigger/core/models.py`, or `rigger/core/db.py`. If you need a change there, stop and report it instead of making it.
- Register plugins by adding one line under `[project.entry-points."rigger.plugins"]` in `pyproject.toml`. Nothing else in that file except a new dependency if listed for your workstream.
- Add a `[plugins.<name>]` table to `config.toml` with `enabled = true` and your settings.
- Tests must pass offline: `respx` for HTTP, `FakeLLM` from `tests/conftest.py` for models. `uv run pytest`, `uv run ruff check .`, `uv run mypy rigger/core rigger/llm` all clean.
- Read `PROJECT_SPEC.md` §3 (principles) and §7.4 (prompt rules) before writing any prompt template.
- Commit messages: conventional commits. Do not push to `main`.

## Integration (after all PRs merge)

1. Add five ASX tickers to `[universe]`, enable all new plugins, set `[plugins.critic].wraps`.
2. `rig run` on US + ASX. Confirm the rendered brief contains real news, events and calendar lines and that signal `evidence_ids` include `NewsItem` and `Event` ids.
3. `rig analyse --strategy critic,ensemble,momentum` produces signals for all three.
4. Update `README.md` command list and Roadmap.

## Exit criterion

`rig run` on five US and five ASX tickers produces signals whose evidence ids include `NewsItem` and `Event` rows, the critic has recorded a counter-argument on each analyst signal, the ensemble reports dispersion, and the momentum baseline has its own signals ready for Phase 3 comparison.
