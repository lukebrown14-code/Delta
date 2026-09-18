# Delta Architecture Context

## Purpose

Delta is a terminal-first investment research assistant. It turns a user’s
watchlist, market data, filings, announcements, RSS feeds, and LLM analysis
into traceable research reports, theses, and decision records. It is designed
to support research, not to execute trades or give personalised advice.

## Package map

The canonical package is `delta` (the project is currently migrating from the
former `rigger` name). New code, imports, entry points, and documentation should
use `delta`; do not introduce new `rigger` references.

| Area | Responsibility |
|---|---|
| `delta/core` | Foundation types and infrastructure: typed configuration, SQLModel database/session access, domain models, identifiers, time/JSON helpers, HTTP, events, and plugin protocols. |
| `delta/plugins` | Extensible market, data-source, and target adapters. Entry points let built-ins and user-installed plugins participate without hard-coding each provider. |
| `delta/services.py` | Application service layer. Coordinates config persistence, target management, market/data-source setup, and user-facing operations so the TUI does not manipulate storage directly. |
| `delta/runtime.py` | Composition root: builds the configured engine, providers, plugin registry, and runtime dependencies used by screens and services. |
| `delta/evidence.py` | Stores and retrieves source material with identity, provenance, timestamps, and citations. |
| `delta/reports.py`, `delta/theses.py`, `delta/review.py`, `delta/decisions.py` | Research-domain workflows: report generation, investment theses, evidence review, and a durable decision journal. |
| `delta/quotes.py`, `delta/asset_metrics.py`, `delta/brief.py`, `delta/chat.py`, `delta/extract.py` | Market quotes/metrics, briefs, conversational research, and structured extraction. |
| `delta/llm` | Provider setup, model catalog/routing, request client, structured responses, and Jinja prompt templates. LLM calls belong behind this boundary. |
| `delta/tui` | Textual application shell, design primitives, theme/CSS, and screens. This is presentation and interaction logic, not the place for persistence/business rules. |
| `tests` | Offline-first executable contract. Tests use fakes, temporary config/database state, and mocked HTTP rather than live financial or LLM services. |

## Runtime and data flow

```text
TUI screen
   │  user action
   ▼
services / domain workflow ──► runtime + core configuration
   │                                  │
   ├──► plugins (markets, sources)    ├──► database
   ├──► LLM router / prompts          └──► configured providers
   ▼
evidence, reports, theses, decisions
   │
   └──► TUI renders provenance and citations
```

The important rule is provenance: a report or decision should lead back to its
claims and the evidence records that support or challenge them.

## TUI responsibilities

`RiggerApp`/the application shell registers screens and global navigation.
Shared widgets in `delta/tui/widgets.py` and shell/theme modules define the
terminal visual language; screens compose those primitives rather than creating
their own styling system.

Key screens:

- **Home / Targets:** watchlist and current market context.
- **Research:** Company → Report → Evidence desk. The report is the central
  working surface; citations select evidence in the same screen.
- **Theses / Decisions:** assess an investment case, track supporting and
  contrary evidence, and preserve the decision rationale.
- **Configuration / source setup / market setup:** connect providers and add
  country- or provider-specific markets and sources.
- **Chat:** ask bounded questions across selected research context.

Screen code should call services/domain workflows, keep long-running work async
via Textual workers, and refresh visible state after completion. Do not perform
blocking network calls in event handlers.

## Coding patterns

### Type boundaries

- Prefer typed domain objects and Pydantic/SQLModel models over unstructured
  dictionaries at module boundaries.
- Keep parsing and provider-specific formats inside their plugin/adapter.
- Use canonical instrument identities (`MARKET:SYMBOL`) and normalisation helpers
  rather than hand-built ticker strings.

### Plugins and configuration

- Add a new market/source as a plugin implementing the relevant core protocol.
- Discover plugins through the registry/entry points; do not add provider
  conditionals across unrelated screens.
- Treat API keys as configuration/secrets: read them through the configured
  provider path and never persist or render their raw values.

### Persistence and workflows

- Put writes behind service/domain functions; UI code triggers the operation and
  renders the result.
- Preserve source URL, provider, retrieval time, and stable identifiers with
  evidence. Citations should reference evidence IDs, not display text.
- Reports, theses, and decisions are distinct records. Link them explicitly
  rather than copying content between tables.

### TUI conventions

- Reuse `Pane`, dialogs, key hints, tables, and theme tokens. Avoid raw hex
  colours and stock controls where a shared primitive exists.
- Keep keyboard actions visible and usable; narrow layouts use a deliberate
  drill-in flow rather than clipping columns.
- Guard empty tables/lists before reading cursor/highlight state.

### Tests and quality gates

- Add or update tests alongside behaviour changes; mock remote providers and
  use temporary paths for config/database tests.
- Run `uv run pytest` and `uv run ruff check .` before handoff.
- Preserve existing public widget IDs, constructor signatures, and navigation
  bindings unless the change intentionally updates their tests and docs.

## Current branch and migration state

`feat/upgrade-1` is the integration branch. It contains the committed
decision-review and configurable-data-source work, plus an in-progress package
rename from `rigger` to `delta`.

The rename touches imports, entry points, test imports, package-relative asset
paths, and all modules added by decision review. Treat it as a single migration:

1. Resolve each changed module in `delta`, retaining both its rename/refactor
   changes and the decision-review behaviour.
2. Move newly added decision modules and TUI screens into the `delta` package.
3. Update imports, plugin entry points, CLI references, fixtures, and docs to
   the new package name.
4. Remove the old `rigger` paths only after `uv run pytest` and
   `uv run ruff check .` pass from a clean worktree.

Avoid broad search-and-replace and do not delete the legacy paths merely to
silence import errors; preserve working behaviour and provenance links while
resolving the migration.
