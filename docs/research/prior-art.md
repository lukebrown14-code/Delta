# Prior art for Delta V2

> Resolves issue #45 (map #43). Researched 2026-10-18 against primary sources:
> project repos (read via `gh api`), official docs and the MCP specification.
> Links point at default branches unless a tag or commit is given; the claims
> are true as of the research date.
> Short forms: OBB = `https://github.com/OpenBB-finance/OpenBB/blob/develop/openbb_platform/`,
> GF = `https://github.com/ghostfolio/ghostfolio/blob/main/`,
> PP = `https://github.com/portfolio-performance/portfolio/blob/master/name.abuchen.portfolio/src/name/abuchen/portfolio/`.

## Answer

**Borrow (the top seven):**

1. **A fetcher split into fetch, record, transform** (from OpenBB's TET pattern). Delta adds one step: the raw payload and its URL are stored as an evidence record *before* transform runs.
2. **Capability declarations.** A provider declares which standard models (and, for Delta, which markets and asset classes) it serves. Config sets the fallback order for each command. Each provider gets only its own credentials.
3. **A central instrument registry.** A canonical id plus a per-source symbol map, with ISO 10383 MIC as the exchange key and ISIN/FIGI/CUSIP as optional identifiers (from Ghostfolio's `symbolMapping`, applied consistently). OpenBB has no such layer, and its per-fetcher string munging is the cautionary example.
4. **User-defined weighted taxonomies** for classification (Portfolio Performance), with a small fixed `AssetClass` kept only for behaviour. A separate user-overrides table keeps provider data and user corrections apart (Ghostfolio).
5. **Import with a preview step:** named CSV column-mapping templates, header aliases, a dry run and an explicit duplicate key. Every imported row records its source (Ghostfolio and Portfolio Performance). Money is fixed-point.
6. **One action registry** behind the command line, the palette, the headless CLI, routines and MCP tools (Fincept's `CommandParser`, k9s `:` mode, OpenBB v3 routines). Grammar: `<instrument> [<class>] <FUNC> [args]`.
7. **MCP as resources plus a few read-only tools.** Expose `delta://evidence/{id}` resource templates. Search tools return `resource_link`s and an `outputSchema` that requires `evidence_id`, `source_url` and `retrieved_at`. Every result carries a source URL, as the SEC EDGAR MCP server does.

**Avoid (the top six):**

1. **Thin pass-through tools** (Massive `call_api`, Alpha Vantage, yfinance, FMP MCP servers). They let an agent bring in facts with no evidence id.
2. **Results without stable ids.** OpenBB's `OBBject.id` is per call and never stored, its rows have no ids, and its arguments are truncated to 80 characters.
3. **Fixed enums for data sources or asset classes.** Ghostfolio needs a Prisma migration for every new provider. Portfolio Performance has deprecated its own `type` field.
4. **Breadth over depth.** Fincept has 54 screens and about 1,400 Python subprocess scripts. OpenBB V5 dropped ten or more providers it could not maintain.
5. **Accounting depth:** separate cash and securities ledgers, delivery, tax and refund types (Portfolio Performance).
6. **LLM-driven import as the source of truth** (Ghostfolio's vision import). At most, AI proposes rows and the user accepts them.

**Licences.** OpenBB is Apache-2.0 from V5 (2026-09-29). Its v4.x releases, from May 2024 to March 2026, were AGPL-3.0. Ghostfolio, Fincept and the SEC EDGAR MCP server are AGPL-3.0. Portfolio Performance is EPL-1.0. Take ideas only from all of these except OpenBB V5+, tickrs (MIT), cointop (Apache-2.0), longbridge-terminal (Apache-2.0) and `rmcp` (Apache-2.0).

## Licence table

| Project | Licence | Borrow code? | Source |
|---|---|---|---|
| OpenBB Platform ≥ V5 | Apache-2.0 (repo now resolves to `openbq-org/OpenBB`) | Yes, keeping the notice and marking changes | [LICENSE](https://github.com/openbq-org/OpenBB/blob/main/LICENSE), release `ODP` 2026-09-30 |
| OpenBB v4.x (May 2024 to Mar 2026) | AGPL-3.0 | No, ideas only | commit `c61d10d6e3` (#6415) |
| OpenBB Terminal v3.x | MIT | Yes | tag `v3.2.5` |
| Ghostfolio | AGPL-3.0 | No | `gh api repos/ghostfolio/ghostfolio/license` |
| Portfolio Performance | EPL-1.0 | No (weak copyleft per file) | `gh api repos/portfolio-performance/portfolio/license` |
| Fincept Terminal | AGPL-3.0-or-later, proprietary Enterprise edition | No | [README](https://github.com/Fincept-Corporation/FinceptTerminal/blob/main/README.md) |
| tickrs | MIT | Yes | [repo](https://github.com/tarkah/tickrs) |
| ticker | GPL-3.0 | No | [repo](https://github.com/achannarasappa/ticker) |
| cointop (archived 2024-04) | Apache-2.0 | Yes | [repo](https://github.com/cointop-sh/cointop) |
| longbridge-terminal | Apache-2.0 | Yes | [repo](https://github.com/longbridge/longbridge-terminal) |
| sec-edgar-mcp | AGPL-3.0 | No | [repo](https://github.com/stefanoamorelli/sec-edgar-mcp) |
| `rmcp` (official Rust MCP SDK) | Apache-2.0 crate | Dependency (needs approval under AGENTS.md) | [rust-sdk](https://github.com/modelcontextprotocol/rust-sdk) |

## OpenBB Platform

**Facts**

- **The TET pattern.** `Fetcher[Q, R]` has three stages: `transform_query`, then `extract_data`/`aextract_data(query, credentials)`, then `transform_data(query, data)`. `transform_data` can return `AnnotatedResult{result, metadata}`. V5 adds `stream_data`, and `Fetcher.test()` exercises every stage. Source: OBB `core/openbb_core/provider/abstract/fetcher.py`, `annotated_result.py`.
- **Provider.** `Provider(name, website, credentials, fetcher_dict, ...)`, where `fetcher_dict` maps a standard-model name (e.g. `"EquityHistorical"`) to a fetcher. Providers register through Python entry points. Source: OBB `core/openbb_core/provider/abstract/provider.py`, `providers/tmx/pyproject.toml`.
- **Commands.**
  - `@router.command(model="EquityHistorical")` turns into one function. Every provider that serves the model is listed in a `provider` Literal.
  - Provider-only parameters are merged in, labelled with the providers that support them. Unsupported parameters only produce a warning.
  - The fallback order per command lives in `defaults.commands[...].provider`.
  - Source: OBB `core/openbb_core/app/provider_interface.py`, `app/query.py`, `app/model/defaults.py`.
- **Credentials.** Credential names carry the provider name as a prefix and are held as `SecretStr`. `QueryExecutor.filter_credentials` gives each fetcher only its own keys. Source: OBB `core/openbb_core/app/model/credentials.py`, `provider/query_executor.py`.
- **OBBject (the result wrapper).** Fields: `id` (UUIDv7, per call), `results`, `provider`, `warnings`, `chart`, `extra`. Metadata records the route, arguments (truncated at 80 characters), duration and timestamp. There is no source URL, no payload hash and no per-row id. Source: OBB `core/openbb_core/app/model/obbject.py`, `metadata.py`.
- **Standard models.** About 180 pairs of `XQueryParams` and `XData`. Providers subclass them and use `__alias_dict__` to map field names. `Data` is `extra="allow"`, so unknown fields pass through. Asset classes are **separate routers with duplicated models** (`EquityHistorical`, `EtfHistorical`, `CryptoHistorical`, `CurrencyHistorical`, `IndexHistorical`, ...). Source: OBB `core/openbb_core/provider/standard_models/`, `extensions/`.
- **No central symbology.**
  - yfinance FX appends `=X`, and yfinance crypto rewrites `BTCUSD` to `BTC-USD` ([v4.7.0 `currency_historical.py`](https://github.com/OpenBB-finance/OpenBB/blob/v4.7.0/openbb_platform/providers/yfinance/openbb_yfinance/models/currency_historical.py)).
  - TMX strips `.TO` (OBB `providers/tmx/openbb_tmx/utils/helpers.py`).
  - Core ships an ISO 10383 MIC table that no provider uses (OBB `core/openbb_core/provider/utils/exchange_utils.py`).
- **MCP server.**
  - One tool per route (`equity_price_historical`), grouped into categories by the first path segment.
  - An optional discovery mode adds `available_categories`, `search_tools` and `call_tool`. There is also a `run_pipeline` tool.
  - Streamable HTTP by default.
  - Source: OBB `extensions/mcp_server/README.md`.
- **Workspace citations.** Workspace cites a *widget plus its input arguments*. For PDFs it can add bounding boxes. Source: [docs.openbb.co citations](https://docs.openbb.co/workspace/developers/ai-features/citations-for-documents).
- **V5 removed providers.** yfinance, fmp, intrinio, tiingo, alpha-vantage, econdb and others are no longer bundled. The bundled free core is now official and exchange sources: SEC, FRED, BLS, ECB, IMF, OECD, CFTC, EIA, CBOE, TMX and others. Source: V5 commit `bbf1ab2020`, OBB `providers/`.

**Borrow**

- **The TET stages as a Rust trait**, with an `extract` → record-evidence → `transform` order.
- **A strict standard model plus a typed per-provider `extras` map.**
- **Capability declarations and config-driven fallback order.**
- **Credential scoping.**
- **MIC as the exchange key.**
- **Tool names derived from the command path.**

**Avoid**

- Per-fetcher symbol munging.
- Duplicating models per asset class.
- Ids that are per call and never stored.
- `extra="allow"` pass-through.
- Python entry-point and metaclass plumbing. Delta's static registry keyed by `plugin.name()` already fits Rust.
- Citations that point at a query rather than a stored fact.

## Ghostfolio

**Facts**

- **Identity.** `SymbolProfile` is unique on `[dataSource, symbol]` and has optional `isin`, `cusip` and `figi*` fields, plus `symbolMapping` (Json), `scraperConfiguration`, `countries`, `sectors` and `holdings`. Source: GF `prisma/schema.prisma` L231-272.
- **Enums.** Source: GF `prisma/schema.prisma` L347-385.
  - `AssetClass`: ALTERNATIVE_INVESTMENT, COMMODITY, EQUITY, FIXED_INCOME, LIQUIDITY, REAL_ESTATE.
  - `AssetSubClass`: BOND, CASH, COLLECTIBLE, COMMODITY, CRYPTOCURRENCY, ETF, LOAN, MUTUALFUND, PRECIOUS_METAL, PRIVATE_EQUITY, STOCK.
  - `DataSource` is a database enum: ALPHA_VANTAGE, COINGECKO, EOD_HISTORICAL_DATA, FINANCIAL_MODELING_PREP, GHOSTFOLIO, GOOGLE_SHEETS, MANUAL, RAPID_API, YAHOO.
- **Activities.**
  - Types: BUY, SELL, DIVIDEND, FEE, INTEREST, LIABILITY.
  - `ITEM` was converted to BUY and then removed in 2025. Source: GF `prisma/migrations/20250926182138_removed_item_from_type`.
  - An `Order` carries a comment, tags and an optional account.
- **Other models.**
  - `MarketData` is unique on `[dataSource, symbol, date]`, with state `CLOSE` or `INTRADAY` and an `isCarriedForward` flag.
  - `AssetProfileOverrides` stores user corrections to class, sectors, name and so on.
  - Source: GF `prisma/schema.prisma`.
- **Provider interface.** `canHandle`, `getAssetProfile`, `getDataProviderInfo`, `getDividends`, `getHistorical`, `getQuotes`, `search`, `getTestSymbol`, `getMaxNumberOfSymbolsPerRequest`. A separate data-enhancer layer (OpenFIGI, TrackInsight) fills in profiles. Source: GF `apps/api/src/services/data-provider/interfaces/data-provider.interface.ts`, `data-enhancer/`.
- **MANUAL scraper.** Configured with a URL, a selector (JSONPath for JSON, CSS otherwise) and a locale for number parsing. Source: GF `apps/api/src/services/data-provider/manual/manual.service.ts`.
- **Import.**
  - JSON with `activities`, `accounts`, `assetProfiles` and `tags`, validated by a DTO.
  - `isDryRun` previews the import, and rows are flagged `IS_DUPLICATE` on a field-tuple match.
  - CSV header aliases (`qty|quantity|shares|units`, ...).
  - Source: GF `apps/api/src/app/import/import.service.ts`, `apps/client/src/app/services/import-activities.service.ts`.
- **AI.**
  - Prompt export (`portfolio` / `analysis` modes).
  - Vision-model import from CSV, PDF or images, with unmatched rows returned.
  - A built-in MCP server with read tools and two write tools (`import-activities`, `update-account-balance`).
  - Source: GF `apps/api/src/app/endpoints/ai/ai.service.ts`, `endpoints/mcp/mcp.service.ts`.
- **Base currency** is a user setting and defaults to USD. Source: GF `libs/common/src/lib/config.ts`.

**Borrow**

- (source, symbol) identity plus optional global identifiers.
- A thin provider trait with `getTestSymbol` for health checks.
- A user-overrides table kept separate from provider data.
- Explicit `isCarriedForward` / stale markers on prices.
- Import with dry run, duplicate key and header aliases.
- A selector-based scraper as the free fallback for niche instruments.

**Avoid**

- A data-source enum in the schema.
- A coarse fixed class/subclass enum.
- A silent USD default (make the base currency required in config).
- Write tools over MCP.
- AI import as the source of truth.

## Portfolio Performance

**Facts**

- **Security.** Fields include `uuid`, `isin`, `tickerSymbol`, `wkn`, `currencyCode`, `calendar`, `feed` + `feedURL`, `latestFeed` + `latestFeedURL`, `attributes`, `events` and `note`. The fixed `type` and `industryClassification` fields are `@Deprecated`. Source: PP `model/Security.java`.
- **Transactions.**
  - `Transaction` has `crossEntry` (paired legs), `note` and `source`.
  - Each `Unit` (GROSS_VALUE / FEE / TAX) carries `forex` and `exchangeRate`.
  - Money is fixed-point (Amount to 2 decimals, Share to 8).
  - Source: PP `model/Transaction.java`, `money/Values.java`.
- **Ledgers.** Separate `Account` (cash; 13 types) and `Portfolio` (securities; 6 types). Source: PP `model/AccountTransaction.java`, `model/PortfolioTransaction.java`.
- **QuoteFeed.**
  - Methods: `getLatestQuote`, `getHistoricalQuotes(security, collectRawResponse)`, `previewHistoricalQuotes`, `getExchanges`.
  - `HistoricalUpdatePolicy` is MERGE, REPLACE or REPLACE_IF_SOURCE_CHANGED.
  - Feeds include Yahoo, Alpha Vantage, CoinGecko, EODHD, Finnhub, TwelveData, ECB, Eurostat and Portfolio Report, plus **Generic JSON** (JSONPath per field) and **Generic HTML table** (header regex).
  - Source: PP `online/QuoteFeed.java`, `online/impl/GenericJSONQuoteFeed.java`, `online/impl/HTMLTableQuoteFeed.java`.
- **Taxonomies.** Trees of `Classification` nodes. A weighted `Assignment` lets one security be split across categories. Templates (e.g. an asset-class template) ship as properties files. Source: PP `model/Taxonomy.java`, `model/Classification.java`, `model/taxonomy_templates/`.
- **Import.**
  - The CSV importer has column-to-field mapping, `FieldFormat`, delimiter, encoding and skip-lines settings. Configs are saved by name (built-in `csv-config.json` plus the user's own).
  - About 140 regex PDF extractors are tested against about 2,800 anonymised text fixtures.
  - Source: PP `datatransfer/csv/CSVImporter.java`, `CSVConfigManager.java`, [CONTRIBUTING.md](https://github.com/portfolio-performance/portfolio/blob/master/CONTRIBUTING.md).
- **Storage.** A single user-owned file: XML, ZIP, AES-encrypted or protobuf. Source: PP `model/ClientFactory.java`.

**Borrow**

- Weighted taxonomies with seeded templates.
- Generic JSON and HTML-table feeds configured per instrument, so a new free source needs no code.
- Separate history and latest-price feeds per instrument.
- The update policy and a raw-response preview.
- Fixed-point money with transaction units that carry forex and rate, plus `source` on each row.
- Saved named CSV templates.
- The fixture-in, field-matcher-out extractor test pattern.

**Avoid**

- Dual ledgers and accounting types.
- Per-bank PDF regex extractors (stay CSV-first).
- Storage as one monolithic file.
- An EUR default and German-centric identifiers.

## Fincept Terminal

**Facts**

- **Rewrites.** It went from Python/Rich, to Textual (1.0, Feb 2025), to DearPyGui, to Tauri + React (v3, Oct 2025), and is now **C++20/Qt6 with embedded Python** (v4, Mar 2026; v4.5.1 released 2026-10-13). Source: [README](https://github.com/Fincept-Corporation/FinceptTerminal/blob/main/README.md), [Production1.0.0](https://github.com/Fincept-Corporation/FinceptTerminal/releases/tag/Production1.0.0).
- **Architecture.**
  - 54 screens across 13 areas.
  - A "DataHub" fetches each topic (`market:*`) once, shares it with every subscriber, and serves the last good value when a fetch fails.
  - Screens must not call HTTP directly; the doc admits 6 screens break this rule.
  - About 1,400 Python scripts run as subprocesses (3 at a time, 0.5–1.5 s cold start).
  - Source: [docs/ARCHITECTURE.md](https://github.com/Fincept-Corporation/FinceptTerminal/blob/main/docs/ARCHITECTURE.md).
- **AI.** 37 agents. `McpService` gives chat, agents and the node editor one shared tool set, and external MCP servers can be added. Source: same doc.
- **Command bar.**
  - `CommandParser` first tries a single-token function code, then falls back to verb-object parsing with longest-prefix match and typed parameter slots.
  - A token that matches nothing is treated as a ticker and sent to the linked panels.
  - Source: [CommandParser.cpp](https://github.com/Fincept-Corporation/FinceptTerminal/blob/main/fincept-qt/src/ui/command/CommandParser.cpp).

**Borrow**

- One action registry behind both the parser and the palette, with typed slots and aliases.
- Fetch-once shared topics with a last-good fallback, in a worker layer.
- No network calls from screens; Delta's AGENTS.md already enforces this.

**Avoid**

- Breadth (54 screens).
- Subprocess connectors.
- Repeated platform rewrites.
- Treating any unknown token as a ticker without resolving it first.
- AGPL code.

## Terminal and TUI finance apps

| App | Stack, status | Notable | Source |
|---|---|---|---|
| tickrs | Rust/ratatui; last commit 2026-05 | Line, candle and kagi charts, options; Yahoo only; YAML config | [repo](https://github.com/tarkah/tickrs), [help.rs](https://github.com/tarkah/tickrs/blob/master/src/widget/help.rs) |
| ticker | Go/Bubble Tea; v5.3.0 2026-06 | The symbol suffix picks the source (`.X`, `.CB`); lots and groups in YAML; handles minor-unit currencies (GBp); `ticker print` outputs JSON/CSV | [repo](https://github.com/achannarasappa/ticker) |
| cointop | Go; **archived** 2024-04 | htop and vim style; TOML maps keys to named actions (`"/" = "open_search"`); `holdings --format json` | [config.md](https://github.com/cointop-sh/cointop/blob/master/docs/content/config.md) |
| longbridge-terminal | Rust/ratatui 0.30; active | **CLI-first for scripts and agents**, with the TUI on top; `--format json`; market-suffixed symbols (`TSLA.US`) | [README](https://github.com/longbridge/longbridge-terminal/blob/main/README.md) |
| alphai-tui | Rust/ratatui; active | A quote line that stays across views; every quote labelled with its feed and timestamp, and delayed or stale prices marked | [repo](https://github.com/makeev/alphai-tui) |
| OpenBB Terminal v3 | Python; superseded | Path-style menus (`/stocks/options`, `..`); `.openbb` routines with `$ARGV[n]`, variables and relative dates | [navigation](https://github.com/OpenBB-finance/OpenBB/blob/v3.2.5/website/content/terminal/usage/overview/structure-and-navigation.md), [routines](https://github.com/OpenBB-finance/OpenBB/blob/v3.2.5/website/content/terminal/usage/routines/routines-for-power-users.md) |

**Command-grammar prior art**

- **Bloomberg:** `TICKER <MARKET> FUNCTION <GO>`, where yellow keys pick the market sector (EQUITY, CORP, GOVT, COMDTY, INDEX, CURNCY, ...) and `DES` is the description. No public Bloomberg-authored page was found, so the sources are library guides: [Illinois guide](https://guides.library.illinois.edu/bloomberg_user_guide/the_bloomberg_keyboard), [Tennessee Tech guide](https://ouweb.tntech.edu/business/pdf/resources/Bloomberg_User-Guide_May-2021.pdf).
- **k9s:** a `:` command mode (`:pod ns-x`), `/` to filter, back/forward history, `aliases.yaml` and `hotkeys.yaml` that expand to command strings. Source: [README](https://github.com/derailed/k9s/blob/master/README.md).

**Borrow**

- **Grammar:** `<instrument> [<class>] <FUNC> [args]`, with the class playing the part of Bloomberg's yellow key and only needed when a symbol is ambiguous.
- **Navigation:** `:` command mode, `/` filter, history, user aliases and hotkeys mapped to named actions in config.
- **Routines:** plain command lists with `$ARGV`, run headless by the same parser.
- **Headless output:** the same output as JSON from the CLI.
- **Labels:** feed and timestamp on every displayed value.

**Avoid**

- Deep menu trees as the main way around; everything should hang off the dossier.
- Single-source designs.
- Per-view one-letter toggles that don't scale.

## MCP servers for finance data

| Server | Surface | Provenance in results | Licence | Source |
|---|---|---|---|---|
| OpenBB MCP | One tool per route, plus a discovery mode | provider, route, arguments, timestamp; no URLs | Apache-2.0 | OBB `extensions/mcp_server/README.md` |
| financial-datasets | 10 tools, `json.dumps` of the API payload; stale (2025-06) | None | MIT | [server.py](https://github.com/financial-datasets/mcp-server/blob/main/server.py) |
| Alpha Vantage (official) | One tool per API function; `readOnlyHint` | None | MIT | [registry.py](https://github.com/alphavantage/alpha_vantage_mcp/blob/main/mcp/src/av_mcp/tools/registry.py) |
| yfinance-mcp (narumiruna) | Ticker info, news, screen; read-only and open-world hints | None | MIT | [server.py](https://github.com/narumiruna/yfinance-mcp/blob/main/src/yfmcp/server.py) |
| **sec-edgar-mcp** | 21 tools (filings, content by accession, Form 4) | **Accession, filing date and URL on every result** | AGPL-3.0 | [filings.py](https://github.com/stefanoamorelli/sec-edgar-mcp/blob/main/sec_edgar_mcp/tools/filings.py) |
| Massive (ex-Polygon) | `search_endpoints`, `call_api`, `query_data` (SQL) | Strips `request_id` | MIT | [server.py](https://github.com/massive-com/mcp_massive/blob/main/src/mcp_massive/server.py) |
| CoinGecko (official) | Docs search plus agent-written TypeScript run in a sandbox ("Code Mode") | None | Apache-2.0 | [mcp-server](https://github.com/coingecko/coingecko-typescript/tree/main/packages/mcp-server) |
| EODHD (official) | 88 tools; 3 are not read-only (they generate reports and send email) | Partial | MIT | [repo](https://github.com/EodHistoricalData/EODHD-MCP-Server) |
| Alpaca (official) | Toolsets derived from OpenAPI; places orders (`destructiveHint`) | n/a | MIT | [overrides.py](https://github.com/alpacahq/alpaca-mcp-server/blob/main/src/alpaca_mcp_server/overrides.py) |
| Ghostfolio | Built-in read tools plus import and balance-update writes | n/a | AGPL-3.0 | GF `apps/api/src/app/endpoints/mcp/mcp.controller.ts` |

**Spec facts (MCP 2026-07-28, the current version)**

- **Breaking changes in this version:** the `initialize` handshake is gone, `server/discover` is mandatory, sessions are removed, and list/read results carry `ttlMs`/`cacheScope`. Source: [changelog](https://github.com/modelcontextprotocol/modelcontextprotocol/blob/main/docs/specification/2026-07-28/changelog.mdx).
- **Resources:** identified by RFC 3986 URIs, and custom schemes are allowed (`delta://evidence/{id}` is valid). RFC 6570 templates have completion support. Annotations are `audience`, `priority` and `lastModified`. Source: [resources](https://modelcontextprotocol.io/specification/2026-07-28/server/resources).
- **Tool results:** tools may return `resource_link` blocks. Once an `outputSchema` is declared, `structuredContent` MUST conform to it. Source: [tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools).
- **Tool annotations:** `readOnlyHint` (default false) and `openWorldHint` (default true) are untrusted hints, so read-only must be enforced in services.
- **`rmcp` (the Rust SDK):** v3.5.1 (2026-10-05) implements 2026-07-28. Transports: stdio, child process, streamable HTTP as a Tower service, in-process. It has `#[tool]` macros and resource handlers. Source: [rust-sdk](https://github.com/modelcontextprotocol/rust-sdk).

**Borrow**

- **Resource templates** for evidence, reports, theses and notes, with `lastModified` set to when the item was gathered.
- **About 5–7 read-only tools:** `search_evidence`, `get_evidence`, `list_reports`, `get_report`, `get_thesis`, and optionally `verify_citations(text)`, which reuses the existing evidence-id validator. Each returns `resource_link`s and has an `outputSchema` that requires the provenance fields.
- **Annotations:** set `openWorldHint=false` explicitly.
- **Notes:** label them `kind: "user_view"` and say they cannot be cited.
- **Transport:** stdio first; HTTP only with auth.

**Avoid**

- Pass-through fetch tools; any fetch must go through ingest.
- SQL or code-execution tools.
- Hundreds of tools behind meta-discovery.
- Write tools (acceptance and decisions stay with the user).
- Exposing web results.

## Implications for open tickets

- **#46 Markets and coverage:**
  - Key exchanges by MIC.
  - Each source declares (standard model × market × asset class) capabilities.
  - Per-source symbol maps live in the central registry, not in fetchers.
  - Config fallback order per function.
  - A coverage view can be generated from the declarations.
  - Handle minor-unit currencies (GBp, ZAc).
- **#47 Asset classes:**
  - One `Instrument` with a small behavioural `AssetClass`.
  - Shared bar and quote models.
  - Per-class metrics as typed extensions.
  - Weighted user taxonomies for classification.
  - Macro series could be instruments of class `series`; OpenBB's separate `economy` router is the counter-example.
- **#48 Dossier and grammar:**
  - One action registry with typed slots.
  - `<instrument> [<class>] <FUNC>`, `:` mode, aliases and hotkeys.
  - Routines run headless.
  - Resolve before falling back to a ticker.
- **#50 Notes:** neither Ghostfolio nor Portfolio Performance goes beyond a `note`/`comment` field and tags, so there is no prior art for notes linked to evidence. MCP should label notes as `user_view`.
- **#51 Light portfolio:**
  - A single transaction table: BUY, SELL, DIVIDEND (cash only), FEE, DEPOSIT, WITHDRAW, SPLIT, and maybe TRANSFER.
  - Units with forex and rate, fixed-point money, `source` on each row.
  - Saved CSV templates, dry run and a duplicate key.
  - The base currency is required.
  - ECB as the free FX source.
- **#53 MCP:**
  - Build on `rmcp` (adding the dependency needs approval).
  - Target spec 2026-07-28.
  - Resources plus read-only tools, as above.
  - Add a headless `delta gather` for scheduling (CLI-first, as longbridge does).
- **Free-data default:** OpenBB V5 dropping yfinance and FMP from its core is a signal. Prefer official sources (SEC, FRED, ECB, exchange feeds) as defaults and treat Yahoo as best-effort. Generic JSON and HTML feeds cover the long tail.
