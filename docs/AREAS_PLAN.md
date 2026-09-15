# Areas Plan — any asset class or sector as a plugin

> Steps 0-7 of a 10-step design. Same working rules as Phase 2
> (`docs/PHASE2_PLAN.md`). Composes with `docs/MODEL_SELECTION_PLAN.md`.

## Context

Rigger's plugin system already makes data sources, strategies, brokers and
reports cheap to add. Universe membership is not: `[universe]` in `config.toml`
is `market -> [tickers]`, and `Instrument` carries only `market` and a free-text
`sector`. `Instrument.market` is doing four jobs at once — venue identity,
currency key, fee key, and the only classification axis — and those are three
orthogonal things:

| Axis | Question | Cardinality | Drives |
|---|---|---|---|
| **Venue** | Where does it trade? | exactly one | currency, fees, calendar, vendor symbology |
| **Asset class** | What kind of thing is it? | exactly one | price semantics, sizing, which brief sections mean anything |
| **Area / theme** | What is it *about*? | many, open-ended | universe membership, exposure buckets, prompt choice, scorecard grouping |

The goal is to add an **area of interest** — asset class *or* sector/theme — as
one plugin, without committing to which comes first. Today neither works: a bond
has nowhere to declare it is a bond, and a mining slice has nowhere to declare
its members or its extra context.

Five things block it:

1. `Instrument` has no `asset_class` and no tags. `sector` is free text set by a
   hardcoded 7-symbol dict (`rigger/plugins/markets/us.py:15-23`); every ASX name
   is `None`, so `max_sector_pct` is inert for the entire ASX sleeve.
2. Strategies take `ctx.universe` wholesale. `Momentum` ranks the whole mixed
   universe cross-sectionally in one pool on a 253/22-bar equity convention, so
   adding bonds moves the quintile boundary and silently changes *existing*
   equity signals.
3. `build_brief` has a hardcoded five-section tuple and returns `None` without
   bars — a bond priced off a yield series can never get a brief.
4. One analyst prompt for everything: `ANALYST_TEMPLATE = "analyst_v1.j2"`.
5. `MARKET_FEES` (`rigger/paper/portfolio.py:14-18`) falls back to `(0.0, 0.0)`
   for an unknown market, so a new asset class trades free by accident.

**Outcome:** a sector area is six lines of TOML and no Python. An asset-class
area is one file. Strategies, briefs, prompts and risk limits all scope to areas
automatically.

## Decisions taken

- **Scope: steps 0 through 7.** Areas exist *and* mean something downstream.
  Steps 8–9 (instrument registry, ingest/FX cleanup) are deferred — see
  *Deferred* at the end for what that costs.
- **`Instrument.market` is renamed to `venue`**, with a pydantic
  `AliasChoices("venue", "market")` validation alias and a `market` property, so
  every existing construction and read site keeps working with zero test edits.

## The abstraction

Two typed axes plus one tag axis — not one flat namespace.

`asset_class` is a closed `Literal`, not a tag, because it must *dispatch
behaviour*: which fee model, which sizing unit, whether `close` is a price or a
clean price, whether `volume` is meaningful, whether Momentum may include it. As
a tag, each of those becomes a string search at the call site — exactly the
`if name == "yfinance":` hack in `services.py` that this codebase already
regrets. `venue` must be single-valued because it is the lookup key for currency
and fees; a set cannot be a foreign key. `tags` stay open-ended because themes
genuinely are many-to-many — BHP is Materials *and* mining *and* ASX200.

### `rigger/core/models.py`

```python
AssetClass = Literal["equity", "etf", "bond", "fx", "commodity", "crypto", "cash", "other"]

class Instrument(BaseModel):
    model_config = ConfigDict(populate_by_name=True)

    id: str                                    # built by make_instrument_id()
    venue: str = Field(validation_alias=AliasChoices("venue", "market"))
    asset_class: AssetClass = "equity"
    symbol: str
    name: str | None = None
    currency: str

    areas: tuple[str, ...] = ()                # authoritative membership
    sector: str | None = None
    industry: str | None = None
    tags: frozenset[str] = frozenset()

    meta: dict[str, Any] = Field(default_factory=dict)   # {"coupon": 4.25, "maturity": ...}

    @property
    def market(self) -> str:
        """Deprecated alias for venue."""
        return self.venue
```

`rigger/core/ids.py` gains `make_instrument_id(venue, symbol)` and
`split_instrument_id()`, replacing the hand-rolled `f"US:{s}"` / `f"ASX:{code}"`
f-strings in each market plugin.

### `rigger/core/plugin.py`

```python
class VenuePlugin(Plugin):
    """Where instruments trade. Authoritative for money mechanics."""
    currency: str
    timezone: str = "UTC"
    fee_bps: float = 0.0
    fee_min: float = 0.0
    def fee(self, notional: float) -> float: ...
    def is_open(self, ts) -> bool: ...        # still no caller; kept, not invested in
    def next_open(self, ts) -> datetime: ...

MarketPlugin = VenuePlugin                     # deprecated alias


class AreaPlugin(Plugin):
    """A user's area of interest. Instantiated ONCE PER [areas.<name>] table,
    not once per entry point, so one class can back many areas."""

    kind: str                                  # entry-point name in the `rigger.areas` group
    label: str = ""
    default_venue: str | None = None
    default_asset_class: AssetClass = "equity"
    name: str = ""                             # set by the loader from the table key

    def instruments(self) -> list[Instrument]: ...

    # every default below means "behave exactly as today"
    def brief_sections(self, default: list[SectionBuilder]) -> list[SectionBuilder]:
        return default
    analyst_template: str | None = None
    prompt_dir: Path | None = None
    def prompt_vars(self, inst, brief) -> dict[str, Any]:
        return {}


class Scope(BaseModel):
    """Declarative universe filter. AND across axes, OR within an axis."""
    areas: tuple[str, ...] = ()
    asset_classes: tuple[AssetClass, ...] = ()
    venues: tuple[str, ...] = ()
    tags: tuple[str, ...] = ()
    def matches(self, inst) -> bool: ...
    def filter(self, insts) -> list[Instrument]: ...
```

`StrategyPlugin` and `DataPlugin` each gain `scope: Scope = Scope()`, settable
from `[plugins.<name>].scope`; the existing `DataPlugin.market` is folded into
`scope.venues` by the loader so no data plugin needs editing. `Context` gains
`areas: dict[str, AreaPlugin]` and `areas_for(inst)`.

### A separate `rigger.areas` entry-point group

The existing `rigger.plugins` group is a singleton registry keyed by the class
attribute `name`. Areas must be **multi-instance** — three sector areas all
backed by one built-in `TickerArea` class — which would either break
`discover_plugins()`'s contract or force every author to subclass just to get a
name. A parallel group keeps both contracts clean:

```python
AREA_GROUP = "rigger.areas"
def discover_area_kinds() -> dict[str, type[AreaPlugin]]: ...
def build_areas(cfg_areas) -> dict[str, AreaPlugin]:
    """[areas.<name>] with kind = "<entry point>" -> one configured instance each."""
```

## What a user writes

### (a) ASX mining — zero Python

This is the acceptance test for "easy". If a sector area needs a Python file,
the design has failed.

```toml
[areas.asx_mining]
kind        = "tickers"          # built-in area kind
label       = "ASX Mining"
venue       = "asx"
asset_class = "equity"
sector      = "Materials"
tags        = ["mining", "resources"]
tickers     = ["BHP", "RIO", "FMG", "S32", "MIN", "PLS"]

[risk.buckets.area]
asx_mining = 35
```

Nothing in `pyproject.toml`. `TickerArea` (~30 lines,
`rigger/plugins/areas/tickers.py`) is registered once as the `tickers` kind and
reads `venue`, `asset_class`, `sector`, `tags`, `tickers` and per-symbol
`instruments` overrides from the table. As a side effect these names finally
carry a `sector`, so `max_sector_pct` stops being inert for them.

Python is needed only for a custom brief section or prompt:

```python
class ASXMiningArea(TickerArea):
    kind = "asx_mining"
    analyst_template = "analyst_mining_v1.j2"
    prompt_dir = Path(__file__).parent / "prompts"

    def brief_sections(self, default):
        return [*default, IronOreSpotSection()]
```

### (b) Australian government bonds — one file

```python
class AGBVenue(VenuePlugin):
    name, currency, timezone = "agb", "AUD", "Australia/Sydney"
    fee_bps = fee_min = 0.0          # OTC: dealer spread, not brokerage

class AusGovtBondsArea(AreaPlugin):
    kind = "aus_govt_bonds"
    default_venue, default_asset_class = "agb", "bond"
    analyst_template = "analyst_rates_v1.j2"

    def instruments(self):
        return [Instrument(id=make_instrument_id("agb", ln["code"]), venue="agb",
                           asset_class="bond", symbol=ln["code"], currency="AUD",
                           areas=(self.name,), tags=frozenset({"govt", "rates"}),
                           meta={"coupon": ln["coupon"], "maturity": ln["maturity"], "par": 100.0})
                for ln in self.lines]

    def brief_sections(self, default):
        # Replace, not append: a bond brief led by a 20-day SMA is noise.
        return replace_section(default, "prices", YieldSection(required=True))
```

**In scope and genuinely delivered:** bond instrument modelling with coupon and
maturity in `meta`; its own venue and *declared* zero fee rather than the
accidental `(0,0)` fallback; a brief that replaces the price section; its own
prompt and model route; exclusion from Momentum's pool; its own exposure bucket.

**Out of scope, and the harness is quietly wrong until they are done:**

- **Bars.** The cheap correct move needs no schema change — store clean-price
  bars with `volume = 0.0` and put yield/duration/spread into `FundamentalTable`
  as metric rows, which `brief._fundamentals` already renders generically.
- **Pricing and P&L.** `qty * close * fx_rate` with no accrued interest, coupon
  cashflow or dirty price. It runs and is internally consistent; it is not bond
  P&L. Say so in the docs.
- **Sizing by duration/DV01.** `qty = notional / price` hands you a fractional
  number of $100-par units. Mitigation is a hook, not an implementation: add
  `rigger/core/policy.py` with an `AssetClassPolicy` Protocol
  (`lot_size`, `position_value`, `price_is_meaningful`) and a
  `POLICIES: dict[AssetClass, AssetClassPolicy]` registry. Implement
  `EquityPolicy` only; everything else falls back to it. `risk.size_signal` and
  `PaperPortfolio.equity` route through it so coupon accrual lands later behind
  an existing seam.

## Steps

Each is a separate commit, green on its own, leaving `rig run` behaviour
identical unless stated. **The gate throughout: the 16 existing test modules
pass unedited**, with one named exception in step 2.

**0 — Instrument model + id helpers.** *No behaviour change.* New fields with
the `venue`/`market` alias; `make_instrument_id`/`split_instrument_id`;
`us.py:44` stops hardcoding `"USD"` and uses `self.currency`.
*Tests:* new `tests/test_instrument.py` — alias both directions, defaults, id
round-trip.

**1 — `VenuePlugin`.** Rename with the alias; move `fee_bps`/`fee_min`/`fee()`
onto it; delete `MARKET_FEES` and the dead `ASXMarket.fee()`; an unknown venue
now **raises** instead of trading free. `runtime.py:42-46` builds venues from
`isinstance(p, VenuePlugin)`.
*Tests:* new `tests/test_venue.py`; `test_portfolio.py`, `test_asx_market.py`
unedited.

**2 — `AreaPlugin` + loader + `TickerArea` + `[areas.*]`.** The `rigger.areas`
group, `discover_area_kinds()`, `build_areas()`, `AppConfig.areas`.
`Rigger.universe()` becomes the union of area instruments, deduped by id with
`areas` tuples merged. **`[universe] us = [...]` keeps working forever** — the
loader translates each key into an implicit `TickerArea`; do not migrate the
TOML. `VenuePlugin.universe()` is removed; `USMarket`/`ASXMarket` become pure
venues. This also kills two live bugs in `runtime.py:30-33`: a `[universe]` key
colliding with a non-market plugin silently wiping its config, and
`ASXMarket.configure` clobbering the rest of `[plugins.asx]` — areas are
configured once by the loader and never re-`configure()`d.
*Tests:* new `tests/test_areas.py` — multi-instance from one kind, the
`[universe]` shim producing byte-identical instruments, id collisions merging,
unknown `kind`/`venue` failing loudly. `tests/test_services.py:47-52` needs a
small edit (it constructs `USMarket()` directly) — the only existing test this
plan touches.

**3 — `Scope` + strategy scoping + Momentum cohorts.** Each strategy changes one
line: `for inst in ctx.universe:` becomes `for inst in self.universe(ctx):`
(`momentum.py:62`, `llm_analyst.py:88`, `ensemble.py:41`, `critic.py:59`).
Scope alone only *excludes* bonds; it would not stop ASX small caps being ranked
against US mega caps, so `Momentum` also gains
`rank_by: Literal["area", "asset_class", "venue", "all"] = "asset_class"` and
ranks within each cohort.
*Tests:* new `tests/test_scope.py`; new cohort-independence cases in
`test_momentum.py`, existing cases unedited (its five US equities are one cohort
under every `rank_by` value).

**4 — Risk buckets + mark-to-market exposure.** `BucketKind = Literal["sector",
"area", "asset_class", "venue", "tag"]`, `BucketLimits`, and the sector gate
generalised to a loop over `limits.buckets` — **gate order preserved exactly**.
`AppConfig.risk` becomes a typed `RiskConfig` so `[risk.buckets.area]` parses;
`max_sector_pct = 25` keeps working through a shim, and `sector_exposure_pct`
survives as a deprecated kwarg that seeds the sector bucket, which is what keeps
`test_risk.py` unedited. Separately fix `services.execute.exposures()`, which
measures exposure from `pos.qty * pos.avg_price` — cost basis, so it drifts from
reality as prices move.
*Tests:* new `tests/test_risk_buckets.py`; one new `test_services.py` case
asserting exposure moves with price.

**5 — Pluggable brief sections.** `SectionBuilder` Protocol (`key`, `required`,
`build`), `CORE_SECTIONS` in today's exact order, `replace_section()` exported
for area authors, and `Brief` holding a section list with the named
`prices`/`news`/`events`/`fundamentals`/`calendar` properties preserved verbatim.
Byte compatibility is structural, not incidental: an instrument with no areas
gets exactly `CORE_SECTIONS` in exactly the current order. The no-bars hard
return stops being welded into the assembler and becomes
`PricesSection.required = True`, so a bond area replacing `prices` with
`YieldSection(required=True)` inherits the same "don't analyse an instrument
with no data" guarantee.
*Tests:* `test_brief.py` **unedited is the gate**; new
`tests/test_brief_sections.py` for append, replace-by-key, required-None
returning None, and byte-identical render with no areas.

**6 — Pluggable prompt + area model override.** `analyst_template_for(ctx,
inst)`; `render_prompt` gains a `ChoiceLoader` search path so an area ships its
own templates; extra Jinja vars (`venue`, `asset_class`, `sector`, `tags`,
`area`, plus `area.prompt_vars()`). `StrictUndefined` fires only on *missing*
vars, so `analyst_v1.j2` renders byte-identically and **existing `LLMClient`
cache entries stay valid** — which matters for Phase 3's backtest replay
economics. `prompt_version` is still the template name, so area templates must
use distinct names (`analyst_rates_v1.j2`), which keeps the Phase 3 scorecard's
`by=prompt` grouping meaningful for free.
*Tests:* new `tests/test_prompt_seam.py` — area template wins, default
unchanged, and the rendered prompt *and cache key* for a no-area instrument are
byte-identical.

**7 — Surface + docs.** `rig areas list` / `rig areas show <name>`; the TUI
Config screen renders areas instead of `cfg.universe`
(`rigger/tui/screens/config.py:28-34`); `docs/AREAS.md` with both worked
examples; `PROJECT_SPEC.md` §6 updated with the venue-vs-area invariant.

### Composes with the model-selection plan

`docs/MODEL_SELECTION_PLAN.md` step 2 widens by one parameter rather than
conflicting:

```python
def model_for(config, task, plugin=None, area=None) -> str:
    """[areas.<area>].model -> [plugins.<plugin>].model -> [llm.routing].<task> -> KeyError."""
```

Same loud `KeyError`, same message. That plan's TUI picker then gets an "Areas"
row group beside "Tasks" and "Plugins" with no new machinery.

## Deferred, and what it costs

- **Instrument registry / writing `InstrumentTable`.** Until this lands,
  `market_of()` id-prefix parsing stays the currency and fee resolver, and Phase
  3's scorecard sector grouping keeps reading `"unknown"` for 100% of rows
  (`docs/agents/phase3/02-agent-scorecard.md` expects a table nothing writes),
  which blocks Phase 3 workstream B. Bounded and reversible.
- **Ingest contract cleanup** — deleting the hardcoded `if name == "yfinance":`
  FX special case by making FX a real built-in area, and dropping the redundant
  internal re-filters in `sec_edgar.py:84` and `asx_announcements.py:61`.
- Exchange holiday calendars (`is_open`/`next_open` still have no caller
  anywhere); FKs from `bar`/`position`/`signal` to `instrument.id`; multi-lot
  positions and shorts; bond analytics beyond the `AssetClassPolicy` hook;
  migrating `[universe]` out of `config.toml` — the compat shim is permanent;
  `daily_loss_halt_pct`, still loaded and read by nothing.

**What must not be deferred:** step 0's `asset_class` field. Everything
downstream dispatches on it, and retrofitting a typed axis after strategies,
risk, briefs and prompts have been written against tags is the one part of this
that gets dramatically more expensive with delay.

## Sharp edges to document

- **Sector and area caps double-count.** `asx_mining` sets both `sector` and
  `area`, so a position hits two gates and the effective cap is the tighter one.
  Defensible, but undocumented it makes rejections baffling.
- **Two sources of truth for "is the market open"** is how orders get submitted
  into a closed session — `is_open`/`next_open` stay on `VenuePlugin` only.
- **`EventKind` is triplicated** (`models.py:48-57`, `extract_v1.j2:20`,
  `extract.py:32`). A rates or commodities area cannot add `rate_decision` or
  `inventory_report` without editing three places plus a Jinja template, which
  contradicts "an area is one file". First thing that will bite a non-equity
  area; worth its own small step sourcing the template's list from one exported
  tuple.

## Verification

1. `pytest` green at every step; 16 existing modules unedited except
   `test_services.py` in step 2. `mypy --strict rigger/core rigger/llm` and
   `ruff check .` clean.
2. `rig areas list` shows `asx_mining` (6 members, venue `asx`) and
   `aus_govt_bonds` (2 members, venue `agb`) — the mining one added with no
   Python written.
3. `rig ingest --venue asx` — yfinance fetches mining names with `.AX` suffixes
   and does not attempt `GSBG33`.
4. `rig analyse --strategy llm_analyst`, then
   `sqlite3 data/rigger.db "select instrument_id, prompt_version, model from signal"`
   — mining names carry `analyst_mining_v1`, bonds `analyst_rates_v1`, the rest
   `analyst_v1`.
5. `rig analyse --strategy momentum` with bonds in the universe — bonds produce
   no momentum signal, and the equity signals are byte-identical to a run with
   the bond area disabled. This is the regression that matters most.
6. `rig execute` then `rig paper status` — a bond fill is charged the declared
   `agb` fee; a mining position is refused once `[risk.buckets.area].asx_mining`
   is reached, with that reason in the skip log.
