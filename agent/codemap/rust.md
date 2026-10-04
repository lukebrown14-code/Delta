# Rust rewrite code map

Plan: `docs/RUST_REWRITE_PLAN.md`. Findings: `docs/rewrite/findings/`.

## Crates

| Path | Status | Contents |
|---|---|---|
| `crates/delta-core/` | R1a done | `ids.rs` (stable_id, instrument ids), `models.rs` (Instrument, Bar, NewsItem, Event, Fundamental, LlmCall, enums), `json.rs` (list-column helpers), `format.rs` (Python `{:,.Nf}` thousands grouping, shared by metrics + brief), `time.rs` (naive-UTC helpers), `config.rs` (Settings, AppConfig, load/update config, .env read/write), `events.rs` (sync EventBus), `state.rs` (last-seen state), `db.rs` (rusqlite schema, migrations, news_instrument backfill, idempotent `store_items`, readers, `conn_mut` for transactions) |
| `crates/delta-llm/` | R1c done | `eval.rs` (citation validity), `json.rs` (fence parser), `router.rs` (model routing), `client.rs` (cache + cost log, `CompleteParams`), `providers.rs` (OpenAI-compat + OpenRouter via reqwest: pricing, credits auto-fit, 402 retry, `verify_key`), `structured.rs` (minijinja prompts, retry-once), `jev.rs` (Jev Decisions client: `python_dumps` payload hash, cache + llmcall logging), `catalog.rs` (model catalog disk cache + config.toml route writeback), `prompts/` (verbatim `.j2` copies) |
| `crates/delta-services/` | R2 plus live workflows | `analytics.rs` (data_health, pulse, upcoming, costs, closes, headline, latest_report), `pipeline.rs` (ingest, extract, `gather_configured` with partial-stage warnings), `evidence.rs` (pooled evidence and citations), `reports.rs` (validated cited reports and persistence), `chat.rs` (grounded Ask), `theses.rs` (thesis create/edit/status, AI candidate proposal, evidence acceptance), `decisions.rs` (decision journal and review history), `config_ops.rs` (plugin enable, provider/model save, markets, targets), `targets.rs` (WatchTarget), `sentiment.rs` (Jev stance), `brief.rs` (facts-only brief), `thesis_health.rs` (health and evidence reads); tests include `tests/reports.rs`, `tests/chat.rs`, `tests/journal.rs`, `tests/settings.rs` |
| `crates/delta-plugins/` | R1b plus configured registry | `plugin.rs` (DataPlugin trait, Scope, static registry, `configured_plugins` applies enabled/settings/scope), `rss.rs` (feed parse + Matcher), `sec.rs` (filings→news, XBRL facts, rate cap + retry), `asx.rs` (announcements, retries), `yahoo.rs` (own client: chart/quote/quoteSummary with cookie→crumb, search, live-quote state machine), `calendar.rs` (upcoming Yahoo calendar events), `markets.rs` (US/ASX universes + daylight saving sessions), `http.rs` (User-Agent); wiremock tests reuse `tests/fixtures/` |
| `crates/delta-tui/` | R0 + R1d plus live workflows | `lib.rs` (Component/Action, gather/report/chat/thesis proposal actions, quit keys), `main.rs` (component loop and headless `gather`, `report`, `review-due`; live Home, Watchlist, Research, Theses, Ask, Decisions and Settings; journal forms and plugin toggles), `screen.rs` and `screens.rs` (golden cell-grid painters retaining bold/reverse/italic/underline), `desk.rs` (config/DB desk, explicit unavailable state, offline seed for tests), `workers.rs` (quote/metrics/gather/report/chat/thesis proposal/home-refresh workers), `metrics_view.rs` (range-aware multi-asset metrics inspector), golden harness `tests/golden.rs` and populated Settings oracle `tests/settings_live.rs`; live parity coverage and R4 matrix remain incomplete |

## Shared fixtures

- `delta-services/src/asset_metrics.rs` exports `AssetMetrics`, `MetricGroups`, `fetch_asset_metrics`, `fetch_asset_metrics_configured`, `normalize_asset_metrics`, `group_values`, `profile_for`, `groups_for`, `metric_help`, and `range_spec`: all asset-class inspector cards, adjusted provider history with aligned timestamps, requested range intervals, volatility and position, empty-provider all-range local fallback, and displayed fetch errors. `delta-plugins/src/metrics_profiles.rs` provides ordered `KeySpec`/`GroupSpec` tables, `keys_for`, `groups_for`, and `METRIC_HELP` generated offline from canonical `delta/metrics.toml`, exposed through `metrics::profiles`.

- `delta-services/src/schemas.rs` exports `event_batch`, `report_draft`, `thesis_draft`, and `summary_draft`: exact JSON-schema response envelopes generated offline from canonical Python Pydantic models. Structured parsing returns `ProviderError::Validation` for invalid model output, so extraction skips validation failures while propagating provider failures. Stored evidence readers propagate malformed row and timestamp errors while skipping unknown IDs.

- `config_ops::configured_universe` resolves configured market currencies, legacy universe shims and merged target memberships for the desk and gather workers. Live feeds start by default; `DELTA_QUOTES=0` disables network quote polling.
- `config_ops::configure_data_provider` persists SEC contact and market scope while preserving other adapter settings. Settings exposes SEC setup and market forms; forms support previous-field navigation and retain values on save errors. Watchlist reload retains live quotes and metrics for remaining instruments.
- `delta-tui/src/research.rs` exports `ResearchBrowser`, `wrap`, and `truncate`: live evidence reads, kind filtering, search, pagination, selection and source previews. Research uses `e`/`r` to switch views, `/` to search, `k` to cycle kinds, and `l` to load more. Text wrapping uses terminal cell widths.
- `delta-tui/src/watchlist.rs` exports `WatchlistBrowser` and `WatchRow`: target groups by asset class, folding, filter text and multi-ticker member selection. Live Watchlist uses Python's `r`/`R`, `i`, arrows, `/`, space and escape bindings.
- `delta-tui/src/metrics_view.rs` paints the watchlist metrics inspector, including profile-specific metric groups, range tabs, axis formatting, chart and narrow detail scrolling. `Screen` cells and the terminal blitter preserve the oracle's bold, reverse, italic and underline attributes.
- Global `g` opens the Go picker, `?` shows help, `h` opens Home, `m` edits model settings and `p` connects a provider. `config_ops::{ProviderSetup, connect_provider}` and `workers::spawn_provider_setup` handle masked API-key setup, custom endpoints and asynchronous key verification; `delta-core::config::try_set_env_value` surfaces secret-file write errors. `Action::Goto` owns its string rather than leaking accepted picker values.
- `LlmClient::models`, `config_ops::model_catalog` and `workers::spawn_models` supply the searchable model picker (`m`) with cached fallback; `M` opens manual entry. Forms support Ctrl+S. `Screen::text` and `Cell::symbol` preserve wide and combining characters in live rendering.
- Decisions supports `d` then `y` for confirmed deletion and `o` to open Research; journal panes support `/` filtering. Theses uses `d` to edit and `x` to reject evidence, with status in its edit form.
- Home retains its pane layout and uses stored prices, real thesis health and agenda counts. `Desk::last_seen` and `load_feed_since` maintain the prior visit across reloads and worker refreshes. Yahoo `configured_suffixes` is shared by ingest and live workers.
- Research uses simultaneous company/report/evidence panes at wide sizes, drill-in views at narrow sizes, zoom (`z`), report history (`[`/`]`) and a deterministic brief (`b`). `markdown::render` provides prose display lines; `Screen::blit_at` composes pane content.
- `theses_view::{ThesisView, ThesisFocus}` renders live claims, computed health, framing and sourced evidence in three panes, with narrow drill-in focus (`t`/`e`) and citation previews. `L` opens manual evidence linking.
- `decisions_view::DecisionView` renders the decision list and timeline, original thesis snapshots and review history, with narrow Enter/Esc drill-in and scrolling. Home is the startup pane; arrows select and Enter opens the corresponding watch target. Quote actions merge only currently watched instruments, preserving last quotes during feed replacement.
- `ask_view::AskState` manages target scope, citations, sidebar focus and zoom. Ask supports target toggles, confirmed transcript clearing, and saving an answer as a thesis with its stored citations. `evidence::source_url` resolves HTTP sources for Research and Ask's `o` binding.

- Watchlist add/remove forms reload `Desk::open_at` and refresh managed `workers::Workers`; gather reads current targets on each request. Tests cover first-target creation, removal confirmation, empty-desk rendering and worker cancellation.

- `delta-services/src/evidence.rs` exports `evidence_filtered` for target, date, kind and text reads; `tests/evidence.rs` checks filtering before limiting.
- `delta-services/src/review.rs` provides evidence coverage audits, applicable primary sources and the deterministic review queue; `tests/review.rs` checks priority, falsifier matches and future evidence exclusion.
- `delta-services/src/thesis_summary.rs` drafts cited summaries from accepted evidence and computed health, with `summarize_thesis_configured` for workers; `tests/thesis_summary.rs` checks accepted citations and the empty-evidence path without an LLM call. The Theses pane opens summaries with `s`, using `workers::spawn_thesis_summaries` and summary actions.

- `fixtures/delta.db` — Python-created seed DB (`delta.core.db.init_engine` + `store_items`); opened and round-tripped by `crates/delta-core/tests/parity.rs`.
- `fixtures/golden_screens/` — golden screen exports for the R3 oracle (`tests/export_golden.py`).

## Conventions

- Datetimes: naive UTC (`NaiveDateTime`), stored as `YYYY-MM-DD HH:MM:SS.ffffff` — byte-compatible with SQLModel.
- IDs: `stable_id` sha256 over NUL-joined parts; stored hashes must stay Python-compatible.
- Checks: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`.
- Provider tests run against `wiremock` — no live LLM/network calls, ever.
- Findings: `docs/rewrite/findings/rust-core.md`, `docs/rewrite/findings/rust-llm.md`.

## Live rewrite integration

- `footer::FooterState` caches newest bar, cumulative spend and provider; `paint_at` renders Python shell freshness thresholds and navigation. Footer parity is checked against every static screen at all three sizes.
- `settings_view::{SettingsState,SettingsFocus,SettingsView,SettingsDiagnostics,SettingsSource}` provides pane focus, selected market editing, breakpoint-aware diagnostics and cached cost/health tables. `workers::spawn_settings` reads diagnostics in `spawn_blocking`; rendering performs no storage reads. `config_ops::provider_connected` exposes key availability without returning secrets.
- `workers::WorkRequest<T>` carries Run/Cancel for gather, reports and Ask. Gather accepts instrument IDs (empty = all); cancellation drops the owned service future, including pending plugin fetches. Busy actions suppress duplicate requests. Ask results carry a generation and cleared transcripts ignore obsolete completions.
- `ResearchBrowser` supports citation drill-in and scrolling; stored Ask citations navigate to Research, while web citations open their HTTP source. Thesis framing arrows scroll; Shift+arrows scroll ledger notes. Journal navigation clears filters from the preceding pane.
- `schemas::{event_batch,report_draft,thesis_draft,summary_draft}` returns exact canonical Pydantic response envelopes. `ProviderError::Validation` separates unusable structured output from provider/network errors, which extraction propagates. Evidence reads propagate malformed rows rather than skipping them.
