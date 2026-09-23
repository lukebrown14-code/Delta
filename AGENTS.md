# Agent Instructions

## Source Of Truth

- Read `PROJECT_SPEC.md` before changing architecture, domain contracts, plugin behavior, or provenance/citation rules; it is the repository's detailed source of truth.
- `delta` is the canonical package. Do not introduce the former `rigger` name in imports, entry points, or documentation.

## Structure And Boundaries

- `delta/runtime.py` is the composition root; `delta/services.py` owns application operations and persistence-facing workflows; TUI screens should call those services rather than manipulate storage or providers directly.
- `delta/core` contains configuration, SQLModel/database infrastructure, domain models, IDs, HTTP, events, and plugin protocols. `delta/plugins` implements discovered markets, data sources, and targets.
- Plugins are registered through the `delta.plugins` and `delta.targets` entry-point groups in `pyproject.toml`; add a plugin instead of adding provider-specific conditionals to screens or services.
- LLM calls belong behind `delta/llm`; prompt templates are versioned files under `delta/llm/prompts/` and must preserve grounded, cited behavior.
- SQLite is initialized/migrated in `delta/core/db.py`; there is no Alembic. Add post-release columns/indexes through its migration hook lists.

## Configuration And Data

- Runtime settings are split between `config.toml` (targets, routing, plugin settings, paths) and `.env` (secrets). Never commit `.env`, `data/*.db`, or generated `reports/` output.
- The default runtime writes to `data/delta.db` and `reports/`; tests should use `tmp_path` databases and must not depend on repository data.
- Preserve provenance: evidence, report claims, and chat answers must reference gathered evidence IDs. Do not weaken citation validation to make an empty or invalid result pass.

## Tests And Checks

- Tests are offline. Use `respx` for HTTP and the shared `FakeLLM` fixture for model calls; do not call live financial, news, SEC, or LLM services from tests.
- Run a focused test with `uv run pytest tests/test_reports.py -q` (replace the path or add `-k` as needed), then run the full suite with `uv run pytest`.
- Match CI's verification order: `uv run ruff check .`, `uv run mypy --strict delta/core delta/llm`, then `uv run pytest`.
- Ruff is configured for Python 3.12 with a 100-character line length; formatting is `uv run ruff format .` when formatting is needed.

## Running The App

- Install/sync with `uv sync` (CI uses `uv sync --all-groups`), configure a provider key in `.env`, then launch with `uv run delta`.
- Network and LLM work is asynchronous and should run through workers/services, not blocking Textual event handlers.

## Responses

- Keep responses short and simple; use bullet points where possible.
