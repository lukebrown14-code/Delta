# AGENTS

Delta is a Rust terminal app for personal investment research. Data plugins gather evidence into SQLite; model services produce cited reports and answers; theses and decisions track longer-term ideas.

Read `PROJECT_SPEC.md` before changing domain contracts, plugin behavior, or provenance rules. `docs/rewrite/` records Rust migration decisions and frozen Python parity references.

## Commands

```bash
cargo run -p delta-tui                 # launch
cargo test -p delta-tui --test live_golden  # screen parity
cargo test --workspace                # all offline tests
cargo fmt --check                     # format gate
cargo clippy --all-targets -- -D warnings  # lint gate
cargo build --release -p delta-tui    # release binary
```

## Layout and boundaries

- `crates/delta-core`: config, IDs, domain models, SQLite, migrations, events.
- `crates/delta-plugins`: markets, data sources, metrics, plugin registry.
- `crates/delta-llm`: model providers, routing, caching, versioned prompts.
- `crates/delta-services`: application operations, evidence, reports, chat, theses, decisions.
- `crates/delta-tui`: terminal screens, components, input, workers, headless CLI.
- `fixtures/`: frozen offline data and Python screen oracle. `docs/rewrite/`: cutover evidence.
- `data/` and `reports/`: local generated output; never commit.

Screens call services; providers and storage stay behind service boundaries. Network and model work runs asynchronously through workers. Keep evidence ID validation on report claims and chat answers. Thesis candidates require user acceptance before they affect health.

## Testing and edits

Tests are offline and use fixed fixtures, temporary databases, and mocked HTTP/model responses. Run a focused test during development, then the Rust CI gates before completion. Keep diffs scoped and use conventional commit messages.

`config.toml` holds targets, routing, plugins, and paths. `.env` holds secrets. SEC ingest requires a real contact email in `[plugins.sec_edgar] contact`.

Do not read or search `.venv/`, caches, `data/*.db`, `reports/`, or generated TUI shots. Do not edit `.env`, `data/model_catalog.json`, or generated golden fixtures by hand. Ask before adding dependencies, changing config or CI, committing, pushing, or running destructive commands. Do not commit `agent/HANDOFF.md` or `agent/PLAN.md`.

If `agent/HANDOFF.md` exists, read its Next section at session start. Remove it after its task is complete. Keep replies short and lead with the result.

Issues are tracked in GitHub Issues; see `docs/agents/issue-tracker.md`. Use `docs/agents/triage-labels.md` for triage labels and `docs/agents/domain.md` for domain terms.
