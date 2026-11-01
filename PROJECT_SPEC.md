# Delta — Project Specification

> **Audience:** AI coding agents and developers working on this repository. This
> is the source of truth for V2 scope, architecture, contracts and conventions.
> Read it before writing code.
>
> **Status:** V2 is specified, not built. The Rust workspace in `crates/`
> implements V1 (watch targets, theses, in-app reports and chat). §16 says what
> of it survives. Domain language is in `GLOSSARY.md`; decisions are in
> `docs/adr/`.
> **Owner:** Luke
> **Spec source:** the wayfinder map [Delta: harness-first research store
> organised by Space](https://github.com/lukebrown14-code/Delta/issues/56). Each
> section links the closed ticket that holds the full decision.
> **Last update:** 2026-11-01

---

## 1. Purpose

Delta is a personal store of AI-processed investment research, organised by
**Space**: one Space per investment idea. An external AI **harness** (Claude
Code or Codex) does the research and writes into Delta through a validated
interface. The user adds notes and records trades directly. Delta shows each
idea's prices, reports, notes, JEV readings and trades in one place.

It is for one hobby investor, not a professional desk. Prefer plain words over
industry terms, keep required configuration to a minimum, and put expert knobs
in config, out of the way.

**The product is clarity**: organised, cited research about each idea, and a
record of what the user did and how it turned out. JEV readings are
indicators, never advice.

Delta is market- and asset-agnostic. Data sources are added in `config.toml`
(§6).

### Out of scope

- Order execution, broker integration, automated trading.
- Holdings maths, broker-file imports, tax and dividend accounting,
  performance attribution. Trades carry no size.
- Training or fine-tuning models.
- Migrating V1 data. V2 starts with a fresh database ([ADR 0012](docs/adr/0012-fresh-schema-and-rust-native-tests.md)).
- Multi-user or hosted deployment.
- Text generation inside Delta. All open-ended AI work is the harness's
  ([ADR 0003](docs/adr/0003-harness-first-no-in-app-generation.md)).

---

## 2. Principles the code enforces

Each principle must be backed by a test.

1. **The harness researches; Delta validates.** The harness never writes
   SQLite. Every write goes through the tool registry (§7), is validated, is
   atomic (all or nothing) and is idempotent.
2. **Everything in a report is cited.** Every paragraph and bullet outside
   Unknowns cites stored evidence (`[ev:id]`) or a trade (`[tr:id]`). A report
   with any missing or invalid citation is rejected whole
   ([ADR 0007](docs/adr/0007-reject-whole-report.md)).
3. **Notes are the user's view.** They are never cited and never a JEV input.
   The harness may read them to steer research.
4. **Numbers come from data sources.** Prices, fundamentals and macro series
   arrive only through configured data sources. The harness submits text only
   ([ADR 0006](docs/adr/0006-harness-sources-are-evidence.md)).
5. **Readings are indicators.** A JEV reading is shown with its probabilities
   and the tag "Indicator, not advice". The words "Buy" and "Sell" never appear
   bare.
6. **The user's records are the user's.** Only the user writes notes (apart
   from the opening note), trades, premise edits, tripwires, horizon, risk
   appetite, archiving, renames and config
   ([ADR 0010](docs/adr/0010-trust-harness-with-attributed-writes.md)).
7. **Every write is attributed.** It records its author (the user, or the
   harness name and model) and time, and appears in the Space's activity log.
8. **Delta never acts on the user's behalf.** It flags (Exit likely, tripped
   tripwire, stale report) but never closes a trade, rewrites a report or
   changes a Space by itself.
9. **Config over code; secrets in `.env`.** Never commit `.env`, `data/*.db`
   or local output.

---

## 3. Architecture

```
user ──▶ harness (Claude Code / Codex) ──stdio MCP──▶ delta mcp ─┐
user ──▶ TUI (ratatui) ──────────────────────────────────────────┼──▶ tool registry ──▶ services ──▶ SQLite
user ──▶ delta tool <name> --json (CLI) ─────────────────────────┘                        ▲
                       data sources (built-in plugins, command scripts) ── ingest ────────┘
                       Jev decision model (OpenRouter) ◀── readings ── services
```

**Crates.** The V1 crates are reused where they fit (§16):

| Crate | Owns |
|---|---|
| `delta-core` | config, IDs, domain models, SQLite, migrations, events |
| `delta-plugins` | built-in data sources, the command source runner, the source registry, the Jev client |
| `delta-services` | the tool registry and every operation behind it: Spaces, evidence, reports, citation checks, readings, trades, alerts, refresh, jobs |
| `delta-tui` | the TUI, `delta mcp`, the CLI and setup commands |

`delta-llm` is removed ([ADR 0003](docs/adr/0003-harness-first-no-in-app-generation.md)).

**Boundaries.**
- Screens and MCP handlers call the tool registry. Providers and storage stay
  behind services.
- Network work and Jev calls run on background workers or jobs, never on the
  render path.
- SQLite runs in WAL mode with busy timeouts. Several `delta mcp` processes and
  the TUI may be open at once.

---

## 4. Domain model

Terms are defined in `GLOSSARY.md`. This section gives fields and rules.

### Space
Decided in [Space model](https://github.com/lukebrown14-code/Delta/issues/60)
and [Space interview script](https://github.com/lukebrown14-code/Delta/issues/69).

- **Identity:**
  - a display name and a unique slug, used by the user and the harness to
    address the Space;
  - both can be renamed, by the user only;
  - a stable internal ID underneath keeps references intact.
- **Premise:** one editable sentence (a question or a hypothesis). Every edit is
  kept with its date.
- **Why now:** from the interview, stored in the opening note.
- **Horizon:** under 6 months, 6–18 months, 1–3 years, 3+ years, or blank.
  User-owned, edit history kept.
- **Risk appetite:** low, medium (the default) or high.
  - Shapes the candidates the harness proposes and Scan criteria.
  - Adds a track-record breakdown.
  - Never a JEV input.
  - User-owned, edit history kept.
- **Tripwires:** up to three conditions that would prove the premise wrong.
  User-owned, edit history kept.
- **Benchmark:** at most one instrument. Used for the regime reading and for
  trade comparisons.
- **Related Spaces:** links between Spaces that overlap. Spaces are flat and
  never nest.
- **Lifecycle:** two states, **open** and **archived**.
  - "Invested" is shown when the Space has an open trade; it is not a state.
  - Archiving requires a conclusion note and no open trades.
  - An archived Space is read-only except for notes, stops refreshing, and can
    be reopened.
- **Activity log:** every write, with its author and time.

### Member
- A global instrument plus a one-line reason. Instruments and their prices
  are shared across Spaces.
- Only tradable instruments can be members. Unlisted companies appear in
  reports and notes by name only.
- A member can't be removed while it has an open trade. After removal, its
  notes and closed trades stay in the Space, marked as belonging to a former
  member.
- A member that no source covers for prices is still added, flagged "no price
  source".

### Instrument and market
- **Instrument identity:** `MARKET:SYMBOL`, for example `US:IONQ`.
- **Asset classes:** a fixed list: equity, ETF, bond, FX, commodity, crypto,
  cash, other.
- **Markets:** defined in config (namespace, currency, timezone, session).
- **Symbol overrides:** per-source symbol overrides (`BRK.B` → `BRK-B`) are
  stored on the instrument.

### Evidence
- **Storage:** evidence is stored once, globally, and addressed by a stable id.
  Kinds: price bars, news, filings, fundamentals, calendar events, macro values
  and harness sources.
- **Automatic visibility:** a Space automatically sees all evidence about its
  members.
- **Linked evidence:** evidence not about one member is linked explicitly, by
  the harness or the user. Only links can be removed.
- **No acceptance gate** ([ADR 0005](docs/adr/0005-no-evidence-acceptance-gate.md)).
- **Removed evidence:** a report that cites removed evidence still resolves, but
  is flagged until it is rewritten.
- **Harness sources:** URL, title, extract, optional published date, and the
  retrieval date. A duplicate URL with the same content returns the existing
  id.

### Macro series
- Declared once in config under a canonical key, for example
  `[macro.us10y] source = "fred", id = "DGS10"`, and fetched globally.
- A Space links the series it uses.
- Each market lists the macro series its regime reading uses.

### Note
Decided in [Notes: linking and search](https://github.com/lukebrown14-code/Delta/issues/72).
- **Pin:** exactly one, either the Space or one member.
- **Mentions:** any number of `@` mentions of instruments, Spaces, reports or
  trades, in any Space. Stored by id and rendered by name.
- **Backlinks:** a mentioned item lists "Mentioned in notes".
- **Content:** multi-line markdown.
- **Editing:** edits keep earlier versions. Deletion is allowed.
- **Opening note:** holds the interview answers. It is the only note the
  harness writes.

### Report, JEV reading, trade, alert
See §9, §10, §11 and §12.

---

## 5. Storage

Decided in [Audit](https://github.com/lukebrown14-code/Delta/issues/59).

- **Database:** a fresh SQLite database with versioned migrations. Nothing reads
  V1 databases.
- **Kept from V1, adapted:** `bar`, `newsitem`, `news_instrument`, `event`
  (calendar rows only), `fundamental`, `llmcall` (the Jev cache and cost log),
  `sentiment` (per-item news stance).
- **New tables:**
  - instrument, with symbol overrides;
  - Space, with premise, horizon, risk appetite and tripwire histories;
  - member, related-Space link and linked evidence;
  - harness source and macro value;
  - note, note version and mention;
  - report and report citation;
  - JEV reading;
  - trade, with trade edit and trade correction;
  - alert;
  - activity;
  - job.
- **Full-text search:** SQLite FTS5 indexes evidence text and notes.
- **Dropped:** `thesis`, `thesis_evidence`, `decision`, `decision_review`.

---

## 6. Data sources

Decided in [Data sources in config](https://github.com/lukebrown14-code/Delta/issues/63).

- **Definition:** a **data source** is a named `config.toml` entry, backed by a
  built-in plugin or a **command**
  ([ADR 0008](docs/adr/0008-command-data-sources.md)).
- **Kinds:** prices (bars and quotes), fundamentals, news, filings, calendar
  events, macro.
- **Built-in plugins:** Yahoo (prices, quotes, calendar, search), SEC EDGAR
  (filings, fundamentals), ASX announcements (filings), RSS (news), FRED
  (macro, free API key).
- **Coverage:** each source declares the markets and asset classes it covers.
  An ordered list per kind picks the source for an instrument, and the next one
  is the fallback on failure.
- **Symbols:** each source holds its own per-market mapping rule (for example
  Yahoo's `.AX` suffix). Per-instrument overrides are stored in the database.
- **Command contract:**
  - **Input:** Delta passes JSON on stdin: the kind, the instruments with this
    source's symbols, and `since`.
  - **Output:** one JSON row per line on stdout, in Delta's row schema for that
    kind. Every row is validated.
  - **Failure:** a non-zero exit or the timeout (default 120 s) is a failed
    fetch. stderr goes to the log.
  - **Config:** the entry declares `provides` and coverage.
- **Fundamentals:** a canonical metric list (revenue, net income, EPS, free
  cash flow, debt, shares outstanding, and so on), with a mapping per source.
  Metrics outside the list are stored prefixed with the source name.
- **Trust:**
  - Only the user adds or enables sources.
  - The harness may write a command script and propose its config, but can't
    apply either.
  - Secrets stay in `.env`; the source names the environment variable it reads.

---

## 7. Harness interface

Decided in [Harness link](https://github.com/lukebrown14-code/Delta/issues/57),
[MCP tool surface](https://github.com/lukebrown14-code/Delta/issues/66) and
[Harness packaging](https://github.com/lukebrown14-code/Delta/issues/70).
[ADR 0004](docs/adr/0004-stdio-mcp-and-shared-tool-registry.md) records the
shape.

### Transport and registration
- **Transport:** stdio MCP on `rmcp`, started as `delta mcp`. stdout carries
  MCP messages only; logs go to stderr.
- **Registration:** `delta setup harness` registers Delta at user scope in
  Claude Code and Codex.
  - It writes an absolute binary path and `--home`.
  - It sets Codex `tool_timeout_sec = 120`.
  - `--check` starts `delta mcp`, lists its tools and verifies each
    registration.
  - No project registration files are committed.
- **Home folder:** resolved from `--home`, then `DELTA_HOME`, then the current
  folder.

### Tools
Tools return ids, summaries and metadata, paginated under about 25k tokens.
Full text is served as resources.

**Read tools**

| Tool | Returns |
|---|---|
| `list_spaces` | open and archived Spaces |
| `get_space` | premise and its history, horizon, risk appetite, tripwires and their state, members with flags, benchmark, related Spaces, latest report per type and subject with stale flags, latest readings, open trades, notes, recent activity |
| `search_instruments` | symbol lookup across sources |
| `get_brief` | the facts-only brief for an instrument |
| `search_evidence` | evidence by Space, instrument, kind, date or keyword (FTS5) |
| `search_notes` | notes by keyword, Space, pin or date, labelled as the user's view |
| `list_reports` | latest versions, optionally with history |
| `get_readings` | a subject's readings and their history |
| `list_trades`, `get_track_record` | trades, outcomes and the track record |
| `list_macro_series` | the configured macro series |
| `data_health` | source status and failures |

**Write tools**

| Tool | Checks |
|---|---|
| `create_space` | Slug is unique, the premise is one sentence, members and benchmark are valid. Takes horizon, risk appetite, tripwires and the opening note. Starts the first ingest and returns its job id. |
| `add_member` | `MARKET:SYMBOL` in a configured market, resolvable by a source; otherwise the error suggests close matches. A reason is required. |
| `remove_member` | Fails while the member has an open trade. |
| `set_benchmark`, `link_related_space` | Valid instruments and Spaces. |
| `submit_source` | URL, extract and retrieval date. Text only. Deduplicated. Optionally linked to a Space. |
| `link_evidence`, `unlink_evidence` | Only linked evidence can be unlinked. |
| `link_macro_series` | A configured key. |
| `submit_report` | §9 contract. Records harness, model and request text. Returns the failing paragraphs on rejection. |
| `withdraw_report` | A reason is required. The version stays in history. |
| `set_symbol_override` | A configured source and an existing instrument. |

All writes are rejected on archived Spaces.

**Jobs:** `start_ingest` (a Space or a list of instruments, optionally some
kinds only), `run_reading` (a subject and kind) and `job_status`. Jobs are
persisted, idempotent while running, and resumable after a restart. Calls send
progress notifications while open.

**Resources:** `delta://evidence/<id>`, `delta://report/<id>`,
`delta://trade/<id>` and `delta://guide/<name>`.

### Guides and instructions
- **Guides:** compiled into the binary
  ([ADR 0011](docs/adr/0011-guides-ship-in-the-binary.md)) and served as
  resources and as MCP prompts. Three guides:
  - `new_space`, the interview (§8);
  - one per report type;
  - refreshing a Space.
- **Server instructions:** the rules come first, so the first 512 characters
  stand alone:
  1. Cite Delta ids in every report paragraph, and submit sources before citing
     them.
  2. Ask before creating a Space or adding a member.
  3. Never write the user's records.
  4. Never cite notes.
  5. JEV readings are indicators, not advice.
  6. Long fetches are jobs: poll them.

  Pointers to the guides follow. The whole text stays under 4,096 characters.
- **Unverified:** whether Codex supports MCP prompts. The resources cover it
  either way.

### CLI
- `delta tool <name> --json '{…}'` calls the same registry.
- Human-friendly commands: `delta gather`, `delta mcp`,
  `delta setup harness [--check]` and `delta setup schedule [--check]`.

---

## 8. Space creation

Decided in [Space creation interview](https://github.com/lukebrown14-code/Delta/issues/61)
and [Space interview script](https://github.com/lukebrown14-code/Delta/issues/69).

- **From the TUI:** the TUI creates a bare Space (name, slug, premise) through
  the same `create_space` service.
- **From the harness:** the harness runs the `new_space` interview, one
  question at a time, five at most. Answered questions are skipped, and "skip"
  accepts the default:
  1. **Premise:** the harness restates it as one sentence.
  2. **Why now.**
  3. **Known candidates.** The harness then proposes up to five candidates with
     reasons, shaped by risk appetite and the `[profile]` markets, plus a
     benchmark inferred from the members' market.
  4. **Horizon and risk appetite**, asked together.
  5. **Up to three tripwires.**
- **Summary card:** name, slug, premise, why now, members with reasons,
  benchmark, horizon, risk appetite and tripwires. **Nothing is written until
  the user confirms.** During creation the harness never adds a member without
  the user's yes.
- **First investigation**, which follows immediately:
  1. **Ingest job:** everything configured for the members and the benchmark,
     with price history defaulting to 2 years of daily bars. Instruments
     already held are reused.
  2. **Web research:** the harness researches and submits harness sources,
     linked to the Space.
  3. **Landscape report:** the harness writes it (§9).
- **Failure:** a member whose ingest fails stays, the failure shows in Settings
  and Sources, and the harness reports it at the end instead of stopping.

---

## 9. Reports

Decided in [Report types](https://github.com/lukebrown14-code/Delta/issues/62).

| Type | Subject | Required sections |
|---|---|---|
| Landscape report | the premise | Summary, Landscape (size, structure), Drivers, Trends (each cites dated evidence), Players (members plus unlisted companies by name), Bull, Bear, Catalysts, Risks, Unknowns |
| Deep dive | one current member | Summary, Business, Financials, Valuation context, Bull, Bear, Catalysts, Risks, Unknowns |
| Scan | the scan's title | Summary, Criteria, Candidates (proposals only), Excluded, Unknowns |
| Answer | the user's question, verbatim | Answer, Support, Unknowns. Bull and Bear are optional. |

- **Format:** markdown with inline `[ev:<id>]` / `[tr:<id>]` markers. Delta
  checks the required sections and that every paragraph and bullet outside
  Unknowns carries at least one valid citation. Inferences cite what they're
  drawn from. There is no sentiment score.
- **Versions:** an accepted report is immutable. A rewrite is a new version of
  the same type and subject, and history is kept. The Space shows the latest
  version per type and subject.
- **Withdrawing:** the user (or the harness) can withdraw a version, which
  hides it and keeps it in history. There is no hard delete.
- **Provenance:** each version records the harness and model that wrote it,
  when, and the request text that prompted it.
- **Staleness:** flagged deterministically and never auto-rewritten. A report is
  stale when any of these hold:
  - it is past its type's shelf life (config: Landscape 90 days, Deep dive 30,
    Scan 60, Answer never);
  - it cites removed evidence;
  - a filing or earnings event has landed since it was written for an
    instrument it covers. It covers its subject plus the instruments behind its
    cited evidence.

---

## 10. JEV readings

Decided in [Jev Decisions API](https://github.com/lukebrown14-code/Delta/issues/58)
and [JEV reading design](https://github.com/lukebrown14-code/Delta/issues/64).
Tripwire questions are from [Space interview script](https://github.com/lukebrown14-code/Delta/issues/69).

- **What Jev is:** it answers typed questions (Score, yes/no, Choice) about a
  state Delta sends and returns probabilities only. It costs about US$0.0003 per
  reading. Delta computes numbers and dates and passes them as named buckets.
- **Subjects:**
  - market regime per benchmark, shared across Spaces;
  - outlook health and lean per member within its Space;
  - a Space health roll-up, equal-weighted, naming the weakest member.
- **Shape:** each reading is a versioned set of narrow questions, combined in
  code into a headline, with the component answers kept.
  - **Lean:** the Choice runs twice, the second time with the options reversed;
    if the two runs disagree, the lean is Unclear.
  - **Health and lean:** one yes/no question per evidence item gives attribution
    (which items supported or opposed the reading).
  - **Tripwires:** health adds one yes/no question per tripwire. ≥ 0.7 means
    tripped.

| Reading | Inputs (all bucketed) |
|---|---|
| Regime | benchmark trend against its 50- and 200-day averages, 3-month return, drawdown, volatility against its 1-year norm, news tone, the market's macro series |
| Health | premise and tripwires, the member's price and fundamentals trend buckets, news tone (30 days), evidence from the last 90 days, the Summary, Bull, Bear and Risks sections of the latest non-stale Landscape report and Deep dive |
| Lean | the health inputs, valuation buckets, the current regime headline |

- **Never inputs:** notes, trades and risk appetite. The state is capped at
  about 32 KB; the oldest and lowest-quality items are dropped first, and the
  count is recorded.
- **Timing:**
  - automatically after new evidence or an accepted report, at most once per
    subject per day;
  - on demand;
  - always at trade entry and exit.

  No call is made if the inputs hash is unchanged.
- **Storage:** readings are immutable rows: headline, probabilities,
  confidence, components, the evidence ids in the state, the model snapshot,
  the question-set version, the inputs hash and the time.
- **Display:**
  - lean values: Leans buy, Leans hold, Leans sell, Unclear;
  - the tag "Indicator, not advice";
  - what changed since the last reading, and a sparkline;
  - readings from different versions are marked as not comparable.
- **Config:** the model is pinned to `typesafe/jev-1.14`. Thresholds: yes ≥ 0.7,
  no ≤ 0.3, lean shown at confidence ≥ 0.5. They are retuned from the track
  record.
- **Client work:**
  - Noul and Score questions, with typed answers;
  - keep option order (sent as written, not alphabetically);
  - retry with backoff on 429, 5xx, 529 and in-flight-budget 402;
  - bounded concurrency;
  - a 32 KB state builder;
  - treat a missing `usage.cost` as unknown, not free.

---

## 11. Trades and track record

Decided in [Trade tracker](https://github.com/lukebrown14-code/Delta/issues/65).
The risk appetite breakdown is from [Space interview script](https://github.com/lukebrown14-code/Delta/issues/69).

- **What a trade is:** long only, mirroring a position on the user's own
  trading platform. Delta never places orders. There is no size.
- **Entry:**
  - Space and member;
  - entry date and price, prefilled from the latest quote and editable;
  - target price;
  - **accepted loss** as a stop price;
  - a required one-line rationale;
  - a JEV snapshot.
- **While open:** the target and stop can be moved, with dated history. R is
  measured against the initial stop.
- **Exit likely:** when stored bars cross the stop or target, the trade is
  flagged and offers a prefilled exit (that date and level) for the user to
  correct to the actual fill.
- **Closing:**
  - the user enters exit date, price and reason (target hit, stop hit, premise
    changed, time, other);
  - there are no partial exits;
  - an optional lesson is saved as a note pinned to the member;
  - a JEV snapshot is taken at exit.
- **Outcome measures:** return %, R multiple, days held, the worst and best
  price during the hold, and return against the benchmark over the same dates.
- **Track record:**
  - win rate, average win and loss in R, and expectancy;
  - broken down by lean at entry, regime at entry, Space and risk appetite;
  - a bucket with fewer than 5 closed trades shows "too few trades".
- **Mistakes:** corrections are logged. A trade can be voided (kept, but
  excluded from the track record). There is no hard delete.
- **Harness access:** the harness reads trades and cites them as `[tr:id]`
  ([ADR 0009](docs/adr/0009-trades-are-citable.md)). Only the user records
  trades.

---

## 12. Refresh and alerts

Decided in [Background refresh and alert delivery](https://github.com/lukebrown14-code/Delta/issues/71).

- **Refresh:**
  - When the TUI opens it shows cached data instantly and refreshes in its
    background workers. While it stays open, it keeps refreshing on the cadence
    below.
  - An optional launchd schedule runs `delta gather`. It is off by default and
    turned on with `delta setup schedule`.
  - There is no daemon.
  - Live quotes stream only while the TUI is open.
- **Cadence** (config):

  | When | What |
  |---|---|
  | After each market's close, from its session times | bars, calendar, fundamentals |
  | Every 2 hours, 07:00–22:00 | news and filings |
  | Daily | macro series |
  | Weekends | news only |
  | After new evidence | readings, within the daily cap |

- **Scope:** open Spaces' members and benchmarks, their linked macro series,
  and each market's regime series.
- **Alerts:**
  - **Triggers:**
    - Exit likely;
    - a tripped tripwire;
    - lean or regime change;
    - stale report;
    - results due within 7 days on a member with an open trade;
    - source failure.
  - **Where they show:** only in Home's **Needs attention** when Delta opens.
    There are no OS notifications.
  - **Lifecycle:** each fires once per occurrence and clears when acted on or
    dismissed (`x`). It stays in the Timeline.
- **Stale refresh:** if no refresh has succeeded for more than 24 hours, Home
  shows "last refresh 26h ago, refreshing…".

---

## 13. TUI

Decided in [Screen prototype](https://github.com/lukebrown14-code/Delta/issues/67)
(reference implementation on branch `prototype/screens-67`) and
[Notes: linking and search](https://github.com/lukebrown14-code/Delta/issues/72).
This is a first pass, to be refined.

### Screens
- **Home:**
  - a status line with changes since the last visit, when the harness last
    wrote, and refresh state;
  - the **Needs attention** strip;
  - a **Spaces table**: name, premise, members, regime, health with its weakest
    member, lean mix, open trades, latest report and flags. Archived Spaces are
    dimmed.
- **Space**, three views cycled with `v`. Opening a Space lands on Overview,
  unless it was last viewed in another view this session. Selection carries
  across views.
  - **Overview:**
    - a header: premise, benchmark, regime, health roll-up, horizon, risk
      appetite, tripped tripwires, and the OPEN/INVESTED chips;
    - a members table: last, 1M, 3M sparkline, health, lean, **Next** event,
      flags;
    - a readings panel with attribution;
    - Reports, Notes and Trades panels.
  - **Focus:** a list of the Space plus its members.
    - **Space selected:** premise history, the latest report rendered with
      citation markers, the report list, and notes on the Space.
    - **Member selected:** reason, a chart with entry, stop and target, readings
      with attribution, Deep dive state, trades, pinned notes and "Mentioned in
      notes".
  - **Timeline:**
    - an **Upcoming** block for the next 60 days: calendar events (◷), dated
      report catalysts marked "per report" (◇), and macro release dates when a
      source provides them;
    - a dated feed of everything that happened in the Space;
    - a Now column.
- **Trades** (record first):
  - headline tiles: closed count, win rate, expectancy, average win and loss;
  - breakdowns by lean, regime, Space and risk appetite;
  - all trades, open first, with Exit likely and VOID markers.
- **Settings and Sources:** plugins and sources, markets, macro series, source
  health and failures, harness registration and schedule status. There are no
  LLM provider or model settings.

### Overlays and keys
- `g` go to a Space.
- `n` new note: pin picker, `@` mention picker, `Ctrl-E` opens `$EDITOR`.
- `c` record exit: a prefilled form with a live outcome preview.
- `/` note search.
- `x` dismiss an alert.
- `?` help. `q` quit.

---

## 14. GUI readiness

The tool registry is the contract any future GUI uses, exactly as MCP and the
CLI do. The registry guarantees:
- every operation the TUI can perform is a registry call with typed input and
  output. No screen talks to SQLite or a provider directly;
- validation, attribution, atomicity and idempotency live in services, not in a
  surface;
- long work is a persisted job with polled status, so any client can show
  progress;
- read models (Space, readings, track record, alerts) are computed in services,
  so a GUI shows the same numbers as the TUI.

Adding a GUI means a new client of the registry, never a second API. Add an
HTTP transport only if a GUI needs a shared long-lived server.

---

## 15. Configuration

`config.toml` sections. Secrets live in `.env`.

| Section | Holds |
|---|---|
| `[paths]` | database path |
| `[profile]` | usual markets, base currency |
| `[markets.<key>]` | label, currency, timezone, session times, regime macro series |
| `[sources.<name>]` | kind or plugin, `provides`, coverage, symbol rules, command and timeout, env var names |
| `[sources.order]` | ordered source list per kind |
| `[macro.<key>]` | source, series id, label |
| `[ingest]` | default price-history depth (2 years) |
| `[refresh]` | cadence |
| `[reports]` | shelf lives |
| `[jev]` | model, thresholds, state budget |

Built-in plugin keys are `plugin.name()` strings.

---

## 16. From V1 to V2

Decided in [Audit](https://github.com/lukebrown14-code/Delta/issues/59).

- **Keep:** the data plugins (Yahoo, SEC, ASX, RSS, calendar, http), the TUI
  widget kit (axes, braille, chart, components, dialog, form, input, markdown,
  table, theme, wrap, status bar), the quote and ingest workers, and the core
  helpers.
- **Adapt:**
  - config: drop targets and `llm_*`;
  - db: drop V1 compatibility, add migrations;
  - models, evidence, ingest, brief and analytics;
  - reports: storage and render only;
  - Jev client: extend it and move it to `delta-plugins`;
  - citation validation: move it to services;
  - plugin registry and scope: coverage by market and asset class;
  - setup and config operations;
  - instrument search;
  - the app shell, Home and Settings;
  - the research and watchlist panes, harvested into the Space screen;
  - the CLI.
- **Drop:**
  - `delta-llm` apart from Jev;
  - chat, report generation, LLM event extraction, schemas;
  - targets, theses, thesis health and summary, decisions;
  - the Ask, Theses, Decisions and live screens;
  - the parked Dossier grammar;
  - the Python golden oracle, fixtures and parity tests
    ([ADR 0012](docs/adr/0012-fresh-schema-and-rust-native-tests.md)).
- **Archive:** `docs/rewrite/`.
- **Tests:** TUI screens are tested with Rust-native snapshots on the cell-grid
  screen model.

---

## 17. Coding standards

- Rust toolchain pinned in `rust-toolchain.toml`. Run `cargo fmt --check` and
  `cargo clippy --all-targets -- -D warnings`.
- Async for I/O (fetching, Jev calls); sync for glue.
- **No network in tests.** Use fixed fixtures, temporary databases, and mocked
  HTTP and Jev responses.
- Conventional commits (`feat:`, `fix:`, `docs:`, `chore:`).
- Never commit `.env`, `data/*.db` or local output.
- CI runs format, Clippy and `cargo test --workspace` on every push.

---

## 18. Caveats and open items

- **Data quality:** free data sources are rate-limited and sometimes wrong. A
  single citation is a lead, not a fact.
- **Reports:** a cited report is only as good as what was gathered. The
  citation contract is the defence against invented facts; loosening it is
  never the fix for a rejected report.
- **Readings:** Jev's calibration is claimed across groups of predictions and
  is unproven on market outcomes. The track record is the only measure of
  whether leans mean anything.
- **Unverified:** whether Codex supports MCP prompts, and whether MCP progress
  extends Codex's tool timeout. Latency of 8k–32 KB Jev states via OpenRouter
  is unmeasured.
- **Known gap:** market session times have no holiday calendar.
