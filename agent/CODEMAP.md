# Delta — Code Map

Personal investment research assistant: data plugins gather evidence into SQLite, an LLM turns it into cited reports/chat, an optional thesis layer tracks long-horizon ideas. Python 3.12 Textual TUI.

| Path | Responsibility | Key entry file | Map |
|---|---|---|---|
| `delta/` root modules | Domain workflows: ingest, extract, reports, chat, theses, decisions | `delta/services.py` | `agent/codemap/delta.md` |
| `delta/core/` | Config, DB schema, domain models, plugin protocols, IDs, events | `delta/core/plugin.py` | `agent/codemap/delta/core.md` |
| `delta/llm/` | LLM client, providers, routing, prompts, caching, eval | `delta/llm/client.py` | `agent/codemap/delta/llm.md` |
| `delta/plugins/` | Discovered markets, data sources, watch-target kinds | `pyproject.toml` entry-points | `agent/codemap/delta/plugins.md` |
| `delta/tui/` | Textual app, screens, widgets, charts, theme | `delta/tui/app.py` | `agent/codemap/delta/tui.md` |
| `tests/` | Offline pytest suite, snapshot baselines, FakeLLM | `tests/conftest.py` | `agent/codemap/tests.md` |
| `docs/` | Design notes (UI audit, Rust rewrite plan) | — | — |

## Where to find X

- App entry point / launch → `delta/tui/app.py` (`run_tui`; script `delta` in `pyproject.toml`)
- Runtime wiring / composition root → `delta/runtime.py` (`class Delta`, `RELOAD_PARTS`)
- App operations (ingest, targets, pulse, costs) → `delta/services.py`
- DB schema / migrations → `delta/core/db.py` (SQLModel tables, `_migrate`; no Alembic)
- Theses + decisions tables → `delta/theses.py`, `delta/decisions.py` (own their SQLModel tables)
- Domain models (Instrument, Bar, NewsItem, Event, Fundamental) → `delta/core/models.py`
- Config loading / `config.toml` / `.env` writes → `delta/core/config.py`
- Stable IDs → `delta/core/ids.py` (`stable_id`, `make_instrument_id`)
- Plugin protocols + discovery → `delta/core/plugin.py` (`DataPlugin`, `MarketPlugin`, `TargetPlugin`, `Scope`)
- Register a plugin → `[project.entry-points]` in `pyproject.toml` (`delta.plugins`, `delta.targets`)
- Add a data source → new `delta/plugins/data/<name>.py` (subclass `DataPlugin`)
- Add a market → new `delta/plugins/markets/<name>.py` (subclass `MarketPlugin`)
- Add a watch-target kind → `delta/plugins/targets/tickers.py` (e.g. `SectorTarget`)
- Watch-target model / parsing → `delta/targets.py` (`WatchTarget`, `target_from_spec`)
- LLM client construction / keys → `delta/llm/client.py` (`build_client`), `delta/llm/providers.py` (`PROVIDERS`)
- Per-task model routing → `delta/llm/router.py` (`model_for`), `[llm.routing]` in `config.toml`
- Prompt templates → `delta/llm/prompts/*.j2` (versioned; never weaken grounded/cited rules)
- Model catalog / picker data → `delta/llm/catalog.py` (caches `data/model_catalog.json`)
- Citation validation / eval → `delta/llm/eval.py` (`valid_citations`, `hallucinated_citations`)
- Reports (claims, draft, markdown) → `delta/reports.py` (`build_report`, `render_markdown`)
- Evidence pool / citation ids → `delta/evidence.py` (`EvidenceItem`, `evidence`, `evidence_by_ids`)
- Grounded chat → `delta/chat.py` (`ChatDraft`, `SearchTool`, prompt `chat_v1`)
- Event extraction from news → `delta/extract.py` (`extract_events`, prompt `extract_v1.j2`)
- Sentiment judgments → `delta/sentiment.py` + `delta/llm/jev.py` (`JevClient`)
- Theses CRUD → `delta/theses.py`; health → `thesis_health.py`; AI summary → `thesis_summary.py`
- Decision log / reviews → `delta/decisions.py`
- Quotes / symbol search → `delta/quotes.py` (`YahooQuotes`, `yahoo_search`)
- Morning brief → `delta/brief.py` (`build_brief`)
- Evidence staleness / coverage review → `delta/review.py`
- TUI screens → `delta/tui/screens/<name>.py`; registered in `delta/tui/app.py` (`_screens` dict)
- Theme / colors → `delta/tui/theme.py` (`DELTA_DARK`); stylesheet `delta/tui/delta.tcss`
- Charts → `delta/tui/axes.py` (`nice_ticks`), `delta/tui/widgets.py` (`BrailleGraph`, `PriceChart`)
- Test fixtures (FakeLLM, `seed_bars`) → `tests/conftest.py`
- TUI snapshot baselines → `tests/__snapshots__/` (update: `--snapshot-update`)

## Request / data flow

- Ingest: screen/worker → `services.ingest` → `DataPlugin.fetch` → db tables (`BarTable`, `NewsItemTable`, `FundamentalTable`)
- Extract: `services.extract` → `extract.extract_events` → `llm/structured` (`extract_v1.j2`) → `EventTable`
- Sentiment: `services.classify_sentiment` → `sentiment.classify_news` → `llm/jev.JevClient` → `SentimentTable`
- Report: `services.gather` → `evidence.evidence` → `reports.build_report` → LLM (`report_v2.j2`) → claims filtered against gathered ids → `reports/<target>/<date>.md`
- Chat: `tui/screens/chat` → `chat.py` → LLM → citations machine-checked against evidence ids
- Thesis: `tui/screens/thesis_form` → `theses.py` → `thesis_health.compute_health` (pure) → `thesis_summary.summarize_thesis`
- TUI: `runtime.Delta` → `tui/app.DeltaApp` → screens call `delta/services.py` (never storage/providers directly)

## Naming conventions

- Domain concept → `delta/<concept>.py`; storage tables mostly in `core/db.py` (exceptions: `theses.py`, `decisions.py`)
- LLM task → template `delta/llm/prompts/<task>_v<N>.j2`; `PROMPT_VERSION` = template minus `.j2`
- Screen → `delta/tui/screens/<name>.py`, class inherits `DeltaScreen` (`delta/tui/shell.py`)
- Test → `tests/test_<module>.py`; screen interaction tests often `test_<screen>_tui.py` or `test_<screen>.py`
- SQLModel tables end in `Table`; pydantic API models don't (`Thesis` vs `ThesisTable`)
