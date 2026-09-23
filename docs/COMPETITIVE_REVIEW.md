# Competitive Review — Fincept & Finterm → Delta feature plan

> Findings from reviewing https://fincept.in/ and https://finterm.ai/ against
> Delta, and an implementation plan for the ideas worth adopting.
> Scope rule for everything below: **harness work only** — deterministic
> computation and gathering, never model judgment (PROJECT_SPEC.md §2).

---

## 1. Findings

### What each app is

| | Delta | Fincept Terminal | Finterm |
|---|---|---|---|
| Audience | hobbyist investor | professional desk | agent developers |
| Form | TUI app (Textual) | 41-module terminal + desktop app | CLI (`npm i -g @finterm-ai/cli`) consumed by coding agents |
| Data | free sources: yfinance, RSS, SEC EDGAR, ASX | 2,000+ sources, real-time feeds, broker links | curated structured bundles over filings, ownership, options |
| AI role | synthesise + challenge, never tips | multi-agent research, user-built agents, MCP | none — data for someone else's agent |
| Trading | explicitly a non-goal | live execution, algos, bracket/OCO orders | none |
| Citation | enforced contract, machine-checked, tested | "sourced note" from research agents | links, timestamps and caveats retained on findings |
| Price | free / open source | open source + $10–40/user/mo Enterprise | $150–200/mo |

### What they share
- All three refuse to give advice; sourcing is the headline feature in each pitch.
- Delta and Fincept share the evidence → report shape; Delta and Finterm share
  the "structured, machine-checkable output" ethos.

### Where they differ
- Fincept competes on **breadth and execution**; Finterm on **agent-ready
  structure**; Delta on **trust (citation contract) and restraint**.
- Finterm's two flagship features — SEC filing diffs and netted insider/13F
  flows — are built on free public data (EDGAR) that Delta already ingests.

---

## 2. What to adopt (ranked by fit and effort)

| # | Idea | Source | Fit | Why it fits Delta |
|---|---|---|---|---|
| 1 | SEC filing diffs | Finterm | High | Deterministic diff of public filings; writes into the evidence pool with citations. Finterm claims "no packaged equivalent" |
| 2 | Netted insider flows (Form 4, 90-day) | Finterm | High | Same free EDGAR data Delta already touches; pure computation |
| 3 | Computed sentiment composite (0–100) | Finterm | Medium | Same philosophy as `thesis_health.py`: computed, not claimed, honest about omitted components |
| 4 | RSS source labelling + dedupe | Finterm deep-research | Medium | Merge syndicated reprints, tag primary/secondary — cheap trust win for reports |
| 5 | Scheduled gathering | Fincept | Low | Pool stays fresh without pressing "Gather evidence" |
| 6 | MCP server / CLI JSON mode | both | Deferred | Expose the evidence pool to coding agents; likely scope creep for now |

### What NOT to copy
- Execution, algos, backtesting — explicit non-goals (PROJECT_SPEC.md §1).
- 41-module breadth — Delta's product is clarity, not coverage.
- Autonomous multi-agent research — conflicts with "discovery never auto-accepts".

---

## 3. Implementation plan

### Phase 1 — SEC filing diffs (`delta/filings.py`)

Current state: `plugins/data/sec_edgar.py` fetches submissions **metadata**
only (form, date, accession, primaryDocument URL via `FILING_URL`); it never
fetches filing documents.

1. **Shared EDGAR document fetcher.** Add document fetching to `sec_edgar.py`:
   fetch the primary document HTML with the plugin's `User-Agent` and existing
   `asyncio.Semaphore` (SEC fair-access: ≤10 req/s, 1 s slot hold). Strip tags
   to plain text with stdlib `html.parser` — no new dependency.
2. **Section splitter.** Regex on `Item N.` headings to extract Item 1A (Risk
   Factors) and Item 7 (MD&A). Unknown layouts degrade to whole-document diff.
3. **Diff.** `difflib` line-level diff between consecutive same-form filings
   (10-K vs prior 10-K, 10-Q vs prior 10-Q; never 10-K vs 10-Q). Output per
   section: added / removed / changed lines and churn = changed ÷ total.
4. **Store as Events.** One `Event` row per changed section, `kind=
   "filing_change"`, body = diff summary, url = filing URL. Flows into the
   evidence pool with zero changes to `EvidenceKind`.
   - Known cost (spec §9): `EventKind` is defined in three places
     (`core/models.py`, `extract.py`, `llm/prompts/extract_v1.j2`). Add it in
     all three, or consolidate them as part of this phase.
5. **Config.** `[plugins.sec_edgar] diff_forms = ["10-K", "10-Q"]` plus a
   per-run document cap; diffs run inside the normal **Gather evidence** pass.
6. **Tests.** Offline: `respx` mocks for EDGAR endpoints, fixture HTML for the
   splitter, snapshot tests for the diff. No network.

### Phase 2 — Netted insider flows (extend `sec_edgar`)

1. For Form 4 accessions, fetch the accession `index.json` → locate the
   ownership XML document.
2. Parse `nonderivativeTable`: filer name and role, transaction code, shares,
   price, acquired/disposed flag.
3. Open-market filter: transaction codes P (purchase) and S (sale); exclude
   grants, awards and option exercises.
4. Rolling 90-day net per instrument → one `Fundamental` row
   (`metric="insider_net_open_market_90d"`, plus optional gross buy/sell rows);
   per-transaction `Event` rows for the evidence pool.
5. Tests: `respx` + fixture Form 4 XML; net arithmetic cases (grants excluded,
   mixed buys/sells, 90-day window boundary).

### Phase 3 — Sentiment composite (`delta/sentiment_index.py`)

1. Pure function over bars, following the `thesis_health.py` pattern: no
   persistence, no LLM, fully offline-tested.
2. Components, each scored 0–100 against the ticker's own trailing year:
   price vs 125-day average, 52-week range position, up-day volume share (20d).
3. Honest coverage: components with no wired data source (short interest, IV
   rank) are listed as omitted with a reason; coverage count reported —
   never faked. Score is the mean over scored components only.
4. Surface via `brief.py` (facts-only brief), the Watchlist inspector, and as
   report context — labelled as a computed measure, not an opinion.

### Phase 4 — RSS labelling & dedupe; scheduled gathering

**4a. Labelling & dedupe (`plugins/data/rss.py` + small `delta/news_filter.py`).**
Current state: `rss.py` dedupes only identical `news_id(link, published)`
pairs (`items.setdefault`), so the same story syndicated at two URLs is stored
twice.
1. Canonical URL normalisation before id hashing: strip `utm_*` and tracking
   params, trailing slashes, fragments; lowercase host.
2. Title normalisation for similarity: casefold, strip punctuation and
   source suffixes ("— Reuters", "| Bloomberg"). Merge within a batch when
   normalised titles match or exceed a high similarity threshold
   (`difflib.SequenceMatcher`, stdlib); keep the earliest published copy and
   record the dropped sources in `raw["also_seen_on"]`.
3. Cross-batch merges: after ingest, one pass in `services.py` over the newest
   `NewsItemTable` rows applying the same normalisation, tagging duplicates in
   `raw` rather than deleting (evidence ids must stay stable for citations).
4. Primary/secondary tiering via a config allowlist
   (`[plugins.rss].primary_domains`); tier stored in `raw["tier"]` and shown
   in the Evidence screen so reports can prefer primary sources.
5. Tests: offline fixtures for URL/title normalisation, merge precedence,
   and allowlist tiering.

**4b. Scheduled gathering (`delta/scheduler.py` + headless entry point).**
1. Extract the **Gather evidence** pipeline (ingest + extract) from the TUI
   palette action into a `services.py` operation callable headlessly.
2. Add a headless entry point, `uv run delta gather`, running one pass and
   exiting — the TUI keeps its existing action on top of the same code.
3. Scheduling is the user's cron/launchd pointing at `delta gather` (documented
   in README), not a long-lived daemon — simplest thing that survives the TUI
   closing, and it inherits all rate limits and the citation contract.
4. Optional `[gather] schedule` documentation section in `config.toml`
   examples; no scheduler dependency added.

### Sequencing and guardrails
- Phases 1–2 share one code path (EDGAR document fetching) — build the
  fetcher once in Phase 1.
- Phases 3 and 4a are independent of 1–2 and can land in any order.
- Every phase is gather/compute only; the model still only synthesises what
  the harness collected, citing evidence ids.
- CI unchanged: `pytest` (offline), `ruff`, `mypy --strict` on core + llm.
