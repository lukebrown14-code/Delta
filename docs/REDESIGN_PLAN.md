# Redesign Plan — Rigger as a hobby research assistant

> Turn Rigger from a paper-trading harness into a personal research assistant: a
> hobby investor adds companies, sectors, industries and markets it cares about;
> Rigger gathers evidence, an AI synthesises that evidence into sourced reports
> and sentiment, and an optional thesis layer tracks an idea over time. Trading
> is out of scope completely.

## Why

The previous build was a paper-trading pipeline: ingest → analyse → execute →
report, with buy/sell signals as the product. That put the AI in a job it is
weak at — producing profit signals — and put the evaluation loop (scorecard,
attribution, backtest) behind it as the thing that would one day prove an edge.

This redesign inverts that. The product is now *clarity*: organised facts, cited
summaries, and a place to reason about a long-horizon idea. Profit comes later,
and only if the research demonstrably beats naive baselines — which we test
before we ever trade.

## Principles

- **Facts come from the harness, not the model.** Every claim a model makes is
  in evidence the harness collected; prompts forbid reasoning from memory.
- **Everything is cited.** A summary or sentiment without a source is a draft,
  not a result. Web results and AI inference are labelled as such and never
  presented as established fact.
- **The AI is a synthesizer and a challenger, not a tipster.** It summarises,
  and it argues both sides. It does not recommend "buy".
- **Theses are optional.** A user who only wants to browse reports never sees a
  thesis. A thesis is a claim plus evidence for and against.
- **No edge claim without measurement.** Until the scorecard shows a genuine,
  sample-size-respecting advantage, the app says so.
- **Config over code, plugins are cheap, offline tests only.**

## What is removed (the trading/signal machinery)

- `paper/` — portfolio, risk, fx
- `plugins/brokers/` — the paper broker
- `plugins/strategies/` — llm_analyst, critic, ensemble, momentum
- `plugins/reports/markdown.py` and the `Report`/`ReportPlugin` types
- Domain models `Signal`, `Order`, `Fill`, `Position`, `Evaluation`, `Direction`
  and their tables (`signal`, `order`, `fill`, `position`, `evaluation`)
- CLI `run`, `analyse`, `execute`, `report`, `paper`, and `trading_enabled`
- TUI `Pipeline`, `Portfolio`, `Signals` screens

## What is kept (the research foundation)

- `core/` — config, db, plugin, ids, json, time, http, events
- `Instrument` (plus new fields), `Bar`, `NewsItem`, `Event`, `Fundamental`,
  `LLMCall`
- `llm/` — provider-agnostic client, routing, structured outputs, prompts
- `markets/`, `data/` — yfinance, rss, asx_announcements, sec_edgar, calendar
- `watchlists/` + `Scope` — broadened into **watch targets**
- `brief.py` and `extract.py` — facts assembly and news → events

## Domain model

A **watch target** is anything the user wants to follow, not just a share:

```
WatchTarget(id, kind, name, markets, tickers, tags, notes)
   kind: company | sector | industry | market | theme
```

**Evidence** is any sourced fact: a bar, a news item, a filing, a fundamental,
an event (earnings, guidance, …), or a web hit. Every item carries source, url,
timestamp, and enough to cite it in a report.

**Report** is the AI synthesis over a target's evidence: what changed, bull
case, bear case, risks, catalysts, unknowns, and a sentiment with explanations
and citations.

**Thesis** (optional) is a claim plus scope, assumptions, supporting evidence,
counter-evidence, unknowns, confidence, and falsifiers, with a history of how it
changes as evidence arrives. **Thesis health** is a deterministic score over the
evidence graph — not a truth claim — rendered as Emerging / Building / Mixed /
Weakening / Challenged / Idle, with an AI paragraph that cites the drivers.

**Chat** is grounded Q&A: local data first, optional web search as an explicit
tool, every answer cited and labelled `Stored` / `Web` / `AI inference`.

## Build order

Each stage is shippable and keeps `pytest` green. Stages depend only on the
foundation and the stages they consume.

1. Foundation — models, tables, seams, read-side services
2. Watch targets
3. Evidence collection
4. Reports
5. Chat (+ optional web search tool)
6. Theses
7. Thesis health
8. Model selection (refit existing plan)

## How to run multiple agents at once

The foundation ships first because every stage consumes its seams. After that,
stages with disjoint file ownership run in parallel. The map:

| Stage | Name | Owns (files) | Depends on |
|---|---|---|---|
| 0 | Foundation | `core/models.py`, `core/db.py`, `services.py` (read-side), `core/config.py` | — |
| 1 | Watch targets | `plugins/watchlists/*`, CLI `watchlist`, TUI watchlists | 0 |
| 2 | Evidence | `plugins/data/*`, `extract.py`, evidence storage queries | 0 |
| 3 | Reports | prompts `report_v1.j2`, `services.report`, TUI reports | 0, 2 |
| 4 | Chat | `services.chat`, search tool, TUI chat | 0, 2 |
| 5 | Theses | `services.theses`, TUI theses | 0, 2 |
| 6 | Thesis health | `services.thesis_health`, scoring | 5 |
| 7 | Model selection | `llm/catalog.py`, `services.set_llm_route`, picker | 0 |

Parallel batches after 0 ships:

- **Batch A:** 1 (watch targets) and 2 (evidence) — disjoint files.
- **Batch B:** 3 (reports), 4 (chat), 5 (theses), 7 (model selection) — all read
  evidence, own disjoint files.
- **Batch C:** 6 (thesis health) after 5.

Rules for every agent (unchanged from earlier phases):

- Work in your own git worktree on a branch `redesign/<stage>-<name>`; one PR
  per stage.
- Own only your listed files. If you need a change elsewhere, stop and report it
  instead of editing.
- Tests offline only: `respx` for HTTP, `FakeLLM` from `tests/conftest.py` for
  models. `uv run pytest`, `uv run ruff check .`, and the mypy command must stay
  clean.
- Conventional commits. Never commit `.env`, `data/*.db`, or `reports/`.
- Read the principles above before writing any prompt template.

## Testing

- `tests/test_watch_targets.py`, `tests/test_evidence.py`,
  `tests/test_reports.py`, `tests/test_chat.py`, `tests/test_theses.py`,
  `tests/test_thesis_health.py`, `tests/test_catalog.py`.
- `tests/test_tui_*.py` for each new screen via `App.run_test()`.
- Existing data-source tests (`rss`, `sec_edgar`, `asx_announcements`,
  `yfinance_calendar`, `brief`, `extract`, `asx_market`) remain unedited and
  green.

## Out of scope (for now)

Trading, order routing, portfolio accounting, sizing, backtesting, automated
recommendations, and any claim that the tool improves returns.
