# Rust rewrite code map

Plan: `docs/RUST_REWRITE_PLAN.md`. Findings: `docs/rewrite/findings/`.

## Crates

| Path | Status | Contents |
|---|---|---|
| `crates/delta-core/` | R1a done | `ids.rs` (stable_id, instrument ids), `models.rs` (Instrument, Bar, NewsItem, Event, Fundamental, LlmCall, enums), `json.rs` (list-column helpers), `format.rs` (Python `{:,.Nf}` thousands grouping, shared by metrics + brief), `time.rs` (naive-UTC helpers), `config.rs` (Settings, AppConfig, load/update config, .env read/write), `events.rs` (sync EventBus), `state.rs` (last-seen state), `db.rs` (rusqlite schema, migrations, news_instrument backfill, idempotent `store_items`, readers) |
| `crates/delta-llm/` | R1c done | `eval.rs` (citation validity), `json.rs` (fence parser), `router.rs` (model routing), `client.rs` (cache + cost log, `CompleteParams`), `providers.rs` (OpenAI-compat + OpenRouter via reqwest: pricing, credits auto-fit, 402 retry, `verify_key`), `structured.rs` (minijinja prompts, retry-once), `jev.rs` (Jev Decisions client: `python_dumps` payload hash, cache + llmcall logging), `catalog.rs` (model catalog disk cache + config.toml route writeback), `prompts/` (verbatim `.j2` copies) |
| `crates/delta-services/` | R2 done | `analytics.rs` (data_health, pulse, upcoming, costs, closes, headline, latest_report), `pipeline.rs` (ingest with bar-since floor, extract_events via delta-llm, gather), `config_ops.rs` (plugin enable, markets, targets), `targets.rs` (WatchTarget), `sentiment.rs` (Jev news-stance classify + weighted `stock_sentiment`, `sentiment` table upsert), `brief.rs` (facts-only brief incl. `{:,.2f}` value magnitudes, Phase 1 price-section golden), `thesis_health.rs` (pure `compute_health`, `evidence_by_ids`, `thesis_fleet` + `thesis`/`thesis_evidence` ensure-tables); parity tests run delta.services goldens on identically seeded data |
| `crates/delta-plugins/` | R1b done | `plugin.rs` (DataPlugin trait, Scope, static registry), `rss.rs` (feed parse + Matcher), `sec.rs` (filings→news, XBRL facts, rate cap + retry), `asx.rs` (announcements, retries), `yahoo.rs` (own client: chart/quote/quoteSummary with cookie→crumb, search, live-quote state machine), `markets.rs` (US/ASX universes + sessions), `http.rs` (User-Agent); wiremock tests reuse `tests/fixtures/` |
| `crates/delta-tui/` | R0 + R1d done | `lib.rs` (Component/Action, quit keys), `axes.rs` (nice ticks, price/x labels), `braille.rs` (connected braille line, `py_round`), `chart.rs` (`PriceChart._runs` layout, direction colours), `theme.rs` (delta-dark tokens), `table.rs` (zebra + cursor), `dialog.rs` (Dialog + ModalStack), `components.rs` (EmptyState, SectionHeading, SuggestionList, CommandPalette, WhichKey); binary `main.rs` (component loop, footer + modal shell); `screen.rs` (golden cell-grid model), `screens.rs` (Watchlist inspector painter, Tier A zero-mismatch at 120x40); `desk.rs` (desk state from config/DB, offline seed, `load_feed` Home analytics, `HomeFeed` in `screens.rs`); `workers.rs` (quote/metrics/gather/home-refresh workers over the Action bus); golden harness `tests/golden.rs`; Home + glossary painters; `delta-services/src/fixture.rs` (offline desk seed) |

## Shared fixtures

- `fixtures/delta.db` — Python-created seed DB (`delta.core.db.init_engine` + `store_items`); opened and round-tripped by `crates/delta-core/tests/parity.rs`.
- `fixtures/golden_screens/` — golden screen exports for the R3 oracle (`tests/export_golden.py`).

## Conventions

- Datetimes: naive UTC (`NaiveDateTime`), stored as `YYYY-MM-DD HH:MM:SS.ffffff` — byte-compatible with SQLModel.
- IDs: `stable_id` sha256 over NUL-joined parts; stored hashes must stay Python-compatible.
- Checks: `cargo fmt --all`, `cargo clippy --all-targets -- -D warnings`, `cargo test --workspace`.
- Provider tests run against `wiremock` — no live LLM/network calls, ever.
- Findings: `docs/rewrite/findings/rust-core.md`, `docs/rewrite/findings/rust-llm.md`.
