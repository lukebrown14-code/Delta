# Rigger

AI-driven investment research and paper-trading harness.

Rigger gathers market data, builds a factual brief per instrument, asks an LLM for evidence-backed trade signals, paper trades them with realistic fees and slippage, and scores every decision against real outcomes. The edge is meant to come from information quality and evaluation discipline, not from any single model.

**Status:** Phase 2 (information edge) merged. US and ASX markets, five data sources, four strategies. Paper trading only. Nothing here is financial advice.

See [PROJECT_SPEC.md](PROJECT_SPEC.md) for the full specification, architecture, plugin contracts and build phases.

## How it works

```
Data plugins ──ingest──▶ SQLite ──extract──▶ Events ──brief──▶ Strategy plugins ──▶ Signals + evidence
(yfinance, rss, sec_edgar,          (LLM turns news            (llm_analyst, critic,        │
 asx_announcements, calendar)        into structured facts)     ensemble, momentum)         │
                                                                              risk rules ──▶ Orders ──▶ Paper fills
                                                                                                            │
                                                                                     Markdown report (reports/YYYY-MM-DD.md)
```

Principles the code enforces:

- **Facts come from the harness, not the model.** Prompts forbid reasoning from memory. Every fact the model uses is in the brief.
- **Every signal carries evidence.** Signals without traceable evidence IDs are rejected.
- **Everything is logged.** Every LLM call, signal, order and fill is persisted with model, prompt version and cost.
- **Paper before live.** Live trading does not exist yet and will be hard-gated when it does.
- **Config over code.** Universe, model routing, risk limits and schedules live in `config.toml`.

## Requirements

- Python 3.12+
- [uv](https://docs.astral.sh/uv/)
- An LLM provider key. OpenRouter is the simplest single-key option.

## Install

```bash
git clone https://codeberg.org/LukeBro14/Rigger.git
cd Rigger
uv sync
cp .env.example .env      # then fill in your key(s)
```

## Configure

`config.toml` holds the universe, model routing, paper and risk settings. The defaults track five US and five ASX tickers and use AUD as the base currency. Each plugin has a `[plugins.<name>]` table with an `enabled` flag; set `[plugins.sec_edgar].contact` to a real email before ingesting US filings, as the SEC requires it.

`.env` holds secrets. Pick a provider in `config.toml` under `[llm]`:

| `provider`      | What it does                                   | Key needed                                          |
|-----------------|------------------------------------------------|-----------------------------------------------------|
| `openrouter`    | One API for many models                        | `OPENROUTER_API_KEY`                                |
| `litellm`       | LiteLLM SDK, calls vendors directly            | `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY` as needed |
| `litellm-proxy` | A LiteLLM proxy you run at `proxy_base_url`    | `LITELLM_PROXY_KEY`                                 |

Model ids are plain strings in `[llm.routing]`. Change them freely.

## Run

The whole daily pipeline in one command:

```bash
uv run rig run
```

Or step by step:

```bash
uv run rig ingest --market asx --since 2025-01-01          # bars, news, filings, fundamentals, calendar
uv run rig extract --since 2026-09-01                      # LLM turns unprocessed news into Event rows
uv run rig analyse --dry-run                               # generate signals, don't persist
uv run rig analyse --strategy critic                       # analyst + adversarial critique, stores both
uv run rig analyse --strategy ensemble,momentum            # multi-model vote and the non-LLM baseline
uv run rig execute                                         # risk-check and fill in the paper book
uv run rig report --date 2026-09-14                        # write reports/2026-09-14.md
```

Strategies:

| Strategy      | What it does                                                                 |
|---------------|------------------------------------------------------------------------------|
| `llm_analyst` | Facts-only brief per instrument to the `analyse` model. One signal each.     |
| `critic`      | Wraps another strategy. A second model attacks each thesis and revises conviction. |
| `ensemble`    | Same brief through every model in `[llm.ensemble].models`. Majority vote, records dispersion. |
| `momentum`    | 12-1 month momentum rank. No LLM. The baseline the others must beat.         |

Inspect state:

```bash
uv run rig paper status          # cash, positions, P&L
uv run rig paper reset           # wipe the paper book
uv run rig llm costs --since 2026-09-01
uv run rig llm models            # available models and prices
uv run rig plugins list
uv run rig config show
uv run rig config validate
```

## Project layout

```
rigger/
├── core/        models, SQLite (SQLModel), config, plugin registry, event bus
├── llm/         provider-agnostic client, routing, structured JSON calls, prompt templates
├── paper/       portfolio accounting, fee/slippage model, risk rules
├── brief.py     facts-only brief: prices, news, events, fundamentals, calendar
├── extract.py   news → structured Event rows via the extract model
├── plugins/
│   ├── markets/     us, asx
│   ├── data/        yfinance, yfinance_calendar, rss, asx_announcements, sec_edgar
│   ├── strategies/  llm_analyst, critic, ensemble, momentum
│   ├── brokers/     paper
│   └── reports/     markdown
└── cli.py       the `rig` command
tests/           pytest, no network (respx + fake LLM)
```

Plugins are discovered through the `rigger.plugins` entry-point group in `pyproject.toml`. A new data source is one file implementing one class.

## Develop

```bash
uv run pytest
uv run ruff check . && uv run ruff format .
uv run mypy rigger/core rigger/llm
```

## Roadmap

1. **Phase 1** — skeleton: `rig run` on five US tickers yields a report with evidence-linked signals and paper fills. *(done)*
2. **Phase 2** — information edge: RSS, ASX announcements, SEC EDGAR, event extraction, critic and ensemble strategies, momentum baseline. *(done, see `docs/PHASE2_PLAN.md`)*
3. **Phase 3** — evaluation loop: scorecard, source attribution, backtesting. *(next)*
4. **Phase 4** — automation: scheduler, HTML/email reports, full risk rules, CI.
5. **Phase 5** — optional: Rust hot paths, live brokers (hard-gated), crypto, dashboard, alerts.

## Caveats

- Paper results overstate real performance.
- Free data (yfinance) is rate-limited and sometimes wrong.
- Keep live trading off until the scorecard shows a sustained, statistically meaningful edge.
