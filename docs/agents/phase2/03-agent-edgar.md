# Phase 2 — Agent 3: SEC EDGAR data plugin

**Branch:** `phase2/D-sec-edgar`. One PR. This is the largest single workstream; budget accordingly and cut scope from the bottom of the list if needed, reporting what you dropped.

## Workstream D — SEC EDGAR

Create `rigger/plugins/data/sec_edgar.py` with `class SECEdgar(DataPlugin)`, `name = "sec_edgar"`, `market = "us"`.

- **Compliance.** Every request sends `User-Agent: Rigger/0.1 (<contact email from [plugins.sec_edgar].contact>)`. Throttle to at most 10 requests per second with a simple async semaphore + sleep. Cache `company_tickers.json` for the run.
- **Ticker → CIK.** GET `https://www.sec.gov/files/company_tickers.json`, build symbol → zero-padded 10-digit CIK.
- **Filings → NewsItem.** GET `https://data.sec.gov/submissions/CIK{cik}.json`. From `filings.recent`, take forms in `{"8-K","10-Q","10-K","4"}` with `filingDate >= since`. Each becomes a `NewsItem`: `id` = sha256(accession number), `instrument_ids=[inst.id]`, `published` = filing date UTC, `title` = `"{form}: {primaryDocDescription or form}"`, `url` = the filing index URL (`https://www.sec.gov/Archives/edgar/data/{cik_int}/{accession_nodash}/{primaryDocument}`), `body=None`, `source="sec_edgar"`.
- **Fundamentals.** GET `https://data.sec.gov/api/xbrl/companyfacts/CIK{cik}.json`. For each of `Revenues` (fallback `RevenueFromContractWithCustomerExcludingAssessedTax`), `NetIncomeLoss`, `EarningsPerShareDiluted`, `CommonStockSharesOutstanding` (dei namespace), take the latest 10-K and latest 10-Q fact by `end` date. Emit `Fundamental(instrument_id, as_of=end, metric="{tag}_{fp}", value, source="sec_edgar")`.
- Only process instruments with `market == "us"`. Unknown tickers log and skip.
- Register `sec_edgar = "rigger.plugins.data.sec_edgar:SECEdgar"`. Add `[plugins.sec_edgar]` with `enabled = true` and `contact = "you@example.com"`.
- Tests: `respx` fixtures in `tests/fixtures/edgar/` with trimmed real payloads for one company. Assert CIK lookup, filing filter by form and date, url construction, fundamentals selection of latest annual and quarterly, User-Agent header present, unknown ticker skipped.
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
