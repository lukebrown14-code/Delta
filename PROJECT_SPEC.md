# Rigger — Project Specification

> **Audience:** AI coding agents and developers working on this repository. This
> is the source of truth for scope, architecture, contracts and conventions.
> Read it before writing code.
>
> **Status:** Research assistant. Watch targets, evidence pool, cited reports,
> grounded chat, theses and thesis health are all built and tested.
> **Owner:** Luke
> **Last rewritten:** 2026-09-16

---

## 1. Purpose

Rigger is a personal investment research assistant for someone who enjoys
investing as a hobby and would like to make some money at it. It is **not** for
a professional desk, and every design decision should be read through that lens:
plain words over industry terms, minimum required configuration, expert knobs
optional and out of the way.

The user names the things they are interested in. Rigger gathers evidence about
them from public sources, an AI synthesises that evidence into reports the user
can check line by line, and an optional thesis layer tracks a long-horizon idea
as evidence accumulates for and against it.

**The product is clarity** — organised facts, cited summaries, and a place to
reason about an idea.

### Why it is no longer a trading harness

The original build was a paper-trading pipeline (ingest → analyse → execute →
report) whose product was buy/sell signals, with an evaluation loop behind it
that would one day prove an edge. That put the model in the job it is worst at —
producing profit signals — and deferred the only thing that would have justified
it. The redesign inverted this. Profit comes later, and only if research
demonstrably beats naive baselines, tested before anything ever trades.

### Non-goals

Trading, order routing, portfolio accounting, position sizing, backtesting,
automated recommendations, and any claim that the tool improves returns. Nothing
Rigger produces is financial advice.

---

## 2. Principles the code enforces

These are not aspirations. Each one is implemented and tested; breaking one
should fail a test.

1. **Facts come from the harness, not the model.** Prompts forbid reasoning from
   memory. Every fact a model uses was collected by a data plugin and is present
   in the evidence handed to it.
2. **Everything is cited.** A report claim citing an evidence id that was not
   gathered is dropped before the user sees it; a draft left with no substantive
   claims is rejected outright. Chat citations are machine-checked against the
   gathered pool, and an answer without verifiable support is labelled AI
   inference rather than presented as fact.
3. **The AI synthesises and challenges; it does not tip.** It summarises what was
   found and argues both sides. It never recommends buying or selling.
4. **Discovery never auto-accepts.** A thesis proposes *candidate* evidence,
   stored unaccepted. Only the user accepts. Thesis health is then computed by a
   pure function over accepted evidence — a model may write prose about that
   state but never decides it.
5. **Web results are never stored evidence.** Search hits are a chat-stage tool,
   labelled as web results in the model's context, and never written to the pool.
6. **Everything is logged.** Every LLM call is persisted with task, model, prompt
   version, prompt hash, tokens, cost, latency and cache status.
7. **Config over code.** Targets, model routing and plugin settings live in
   `config.toml`. Secrets live in `.env` and are never committed.
8. **Plugins are cheap to write.** A new data source is one file implementing one
   class, registered through an entry point.

---

## 3. Architecture

```
rigger/
├── core/
│   ├── models.py        Pydantic domain models (§4)
│   ├── db.py            SQLModel engine, tables, migrations, bulk store
│   ├── config.py        pydantic-settings (.env) + TOML loader (config.toml)
│   ├── plugin.py        Plugin base classes, Scope, registry, entry-point discovery
│   ├── ids.py           stable content hashes
│   ├── http.py          shared User-Agent convention for data plugins
│   ├── json.py, time.py serialisation and UTC helpers
│   └── events.py        in-process async event bus
├── llm/
│   ├── client.py        provider-agnostic client: cache, cost + latency logging
│   ├── providers.py     OpenRouter, LiteLLM SDK, LiteLLM Proxy
│   ├── catalog.py       model catalog, config writeback (routes, per-plugin models)
│   ├── router.py        task -> model id, with per-plugin override
│   ├── structured.py    JSON-schema enforced calls returning validated models
│   └── prompts/         Jinja2 templates, versioned by filename
├── plugins/
│   ├── markets/         us, asx
│   ├── data/            yfinance, yfinance_calendar, rss, asx_announcements, sec_edgar
│   └── targets/         company, sector, industry, theme, market (+ legacy tickers)
├── targets.py           WatchTarget model, resolution from config
├── evidence.py          unified read model over the source tables
├── extract.py           news -> structured Event rows
├── brief.py             facts-only per-instrument brief
├── reports.py           cited research reports, citation contract enforced
├── chat.py              grounded Q&A, local evidence first, web search opt-in
├── theses.py            long-horizon claims, candidate evidence, own tables
├── thesis_health.py     pure function over accepted evidence
├── runtime.py           shared Rigger wiring
├── services.py          pipeline operations and read-side queries
├── tui/                 Textual app, screens, widgets, styles
└── cli.py               the `rig` command
tests/                   pytest, fully offline (respx + FakeLLM)
```

### Data flow

```
data plugins ──ingest──▶ SQLite ──extract──▶ Event rows
                            │
                            ▼
                    evidence.py: one pool of EvidenceItem
                            │
        ┌───────────────────┼───────────────────┐
        ▼                   ▼                   ▼
   reports.py            chat.py            theses.py
  (cited report)    (grounded answer)   (candidate evidence)
                                                │
                                                ▼
                                        thesis_health.py
                                     (deterministic state)
```

---

## 4. Domain model (`core/models.py`, `targets.py`, `evidence.py`)

All models are Pydantic v2. Persisted versions are SQLModel tables in
`core/db.py` with the same field names.

```python
InstrumentId = str  # "<MARKET>:<SYMBOL>", e.g. "US:AAPL"
AssetClass = Literal["equity", "etf", "bond", "fx", "commodity", "crypto", "cash", "other"]


class Instrument(BaseModel):
    id: str
    market: str
    symbol: str
    name: str | None
    currency: str
    sector: str | None = None
    industry: str | None = None
    asset_class: AssetClass = "equity"
    watchlists: tuple[str, ...] = ()  # target ids this instrument belongs to
    tags: frozenset[str] = frozenset()
    meta: dict[str, Any] = {}  # asset-class payload; core never reads it


class WatchTarget(BaseModel):  # targets.py
    id: str
    kind: Literal["company", "sector", "industry", "market", "theme"]
    name: str
    markets: tuple[str, ...] = ()
    tickers: tuple[str, ...] = ()
    tags: frozenset[str] = frozenset()
    notes: str = ""


@dataclass
class EvidenceItem:  # evidence.py — the read model
    id: str
    target_ids: tuple[str, ...]
    ts: datetime
    kind: EvidenceKind
    title: str
    body: str | None
    source: str
    url: str | None
    sentiment: float | None
    raw: dict[str, Any]
```

Source records — `Bar`, `NewsItem`, `Event`, `Fundamental` — keep their existing
write paths and are flattened into `EvidenceItem` for reading. `LLMCall` records
every model call.

`Report` (`reports.py`) carries `summary`, `bull`, `bear`, `risks`, `catalysts`,
`unknowns`, `sentiment`, `sentiment_reasons`, plus `target_id`, `as_of` and a
`citations` map from evidence id to cite text — so a written report stays fully
sourced even after the pool moves on.

`Thesis` and `ThesisEvidence` (`theses.py`) own their tables, created
idempotently by every public function rather than in `core/db.py`.
`HealthState` is `emerging | building | mixed | weakening | challenged | idle`.

**Storage:** SQLite via SQLModel, one file at `db_path`. Tables: `instrument`,
`bar`, `newsitem`, `event`, `fundamental`, `llmcall`, `thesis`,
`thesis_evidence`. All timestamps stored UTC; convert at the render layer.
Columns added after the fact go through the `_ADDED_COLUMNS` /
`_ADDED_UNIQUE_INDEXES` migration hooks in `core/db.py` — there is no Alembic.

---

## 5. Plugin contracts (`core/plugin.py`)

```python
class Plugin:
    name: str  # unique, snake_case
    version: str = "0.1.0"
    enabled: bool = True
    shared_config: str | None = None  # borrow another plugin's table

    def configure(self, cfg: dict) -> None: ...  # receives [plugins.<name>]


class TargetPlugin(Plugin):  # instantiated once per [targets.<name>] table
    kind: str  # entry-point name in the `rigger.targets` group
    label: str = ""

    def instruments(self) -> list[Instrument]: ...


class MarketPlugin(Plugin):
    currency: str

    def universe(self) -> list[Instrument]: ...
    def is_open(self, ts) -> bool: ...  # no caller yet
    def next_open(self, ts) -> datetime: ...  # no caller yet


class DataPlugin(Plugin):
    market: str | None = None  # legacy sugar, folded into scope.markets
    scope: Scope = Scope()  # from [plugins.<name>].scope

    def universe(self, ctx) -> list[Instrument]:
        return self.scope.filter(ctx.universe)

    async def fetch(self, instruments, since) -> list[Bar | NewsItem | Fundamental | Event]: ...


@dataclass(frozen=True)
class Scope:  # AND across axes, OR within one; None = unrestricted
    watchlists: frozenset[str] | None = None
    asset_classes: frozenset[str] | None = None
    markets: frozenset[str] | None = None
    tags: frozenset[str] | None = None
```

`DataPlugin.__init_subclass__` wraps a subclass's `configure` so scope parsing
always runs — a subclass overriding `configure` cannot accidentally drop it.

**Discovery.** Three entry-point groups: `rigger.plugins` (one instance per
class, keyed by `name`), `rigger.targets` (a *factory* per kind — one class backs
many differently-named targets), and `rigger.watchlists` (legacy, still
honoured). Third-party plugins are ordinary pip packages using the same groups.

**Back-compat that must keep working:** `[watchlists.<name>]` tables and the
`[universe] <market> = [tickers]` shim both resolve into targets, with kind
inferred from shape when unstated. Do not migrate users' TOML.

---

## 6. LLM layer

One provider-agnostic `LLMClient`. The call is delegated to a `Provider`:
`openrouter` (one API, many models), `litellm` (SDK, calls vendors directly), or
`litellm-proxy`. The client owns caching keyed on
`sha256(model + prompt_version + prompt)`, persists every call to `llmcall`, and
applies the retry policy.

**Four routed tasks** in `[llm.routing]`: `extract`, `report`, `chat`, `thesis`.
`model_for(config, task, plugin=None)` resolves `[plugins.<plugin>].model` first,
then `[llm.routing].<task>`, and **raises `KeyError` on a miss** — a missing
route must never silently fall back to a hard-coded literal.

`catalog.py` fetches the provider's model list (id, name, context length, prices;
OpenRouter reuses the pricing fetch it already makes, cached 24h, failures
degrade to an empty catalog) and writes config back via `set_llm_route` and
`set_plugin_model`. The TUI model picker (`m`) reads it.

**Prompt rules.** Templates live in `llm/prompts/`, versioned by filename
(`report_v1.j2`); `prompt_version` is the filename without `.j2` and is recorded
on every call. Jinja runs with `StrictUndefined`. Prompts must forbid reasoning
from memory and require citation of supplied evidence ids. Structured calls go
through `structured.py`, which enforces a JSON schema and re-prompts once on a
validation failure.

---

## 7. Surfaces

**TUI** (`uv run rig`) — `1`–`6` Home, Data, Config, Reports, Theses, Chat;
`w` Targets; `c` Console; `m` model picker; `?` help; `q` quit.

**CLI** (`rig`) — `target add/remove/list/show`, `ingest`, `extract`,
`report <target>`, `thesis create/list/show/propose`, `llm costs`, `llm models`,
`plugins list/enable/disable`, `config show/validate`, `tui`.

Reports are written to `reports/<target_id>/<YYYY-MM-DD>.md`.

---

## 8. Coding standards

- Python 3.12+, type hints everywhere. `ruff` (line length 100) and
  `mypy --strict` on `rigger/core` and `rigger/llm`.
- Async for I/O (fetching, LLM calls); sync for CLI glue.
- **No network in tests.** `respx` for HTTP, `FakeLLM` for the model provider.
- Conventional commits (`feat:`, `fix:`, `docs:`, `chore:`).
- Never commit `.env`, `data/*.db` or `reports/`.
- CI runs `ruff check`, `mypy --strict rigger/core rigger/llm` and `pytest` on
  every push.

---

## 9. Known gaps

Recorded so they are not rediscovered as surprises:

- `MarketPlugin.is_open` / `next_open` have no caller anywhere, and both
  implementations are weekday-plus-time-of-day with no holiday calendar.
- `EventKind` is defined in three places (`core/models.py`, `extract.py`, and
  `llm/prompts/extract_v1.j2`). A non-equity target cannot add a kind without
  editing all three.
- `Instrument.watchlists` still carries the old name for what are now target ids.
- No evaluation layer: nothing yet measures whether the research is any good.
  That is the prerequisite for ever revisiting trading, not an optional extra.

## 10. Caveats

- Free data sources are rate-limited and sometimes wrong. A single citation is a
  lead, not a fact.
- A cited report is only as good as what was gathered; if ingest missed
  something, the model cannot know about it. That is the deliberate trade for
  never inventing facts.
- Models hallucinate. The citation contract is the defence, and it is why
  loosening it is never an acceptable fix for an empty report.
