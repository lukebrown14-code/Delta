# delta/core

Foundation: configuration, SQLModel database, domain models, IDs, HTTP helper, event bus, plugin protocols. Everything else depends on it; it depends on nothing in-repo except sqlmodel/pydantic.

## Files

- `config.py` — config + secrets: `Settings` (pydantic-settings), `AppConfig`, `MarketConfig`, `load_config`, `build_config`, `update_config`, `read_env_value`/`set_env_value` (.env), paths `CONFIG_PATH`/`ENV_PATH`
- `db.py` — SQLite engine + schema: `init_engine`, `_migrate` (post-release columns/indexes hook), tables `BarTable`, `NewsItemTable`, `NewsInstrumentTable`, `EventTable`, `FundamentalTable`, `LLMCallTable`, `SentimentTable`
- `models.py` — pydantic domain models: `Instrument`, `Bar`, `NewsItem`, `Event`, `Fundamental`, `LLMCall`
- `plugin.py` — plugin layer: `Plugin`, `DataPlugin`, `MarketPlugin`, `TargetPlugin`, `Context`, `Scope`, `parse_scope`, `discover_plugins`/`discover_targets` (entry-point groups `delta.plugins`/`delta.targets`), `DataProviderSpec`/`DataProviderField`
- `ids.py` — `stable_id`, `make_instrument_id`
- `events.py` — `EventBus` (logs handler failures via `logging.getLogger("delta.events")`)
- `http.py` — `user_agent(extra)`
- `json.py` — `to_json`/`from_json` (list<->string columns)
- `state.py` — first-run/last-seen state file: `read_last_seen`, `write_last_seen`, `state_path` (`data/.delta_state.json`)
- `time.py` — `to_utc`, `parse_date`

## To change Y, edit Z

- Add a table → `db.py` (SQLModel class); post-release columns/indexes via `_migrate` hook lists (no Alembic)
- Add a config key → `AppConfig`/`build_config` in `config.py` + `config.toml`
- Change plugin contract → `plugin.py` protocol classes; then all of `delta/plugins/`
- Add env var handling → `config.py` (`Settings`, `read_env_value`)

## Gotchas

- mypy strict is enforced on this package in CI.
- `init_engine` applies pragmas + runs migrations on connect; tests get fresh DBs via `tmp_path`.
- Plugin discovery is by entry-point, not import scanning — new plugins must register in `pyproject.toml`.
