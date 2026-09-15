# Rigger

A personal investment research assistant for people who enjoy investing as a hobby.

You tell Rigger what you're interested in — a company, a sector, an industry, a market, a theme. It gathers evidence about them from public sources, an AI synthesises that evidence into reports you can check line by line, and an optional thesis layer tracks a long-horizon idea as evidence accumulates for and against it.

**It does not trade, and it does not tell you what to buy.** The product is clarity: organised facts, cited summaries, and somewhere to reason about an idea. Nothing here is financial advice.

## What it does

```
Data plugins ──ingest──▶ SQLite ──extract──▶ Events
(yfinance, rss, sec_edgar,          (LLM turns news into
 asx_announcements, calendar)        structured facts)
                                              │
                                    evidence pool (prices, news, events, fundamentals)
                                              │
                    ┌─────────────────────────┼─────────────────────────┐
                    ▼                         ▼                         ▼
             cited reports              grounded chat            optional theses
        (reports/<target>/<date>.md)  (local data first,    (evidence for and against,
                                       web search opt-in)      health computed, not claimed)
```

Four rules the code actually enforces:

- **Facts come from the harness, not the model.** Prompts forbid reasoning from memory. Every fact the model uses was collected by a data plugin.
- **Everything is cited.** A report claim naming an evidence id that wasn't gathered is dropped before you see it, and a draft left with no substantive claims is rejected outright. Chat citations are machine-checked; an answer without verifiable support is labelled AI inference rather than passed off as fact.
- **The AI synthesises and challenges — it doesn't tip.** It summarises what was found and argues both sides. It does not recommend buying or selling.
- **Discovery never auto-accepts.** A thesis proposes *candidate* evidence; only you accept it, and thesis health is computed by a pure function over what you accepted. The model can write prose about that state, but it never decides it.

## Requirements

- Python 3.12+
- [uv](https://docs.astral.sh/uv/)
- An LLM provider key. OpenRouter is the simplest single-key option.

## Install

```bash
git clone https://github.com/lukebrown14-code/Rigger.git
cd Rigger
uv sync
cp .env.example .env      # then fill in your key(s)
```

## Configure

`config.toml` holds your targets, model routing and plugin settings. `.env` holds secrets and is never committed.

Pick a provider under `[llm]`:

| `provider`      | What it does                                | Key needed |
|-----------------|---------------------------------------------|------------|
| `openrouter`    | One API for many models                     | `OPENROUTER_API_KEY` |
| `litellm`       | LiteLLM SDK, calls vendors directly         | `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `GEMINI_API_KEY` as needed |
| `litellm-proxy` | A LiteLLM proxy you run at `proxy_base_url` | `LITELLM_PROXY_KEY` |

Four jobs route to models independently in `[llm.routing]` — `extract`, `report`, `chat` and `thesis` — so you can put a cheap fast model on bulk news extraction and a stronger one on reports. Model ids are plain strings; change them freely, or press `m` in the app to browse what your provider offers, with prices.

Set `[plugins.sec_edgar].contact` to a real email before ingesting US filings — the SEC requires a contact address in the User-Agent.

## Use it

Open the app:

```bash
uv run rig
```

| Key | Screen |
|-----|--------|
| `1`–`6` | Home, Data, Config, Reports, Theses, Chat |
| `w` | Targets — what you're following |
| `c` | Console |
| `m` | Model picker |
| `?` / `q` | Help / quit |

### Or from the shell

```bash
uv run rig target add mining --kind sector --market asx --tickers BHP,RIO,FMG
uv run rig target list
uv run rig ingest --since 2025-01-01      # prices, news, filings, fundamentals, calendar
uv run rig extract                        # news → structured events
uv run rig report mining                  # writes reports/mining/<date>.md
```

Targets come in five kinds — `company`, `sector`, `industry`, `market` and `theme`. All but `market` name tickers in one market; a market target names only the market. Older `[watchlists]` tables keep working, with their kind inferred from shape.

Theses are optional. Skip them entirely and you still get targets, evidence and reports:

```bash
uv run rig thesis create "Iron ore demand holds through 2027" --targets mining
uv run rig thesis propose <id>    # model suggests candidate evidence; you accept it
uv run rig thesis show <id>
```

Inspect things:

```bash
uv run rig llm costs --since 2026-09-01
uv run rig llm models
uv run rig plugins list
uv run rig config show && uv run rig config validate
```

## Project layout

```
rigger/
├── core/            models, SQLite (SQLModel), config, plugin registry, event bus
├── llm/             provider-agnostic client, model catalog, routing, structured calls
├── plugins/
│   ├── markets/     us, asx
│   ├── data/        yfinance, yfinance_calendar, rss, asx_announcements, sec_edgar
│   └── targets/     company, sector, industry, theme, market
├── targets.py       what you follow, and how it resolves to instruments
├── evidence.py      one read model over bars, news, events and fundamentals
├── extract.py       news → structured Event rows
├── reports.py       cited research reports, with the citation contract enforced
├── chat.py          grounded Q&A, local evidence first, web search opt-in
├── theses.py        long-horizon claims with evidence for and against
├── thesis_health.py pure function over accepted evidence
├── tui/             Textual app and screens
└── cli.py           the `rig` command
tests/               pytest, fully offline (respx + a fake LLM)
```

Plugins are discovered through the `rigger.plugins` and `rigger.targets` entry-point groups in `pyproject.toml`. A new data source is one file implementing one class.

## Develop

```bash
uv run pytest
uv run ruff check . && uv run ruff format .
uv run mypy --strict rigger/core rigger/llm
```

The same three run in CI on every push. Tests never touch the network.

Design docs live in `docs/`, one per piece of work — `REDESIGN_PLAN.md` explains why this stopped being a trading harness.

## Caveats

- Free data sources are rate-limited and sometimes wrong. Treat a single citation as a lead, not a fact.
- A cited report is only as good as what was gathered. If ingest missed something the model cannot know about it — that is the deliberate trade for never inventing facts.
- Nothing here is financial advice, and none of it substitutes for reading the primary source.
