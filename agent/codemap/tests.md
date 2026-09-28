# tests

Offline pytest suite (~40 files, ~413 tests). Mirrors module names. No live network, no LLM calls.

## Fixtures (tests/conftest.py)

- `FakeLLM` — duck-typed `LLMClient` stand-in; canned JSON per task; records `calls`
- `FakeConfig` — minimal `AppConfig` for routing strategies
- `seed_bars(engine, instrument_id, ...)` — inserts daily `BarTable` rows

## Module → test file

- `delta/services.py` → `test_services.py`, `test_watch_targets.py`
- `delta/reports.py` → `test_reports.py`; `delta/evidence.py` → `test_evidence.py`; `delta/extract.py` → `test_extract.py`
- `delta/chat.py` → `test_chat.py`; `delta/brief.py` → `test_brief.py`; `delta/review.py` → `test_review.py`
- `delta/theses.py` → `test_theses.py`; `thesis_health.py` → `test_thesis_health.py`; `thesis_summary.py` → `test_thesis_summary.py`
- `delta/decisions.py` → `test_decisions.py` (+ `test_decisions_tui.py` for the screen)
- `delta/sentiment.py` → `test_sentiment.py`; `delta/quotes.py` → `test_quotes.py`; `delta/asset_metrics.py` → `test_asset_metrics.py`
- `delta/llm/` → `test_llm_client.py`, `test_router.py`, `test_eval.py`, `test_jev.py`, `test_catalog.py`, `test_openrouter_provider.py`
- `delta/core/plugin.py` Scope → `test_scope.py`, `test_market_scope.py`
- `delta/plugins/data/` → `test_data_sources.py`, `test_rss.py`, `test_sec_edgar.py`, `test_asx_announcements.py`, `test_yfinance_calendar.py`
- `delta/plugins/markets/asx.py` → `test_asx_market.py`
- `delta/tui/` → `test_tui.py` (app), `test_snapshots.py` (full-app snapshots), `test_home.py`, `test_research.py`, `test_chart.py`/`test_chart_axes.py` (widgets/axes), `test_targets_inspector.py`, `test_market_setup.py`, `test_provider_setup.py`
- `tests/export_golden.py` — golden-screen exporter (Rust rewrite oracle): CLI dumps watchlist states as JSON cell grids with resolved RGB to `fixtures/golden_screens/` (+ `manifest.json`, Tier A/B tags); includes a determinism test

## Conventions

- HTTP mocked with `respx`; models with `FakeLLM`; clocks with `time-machine` where needed.
- Databases: `tmp_path` + `init_engine`; never rely on repo `data/`.
- Parametrize with `@pytest.mark.parametrize` for variant cases.

## Gotchas

- `__snapshots__/` is generated — update with `--snapshot-update`, never by hand; snapshots are machine-sensitive (re-baselined for determinism before).
- Full suite is the slow path; run one file with `-q` while iterating.
