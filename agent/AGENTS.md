# Delta — Agent Instructions

Personal investment research assistant: data plugins gather evidence into SQLite, an LLM turns it into cited reports and chat, an optional thesis layer tracks long-horizon ideas. Python 3.12 Textual TUI app, managed with uv.

`PROJECT_SPEC.md` is the detailed source of truth. Read it before changing architecture, domain contracts, plugin behavior, or provenance/citation rules.

## Commands

```bash
uv sync                          # install (CI uses: uv sync --all-groups)
uv run delta                     # launch the TUI
uv run pytest                    # full test suite
uv run pytest tests/test_reports.py -q        # one file
uv run pytest tests/test_reports.py -k name   # one test
uv run ruff check .              # lint (CI order: ruff -> mypy -> pytest)
uv run ruff format .             # format
uv run mypy --strict delta/core delta/llm     # type-check (strict scope = these dirs)
uv run pytest tests/test_tui.py --snapshot-update  # re-baseline TUI snapshots
```

## Project structure

- `delta/` — the canonical package. Never reintroduce the former `rigger` name.
  - `runtime.py` — composition root (wires config, engine, plugins, LLM).
  - `services.py` — application operations and persistence-facing workflows.
  - `core/` — config, SQLModel/db, domain models, IDs, HTTP, events, plugin protocols.
  - `llm/` — client, router, providers, cache; prompts are versioned `llm/prompts/*.j2`.
  - `plugins/` — discovered markets (`markets/`), data sources (`data/`), targets (`targets/`).
  - `tui/` — Textual app, screens, widgets, theme, stylesheet (`delta.tcss`).
  - domain modules at package root: evidence, reports, chat, theses, decisions, sentiment.
- `tests/` — offline pytest suite; `__snapshots__/` TUI baselines; `fixtures/`.
- `docs/` — design notes (UI audit, Rust rewrite plan).
- `data/`, `reports/` — runtime output (databases, generated reports); not committed.

## Architecture notes

- Entry point: `delta = delta.tui.app:run_tui` (pyproject).
- Plugins register via the `delta.plugins` / `delta.targets` entry-point groups in `pyproject.toml`. Add a plugin instead of provider-specific conditionals in screens or services.
- TUI screens call `delta/services.py`; they never touch storage or providers directly.
- SQLite is initialized/migrated in `delta/core/db.py`; there is no Alembic. Post-release columns/indexes go through its migration hook lists. Default DB: `data/delta.db`.
- Provenance is enforced: evidence, report claims, and chat answers must reference gathered evidence IDs. Never weaken citation validation to make an empty or invalid result pass.
- Network/LLM work is async and runs through workers/services, never blocking Textual event handlers.
- Config: `config.toml` (targets, routing, plugins, paths) + `.env` (secrets). Env var names: `OPENROUTER_API_KEY`, `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`, `CUSTOM_API_KEY`, `LIVE_TRADING`.

## Conventions

- Ruff: Python 3.12, 100-char lines, rules E/F/I/UP/B (E501 ignored).
- mypy strict in CI for `delta/core` and `delta/llm`; keep types clean elsewhere too.
- Types: `from __future__ import annotations`, modern syntax (`X | None`).
- Logging: `logging.getLogger("delta.<module>")`; `.exception()` inside handlers.
- Errors: catch specific exceptions (e.g. `httpx.HTTPError`) at plugin boundaries.
- Commits: conventional style — `feat(tui):`, `fix(llm):`, `test:`, `docs:`, `merge:`. Branch: `main`.

## Testing

- All tests are offline: `respx` mocks HTTP, the shared `FakeLLM` fixture (in `tests/conftest.py`) mocks the model. Never call live financial, news, SEC, or LLM services from tests.
- Tests use `tmp_path` databases; they must not depend on repository `data/` contents.
- TUI snapshot baselines live in `tests/__snapshots__/`; re-baseline with `--snapshot-update` only when a UI change is intentional.

## Gotchas / do-not-touch

- `.env`, `data/*.db`, `data/model_catalog.json`, `reports/` are local/generated — never commit or hand-edit.
- `tests/__snapshots__/` is generated; update via the snapshot flag, not by hand.
- `uv.lock` is a lockfile; don't edit by hand.
- SEC ingest needs a real contact email in `config.toml` (`[plugins.sec_edgar] contact`) — SEC User-Agent rule.
- `docs/RUST_REWRITE_PLAN.md` records a possible rewrite; don't start it unprompted.

## Do not

- Commit `agent/HANDOFF.md` or `agent/PLAN.md` (they're gitignored).
- Read or search: `.venv/`, `.mypy_cache/`, `.ruff_cache/`, `.pytest_cache/`, `uv.lock`, `__pycache__/`, `data/*.db`, `reports/`, `tui-shots*/`, `tests/__snapshots__/`
- Edit: `tests/__snapshots__/`, `data/model_catalog.json`, `uv.lock`, `.env`
- Run the full test suite when a single test or file will do. Use the single-test command above.
- Reformat, rename, or "clean up" code you weren't asked to change.
- Add dependencies, change configs, or touch CI without asking.
- Commit, push, or run destructive commands (rm -rf, db resets, force-push) without asking.
- Guess at commands. Use the ones in this file.
- Create agent files (plans, maps, handoffs, notes) outside agent/.

## Token efficiency

- Quiet commands:
  - `uv run pytest -q --tb=short`
  - `uv run pytest tests/test_reports.py -q -k name`
  - `uv run mypy --strict delta/core delta/llm 2>&1 | tail -50`
- Trim output: pipe long output through `| tail -50`, or filter failures only with `| rg -n "FAIL|Error|error"`.
- Find before reading: use `rg -l` to find files, then `rg -n` to find lines.
- Read partially: for files over ~300 lines, read only the relevant range with offset/limit. Don't read whole large files.
- Don't re-read files already read this session unless they've changed.
- Don't edit agent/AGENTS.md mid-session unless asked (it breaks the prompt cache).

## Code navigation

- Before searching, read agent/CODEMAP.md, then the relevant agent/codemap/<module>.md. Search only if the maps don't answer the question.
- After adding, moving, renaming, or deleting files or exported symbols, update the affected map entries in the same change.
- If a map entry turns out to be wrong, fix it.

## Workflow

- If `agent/HANDOFF.md` exists, read it at the start of a session and continue from "Next". Delete it once the task is done.
- Run tests and lint before saying you're done.
- Keep diffs minimal.
- Ask before adding dependencies.
- Replies: short, concise, simple language. Lead with the answer or result.
