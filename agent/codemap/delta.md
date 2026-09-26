# delta/ root modules

Domain workflows. Screens and workers call these (via `services.py`); they own persistence, LLM orchestration, and domain rules.

Depends on: `delta/core` (models, db, ids, plugin Context), `delta/llm` (client, structured).
Depended on by: `delta/tui` screens, `delta/runtime.py`, tests.

## Files

- `runtime.py` — composition root; `class Delta` wires config, engine, plugins, LLM; `RELOAD_PARTS = ("llm", "data_sources", "targets")`
- `services.py` — app operations: `ingest`, `extract`, `classify_sentiment`, `gather`, `add_target`/`remove_target`, `brief_for`, `pulse`, `upcoming_events`, `llm_costs`, `setup_checks`, `thesis_fleet`, `data_health`, `configure_data_provider`
- `evidence.py` — evidence pool: `EvidenceItem`, `evidence()`, `evidence_by_ids`, `cite`, `source_quality`, `falsifier_hit`
- `extract.py` — news → events: `extract_events` (prompt `extract_v1.j2`), `EventDraft`, `EventBatch`
- `reports.py` — cited reports: `build_report`, `gather`, `Claim`, `Report`/`ReportDraft`, `render_markdown`, `section_counts` (prompt `report_v2.j2`)
- `chat.py` — grounded Q&A: `ChatDraft`, `ChatMessage`, `WebHit`, `SearchTool`/`OfflineSearchTool`, `CHAT_PROMPT_VERSION = "chat_v1"`
- `theses.py` — thesis CRUD + own SQLModel tables: `Thesis`, `ThesisEvidence`, `ThesisTable`, `ThesisEvidenceTable`, `CandidateDraft`, `ThesisDraft`
- `thesis_health.py` — pure health computation: `compute_health`, `HealthResult`, `state_style`, `badge_text`
- `thesis_summary.py` — AI prose over accepted state: `summarize_thesis`, `SummaryDraft` (prompt `thesis_summary_v1.j2`)
- `decisions.py` — decision log + own tables: `Decision`, `DecisionReview`, `create_decision`, `DecisionTable`, `DecisionReviewTable`
- `sentiment.py` — news stance over time: `classify_news`, `SentimentSummary`, `stance_of`
- `review.py` — evidence staleness/coverage: `EvidenceAudit`, `ReviewItem`, staleness constants (`PRICE_STALE_AFTER` etc.)
- `brief.py` — morning brief: `build_brief`, `Section`, `Brief`
- `quotes.py` — quotes/search: `YahooQuotes`, `yahoo_search`, `Quote`, `canonical_symbol`, `SearchResult`
- `targets.py` — watch-target model: `WatchTarget`, `target_from_spec`, `TargetKind`, `LEGACY_KIND`
- `asset_metrics.py` — metric profiles per asset class: `AssetMetrics`, `profile_for`, `groups_for`

## To change Y, edit Z

- Add a new gathered-evidence kind → `evidence.py` (`EvidenceKind` literal) + relevant plugin + `core/db.py` tables
- Change report sections/rendering → `reports.py` + `delta/llm/prompts/report_v2.j2`
- Change ingest orchestration → `services.py` (`ingest`); plugin behaviour stays in `delta/plugins/data/`
- Add an app-level operation → `services.py`; screens only call it

## Gotchas

- `theses.py` and `decisions.py` create their own tables (`_ensure_tables`) — not in `core/db.py`.
- Never weaken `_supported` (claim filtering) in `reports.py` or citation checks in `chat.py`.
- `runtime.py` `__all__ = ["Delta"]` only.
