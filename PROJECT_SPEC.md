# Rigger — Project Specification

> **Audience:** AI coding agents and developers implementing this project. This document is the single source of truth for scope, architecture, contracts and build order. Read it fully before writing code.
>
> **Status:** Greenfield. Repository is empty apart from this file.
> **Owner:** Luke (luke_brown14@icloud.com)
> **Created:** 2026-09-14

---

## 1. Purpose

Build a harness that makes better investment decisions by having better information and a disciplined evaluation loop.

The harness:

1. **Gathers information** (prices, news, filings, fundamentals, events) from pluggable data sources.
2. **Reasons over it** using AI models accessed through **OpenRouter**, producing ranked, evidence-backed trade signals.
3. **Paper trades** those signals in a simulated portfolio with realistic fees and slippage.
4. **Scores every decision** against real outcomes so the system learns which sources, prompts, models and strategies are actually predictive.

The edge is expected to come from *information quality and evaluation discipline*, not from any single model.

### Non-goals (for now)

- Live broker execution (later plugin, hard-gated).
- High-frequency or intraday trading. Daily cadence is the target.
- A web UI (later plugin; the CLI and reports come first).

---

## 2. Decisions already made

| Topic | Decision |
|---|---|
| Core scope | Market-agnostic. Markets, data sources, strategies, brokers and reports are **plugins**. |
| Initial mode | Research + **paper trading**. |
| Language | **Python 3.12+**. **Rust** (PyO3/maturin) only for proven hot paths (backtest kernel, indicators). |
| Interface | **CLI** (`rig`) + **scheduled reports** (Markdown first, HTML/email later). |
| AI access | **LiteLLM** (default), with **OpenRouter** and **LiteLLM Proxy** as provider options behind a single client. Models are config strings, never hard-coded. |
| Storage | **SQLite** via SQLModel. Single file DB, easy to back up and inspect. |
| Package manager | `uv`. |

---

## 3. Guiding principles

- **Facts come from the harness, not the model.** Prompts must forbid reasoning from memory about current prices, news or events. Every fact the model uses must be in the brief.
- **Every signal carries evidence.** A signal without traceable evidence IDs is rejected.
- **Everything is logged.** Every LLM call, signal, order, fill and evaluation is persisted with timestamps, model, prompt version and cost.
- **Plugins are cheap to write.** A new data source should be one file implementing one class.
- **Paper before live. Always.** Live trading requires `LIVE_TRADING=true`, a daily notional cap, and a confirmed kill switch.
- **Config over code.** Universe, model routing, risk limits and schedules live in TOML, not Python.

---

## 4. Architecture

### 4.1 Directory layout

```
rigger/
├── core/
│   ├── plugin.py          # Plugin base classes, registry, entry-point discovery
│   ├── models.py          # Pydantic domain models (see §5)
│   ├── db.py              # SQLModel engine, tables, session helpers, migrations
│   ├── config.py          # pydantic-settings (.env) + TOML loader (config.toml)
│   ├── events.py          # In-process async event bus
│   └── scheduler.py       # APScheduler job definitions
├── llm/
│   ├── client.py           # Provider-agnostic client: cache, cost + latency logging
│   ├── providers.py        # OpenRouter, LiteLLM SDK, LiteLLM Proxy implementations
│   ├── router.py           # Task name → model id mapping from config
│   ├── structured.py       # JSON-schema enforced calls returning validated Pydantic objects
│   └── prompts/            # Jinja2 templates, versioned by filename (analyst_v1.j2, critic_v1.j2 ...)
├── plugins/
│   ├── markets/           # MarketPlugin implementations   (us, asx, crypto)
│   ├── data/              # DataPlugin implementations     (yfinance, rss, asx_announcements, sec_edgar, ccxt)
│   ├── strategies/        # StrategyPlugin implementations (llm_analyst, critic, ensemble, momentum)
│   ├── brokers/           # BrokerPlugin implementations   (paper; later alpaca, ibkr)
│   └── reports/           # ReportPlugin implementations   (markdown, html, email)
├── paper/
│   ├── portfolio.py       # Cash, positions, realised/unrealised P&L, fee + slippage model
│   └── risk.py            # Position sizing and exposure rules
├── eval/
│   ├── scorecard.py       # Performance metrics per strategy / model / source / prompt version
│   ├── attribution.py     # Which evidence sources correlate with good outcomes
│   └── backtest.py        # Replay strategies over stored history using the LLM cache
├── cli.py                 # Typer app exposing the `rig` command
├── rust/                  # Optional maturin crate (Phase 5)
├── tests/
├── config.toml            # User config (universe, routing, risk, schedule)
├── .env.example           # OPENROUTER_API_KEY=... and other secrets
├── pyproject.toml
└── PROJECT_SPEC.md        # This file
```

### 4.2 Data flow (daily pipeline)

```
DataPlugins ──ingest──▶ SQLite ──build brief──▶ StrategyPlugins (LLM via OpenRouter)
                                                        │
                                                        ▼ Signals (+ evidence)
                                                 Critic pass (2nd model)
                                                        │
                                                        ▼ adjusted conviction
                                                   paper/risk.py  ──reject/size──▶ Orders
                                                                                     │
                                                                                     ▼
                                                                              PaperBroker fills
                                                                                     │
                                                                                     ▼
                                                                          ReportPlugin → reports/YYYY-MM-DD.md
                                                                                     │
                                          (weekly) eval/scorecard.py ◀───────────────┘
```

Events emitted on the bus: `data.updated`, `signal.generated`, `signal.critiqued`, `order.submitted`, `order.filled`, `report.rendered`, `evaluation.completed`.

---

## 5. Domain models (`core/models.py`)

All models are Pydantic v2. Persisted versions are SQLModel tables in `core/db.py` with the same field names.

```python
class Instrument(BaseModel):
    id: str                 # "US:AAPL", "ASX:BHP", "CRYPTO:BTC-USD"
    market: str             # plugin name: "us", "asx", "crypto"
    symbol: str
    name: str | None
    currency: str
    sector: str | None

class Bar(BaseModel):
    instrument_id: str
    ts: datetime            # bar close, UTC
    open: float; high: float; low: float; close: float; volume: float
    source: str             # data plugin name

class NewsItem(BaseModel):
    id: str                 # hash of url+published
    instrument_ids: list[str]
    published: datetime
    title: str
    url: str
    body: str | None
    source: str

class Event(BaseModel):     # structured fact extracted from news/filings by the LLM
    id: str
    instrument_id: str
    ts: datetime
    kind: Literal["earnings","guidance","dividend","insider_trade","m&a","regulatory","macro","other"]
    summary: str
    sentiment: float        # -1..1
    evidence_ids: list[str] # NewsItem ids
    extracted_by: str       # model id
    prompt_version: str

class Fundamental(BaseModel):
    instrument_id: str; as_of: date; metric: str; value: float; source: str

class Signal(BaseModel):
    id: str
    ts: datetime
    instrument_id: str
    strategy: str           # plugin name
    direction: Literal["long","short","flat"]
    conviction: float       # 0..1
    horizon_days: int
    thesis: str
    invalidation: str       # what would prove the thesis wrong
    evidence_ids: list[str] # NewsItem / Event / Fundamental / Bar ids used
    model: str | None
    prompt_version: str | None
    cost_usd: float | None

class Order(BaseModel):
    id: str; signal_id: str; instrument_id: str
    side: Literal["buy","sell"]; qty: float
    type: Literal["market","limit"]; limit_price: float | None
    submitted_ts: datetime; broker: str

class Fill(BaseModel):
    order_id: str; ts: datetime; qty: float; price: float; fee: float; slippage: float

class Position(BaseModel):
    instrument_id: str; qty: float; avg_price: float
    opened_ts: datetime; signal_id: str

class Evaluation(BaseModel):
    signal_id: str; evaluated_ts: datetime
    horizon_return: float; hit: bool
    benchmark_return: float; excess_return: float

class LLMCall(BaseModel):
    id: str; ts: datetime; task: str; model: str; prompt_version: str
    prompt_hash: str; input_tokens: int; output_tokens: int
    cost_usd: float; latency_ms: int; cached: bool
```

---

## 6. Plugin contracts (`core/plugin.py`)

```python
class Plugin:
    name: str                       # unique, snake_case
    version: str = "0.1.0"
    def configure(self, cfg: dict) -> None: ...   # receives its [plugins.<name>] TOML table

class MarketPlugin(Plugin):
    def universe(self) -> list[Instrument]: ...
    def is_open(self, ts: datetime) -> bool: ...
    def next_open(self, ts: datetime) -> datetime: ...
    currency: str

class DataPlugin(Plugin):
    market: str | None              # None = works for any market
    async def fetch(self, instruments: list[Instrument], since: datetime) -> list[Bar | NewsItem | Fundamental]: ...

class StrategyPlugin(Plugin):
    async def generate(self, ctx: Context) -> list[Signal]: ...
    # Context gives read access to db, llm client, config, and the current instrument universe

class BrokerPlugin(Plugin):
    async def submit(self, order: Order) -> Fill: ...
    async def positions(self) -> list[Position]: ...
    async def cash(self) -> float: ...

class ReportPlugin(Plugin):
    def render(self, report: Report) -> Path: ...
```

**Discovery:** plugins register via `pyproject.toml` entry points:

```toml
[project.entry-points."rigger.plugins"]
yfinance = "rigger.plugins.data.yfinance:YFinanceData"
```

Third-party plugins are ordinary pip packages using the same entry-point group. `rig plugins list` prints all discovered plugins with type, version and enabled state.

---

## 7. LLM layer (LiteLLM / OpenRouter / Proxy)

Rigger talks to models through a single provider-agnostic `LLMClient` in
`llm/client.py`. The actual call is delegated to one of three providers in
`llm/providers.py`, selected by `[llm] provider`:

| Provider | Description |
|---|---|
| `litellm` (default) | Uses the `litellm` Python SDK directly. Model ids are provider-prefixed (`openai/gpt-4o`, `anthropic/claude-sonnet-4`, `gemini/...`). API keys are read from the environment by LiteLLM. Built-in cost tracking. |
| `openrouter` | Uses the `openai` SDK against `https://openrouter.ai/api/v1` with `OPENROUTER_API_KEY`, sending `HTTP-Referer` / `X-Title`. Pricing from OpenRouter's `/models` endpoint (cached daily). |
| `litellm-proxy` | Uses the `openai` SDK against your self-hosted proxy (`proxy_base_url`, default `http://localhost:4000`). Backends + model aliases are configured in the proxy's own `config.yaml`. |

### 7.1 Client (`llm/client.py`)

- Caching, cost + latency logging, and LLMCall persistence live here and are shared across providers.
- Exponential backoff on 429/5xx, max 5 attempts, 60 s timeout (provider-dependent).
- Every call writes an `LLMCall` row.
- **Cache:** key = sha256(model + prompt_version + rendered prompt). Cache hits are marked `cached=True` and cost 0. Essential for backtests.

### 7.2 Routing (`llm/router.py`)

`config.toml`:

```toml
[llm]
provider = "litellm"                      # litellm | openrouter | litellm-proxy
proxy_base_url = "http://localhost:4000"  # only used by litellm-proxy

[llm.routing]
extract  = "gemini/gemini-flash-1.5"      # cheap, fast: news → Event
analyse  = "anthropic/claude-sonnet-4"    # strong reasoning: brief → Signal
critique = "openai/gpt-4o"                # different vendor: attack the thesis
pm       = "anthropic/claude-opus-4"      # weekly portfolio review
```

Model ids are opaque strings. Change them freely; the scorecard tracks performance per model. Note that LiteLLM uses vendor prefixes (`anthropic/`, `openai/`, `gemini/`, `groq/`, …) while OpenRouter uses its own ids, so routing changes when switching provider.

### 7.3 Structured outputs (`llm/structured.py`)

`await llm.structured(task="analyse", template="analyst_v1.j2", vars={...}, schema=SignalDraft)`

- Builds a JSON schema from the Pydantic model and passes it via `response_format`.
- Validates the response; on failure re-prompts once with the validation error appended.
- Returns the Pydantic instance plus the `LLMCall` id.

### 7.4 Prompt rules

- Templates live in `llm/prompts/` and are versioned by filename. Never edit a version in place; create `_v2`.
- Every template must include: the instrument brief (facts only), the explicit instruction *"Use only the information provided. Do not rely on prior knowledge of prices, news or events."*, the required output schema, and a request for `invalidation` conditions.
- `prompt_version` is stored on every Signal and Event.

---

## 8. Strategies

| Plugin | Description |
|---|---|
| `llm_analyst` | For each instrument: assemble brief (last 60 bars summary, indicators, last 14 days of NewsItems/Events, key Fundamentals, upcoming calendar). Call `analyse`. Emit Signal. |
| `critic` | Wraps another strategy. Sends each Signal's thesis + brief to `critique` model asking for the strongest counter-argument and a revised conviction. Stores both. |
| `ensemble` | Runs the analyst brief through N models in `[llm.ensemble.models]`, averages conviction, records disagreement as `metadata.dispersion`. |
| `momentum` | Non-LLM baseline: 12-1 month momentum rank. Exists so the LLM strategies have a benchmark to beat. |

---

## 9. Paper trading and risk

### `paper/portfolio.py`
- Starting cash from config (`paper.starting_cash = 100_000`).
- Fills at **next session open** after signal time.
- Slippage model: `slippage_bps` (default 5) applied against the trade direction.
- Fee model per market plugin (e.g. ASX 0.1 % min $10, US $0, crypto 0.1 %).
- Tracks realised/unrealised P&L, per-position and total, in base currency (config `base_currency = "AUD"`), using a simple daily FX rate table.

### `paper/risk.py`
Rules (all configurable):
- `max_position_pct` of equity per instrument (default 5 %).
- `max_sector_pct` (default 25 %).
- `max_gross_exposure_pct` (default 100 %; no leverage).
- `min_conviction` to trade (default 0.6).
- Sizing: `fixed_fraction` = conviction-scaled fraction capped at `max_position_pct`; optional `kelly_capped`.
- `daily_loss_halt_pct`: if equity falls more than this in a day, no new orders until manually reset (`rig paper resume`).

---

## 10. Evaluation

### `eval/scorecard.py`
For each Signal older than its horizon: compute `horizon_return`, `hit` (return sign matches direction), `benchmark_return` (market index), `excess_return`. Aggregate by **strategy, model, prompt_version, market, sector, evidence source**:
- Hit rate, mean excess return, Sharpe, max drawdown, calibration curve (conviction bucket vs actual hit rate).
- `rig scorecard` prints a Rich table; `rig scorecard --json` for machines.

### `eval/attribution.py`
For each evidence source (plugin name + event kind), regress excess return on presence of that evidence. Report sources with positive, significant contribution and those with none, so useless sources can be disabled.

### `eval/backtest.py`
Replays the daily pipeline over `[from, to]` using only data with `ts <= day`. LLM calls hit the cache where possible; `--no-llm` forces non-LLM strategies only. This loop is the **Rust candidate** if it becomes slow.

---

## 11. CLI (`rig`)

```
rig plugins list [--type data|strategy|...]
rig plugins enable <name> / disable <name>

rig ingest [--market us] [--tickers AAPL,MSFT] [--since 2024-01-01]
rig analyse [--strategy llm_analyst] [--dry-run]
rig execute                      # route pending signals through risk + broker
rig report [--format markdown|html] [--date YYYY-MM-DD]
rig run                          # ingest → analyse → execute → report in one go

rig paper status | positions | trades | reset | resume
rig evaluate                     # score aged signals
rig scorecard [--by strategy|model|source|prompt] [--json]
rig backtest --from DATE [--to DATE] [--strategy ...] [--no-llm]

rig llm costs [--since DATE]     # spend by task/model
rig llm models                   # list OpenRouter models + prices

rig daemon                       # run the scheduler in the foreground
rig config show | validate
```

---

## 12. Configuration

`.env` (secrets, never committed):
```
# litellm (default) reads provider keys from your environment
# (OPENAI_API_KEY, ANTHROPIC_API_KEY, GEMINI_API_KEY, ...)
OPENROUTER_API_KEY=      # only for provider = "openrouter"
LITELLM_PROXY_KEY=       # only for provider = "litellm-proxy"
LIVE_TRADING=false
```

`config.toml`:
```toml
base_currency = "AUD"
db_path = "data/rigger.db"
reports_dir = "reports"

[universe]
us  = ["AAPL","MSFT","NVDA","GOOGL","AMZN"]
asx = ["BHP","CBA","CSL","WES","FMG"]

[llm]
provider = "litellm"
proxy_base_url = "http://localhost:4000"

[llm.routing]
extract = "gemini/gemini-flash-1.5"
analyse = "anthropic/claude-sonnet-4"
critique = "openai/gpt-4o"
pm = "anthropic/claude-opus-4"

[llm.ensemble]
models = ["anthropic/claude-sonnet-4","openai/gpt-4o","gemini/gemini-pro-1.5"]

[paper]
starting_cash = 100000
slippage_bps = 5

[risk]
max_position_pct = 5
max_sector_pct = 25
max_gross_exposure_pct = 100
min_conviction = 0.6
daily_loss_halt_pct = 3

[schedule]
ingest   = "0 6 * * 1-5"     # cron, local time
analyse  = "30 6 * * 1-5"
execute  = "0 10 * * 1-5"
report   = "15 10 * * 1-5"
evaluate = "0 18 * * 5"

[plugins.yfinance]
enabled = true
[plugins.rss]
enabled = true
feeds = ["https://feeds.reuters.com/reuters/businessNews"]
```

---

## 13. Build phases

### Phase 1 — Skeleton: one signal end-to-end
- [x] `uv init`; deps: `typer`, `rich`, `pydantic`, `pydantic-settings`, `sqlmodel`, `openai`, `httpx`, `apscheduler`, `jinja2`, `yfinance`, `pandas`, `numpy`
- [x] `core/models.py`, `core/db.py`, `core/config.py`, `core/plugin.py`, `core/events.py`
- [x] `llm/openrouter.py`, `llm/router.py`, `llm/structured.py`, `llm/prompts/analyst_v1.j2`
- [x] Plugins: `markets/us`, `data/yfinance`, `strategies/llm_analyst`, `brokers/paper`, `reports/markdown`
- [x] `cli.py`: `plugins list`, `ingest`, `analyse`, `execute`, `report`, `run`, `paper status`, `llm costs`
- [x] **Exit criterion:** `rig run` on 5 US tickers yields a markdown report with reasoned, evidence-linked signals and a paper portfolio with fills.

### Phase 2 — Information edge
- [x] `data/rss`, `data/asx_announcements`, `data/sec_edgar`, `markets/asx`
- [x] `extract` task + `Event` table; `llm/prompts/extract_v1.j2`
- [x] Event calendar (earnings, ex-div) in the brief
- [x] `strategies/critic`, `strategies/ensemble`, `strategies/momentum`

### Phase 3 — Evaluation loop
- [ ] `eval/scorecard.py`, `eval/attribution.py`, `eval/backtest.py`
- [ ] `rig evaluate`, `rig scorecard`, `rig backtest`
- [ ] Calibration report in the weekly briefing

### Phase 4 — Automation and hardening
- [ ] `core/scheduler.py`, `rig daemon`, macOS `launchd` plist in `deploy/`
- [ ] `reports/html`, `reports/email`
- [ ] Full risk rule set incl. daily loss halt and kill switch
- [ ] Tests: pytest, `respx` for HTTP fixtures, `FakeLLM` returning canned JSON; CI via GitHub Actions

### Phase 5 — Optional extensions
- [ ] Rust crate (`rust/`, maturin) for indicators + backtest kernel, only after profiling
- [ ] `brokers/alpaca` (US) / `brokers/ibkr`, gated by `LIVE_TRADING=true` + daily notional cap
- [ ] `markets/crypto` + `data/ccxt`
- [ ] Web dashboard plugin reading the same SQLite DB
- [ ] Telegram/Slack alert plugin

---

## 14. Expansion ideas (backlog)

- **More information sources** (each one DataPlugin): earnings call transcripts, broker research PDFs, Reddit/X sentiment, Google Trends, RBA/Fed statements, insider trading feeds, options flow, shipping and commodity indices.
- **Self-improving prompts:** scorecard by `prompt_version`; promote winners automatically.
- **Model tournament:** same brief through many OpenRouter models daily; track which model wins per market/sector.
- **Regime detection:** macro plugin labels risk-on/off and rate cycle; strategies condition on it.
- **Portfolio manager agent:** weekly LLM review of the whole book proposing rebalances.
- **Thesis monitoring:** daily check of each open position's `invalidation` conditions against new Events.
- **Trade post-mortems:** one-page "why / what happened / lesson" per closed trade, generated by LLM.
- **Multiple paper books:** run strategies as separate portfolios for clean comparison.
- **ESG / sustainability screen** plugin.

---

## 15. Coding standards

- Python 3.12+, type hints everywhere, `ruff` for lint+format, `mypy --strict` on `core/` and `llm/`.
- Async for I/O (data fetch, LLM calls); sync for CLI glue.
- No network calls in unit tests. Use `respx` and `FakeLLM`.
- All timestamps stored in UTC; convert at the report layer.
- Money as `float` in base currency for now; revisit `Decimal` if precision issues appear.
- Commit messages: conventional commits (`feat:`, `fix:`, `chore:`).
- Never commit `.env`, `data/*.db`, or `reports/`.

---

## 16. Verification checklist

- `rig plugins list` shows every built-in plugin.
- `rig ingest --market us --tickers AAPL,MSFT,NVDA` populates `bar` and `newsitem` tables.
- `rig analyse` creates `signal` rows with non-empty `evidence_ids`, `thesis`, `invalidation`.
- `rig llm costs` shows non-zero spend and correct model ids.
- `rig execute` then `rig paper status` shows positions, cash and P&L.
- `rig report` writes `reports/YYYY-MM-DD.md`.
- `rig backtest --from 2025-01-01 --no-llm` completes offline.
- `pytest` passes with no network.

---

## 17. Caveats

- Paper results overstate real performance (no true market impact, survivorship in universe selection).
- Models hallucinate; the brief must contain every fact and the prompt must forbid outside knowledge.
- Free data (yfinance, RSS) is rate-limited and sometimes wrong; budget for a paid source once the loop proves value.
- Nothing here constitutes financial advice. Keep live trading disabled until the scorecard shows a sustained, statistically meaningful edge.
