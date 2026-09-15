# Watchlists Plan — follow what you're interested in

> Deciding what to follow, aimed at a hobby investor rather than a desk. Same
> working rules as Phase 2: no behaviour change without a test, `pytest` stays
> offline. Composes with `docs/MODEL_SELECTION_PLAN.md`.

## Context

Rigger is for someone who enjoys investing as a hobby and would like to make
some money at it — not for a desk quant. Right now, deciding *what to follow*
is the least approachable part of the whole app.

To follow mining stocks today you edit `[universe] asx = [...]` and hope. There
is nowhere to say "these six are my mining picks", nowhere to say "don't put
more than a third of my money in them", and nowhere to say "when you research
these, look at the iron ore price too". And if you add something that isn't a
share — a bond, say — the app has no idea it isn't a share, so it prices it,
sizes it and ranks it as though it were one.

**A watchlist is a named group of things you're interested in.** "Mining."
"Aussie banks." "Government bonds." You give it a name and some tickers; if you
want, you give it its own research notes and its own limits. That's the whole
idea, and it's the only new word anyone has to learn.

### What you'll be able to do

```toml
[watchlists.mining]
market  = "asx"
tickers = ["BHP", "RIO", "FMG", "S32", "MIN", "PLS"]
max_pct = 35                     # never more than 35% of my money in mining
```

```
$ rig watchlist add mining --market asx --tickers BHP,RIO,FMG
$ rig watchlist list
  mining      asx   6 holdings   max 35%
  banks       asx   4 holdings   max 25%
$ rig watchlist show mining
```

Nothing else is required. Market and tickers are the only two things you must
say — market because BHP lists in both Sydney and London, and guessing wrongly
would buy you the wrong shares in the wrong currency.

Everything beyond that is optional and stays out of your way until you go
looking for it: your own research prompt, your own model, your own extra brief
sections, a different kind of thing entirely (bonds, ETFs).

## What has to change under the hood

Four problems, none of them visible to the person using the app:

1. **An instrument can't say what it is.** `Instrument` has `market` and a
   free-text `sector` that is only ever filled in by a hardcoded 7-symbol dict
   (`rigger/plugins/markets/us.py:15-23`) — every ASX name is `None`, which is
   why the existing 25% sector limit silently never applies to any ASX holding.
2. **Strategies see everything.** `Momentum` ranks the entire universe in one
   pool on a twelve-month share convention. Drop bonds in and it moves the
   cut-off, changing the signals it gives for your *shares*. This is the one
   genuine footgun in the whole plan.
3. **Research is one-size-fits-all.** `build_brief` has five fixed sections and
   gives up entirely if there are no daily price bars, and there is a single
   analyst prompt (`analyst_v1.j2`) for everything.
4. **Money mechanics guess.** Brokerage is looked up from the text before the
   colon in the instrument id, falling back to *zero fees* for anything it
   doesn't recognise (`rigger/paper/portfolio.py:14-18`).

### One reversal from the previous plan

The earlier version renamed `Instrument.market` to `venue`. **Keep `market`.**
The confusion was never the word — it was that one field was doing three jobs.
Adding `asset_class` and `watchlists` alongside it fixes that, and "market"
is the word someone already knows from their broker app. `venue` would be a new
piece of jargon bought for nothing.

## Phases

Each phase is shippable on its own and leaves `pytest` green. **The gate
throughout: the 16 existing test modules pass unedited**, with one named
exception in Phase 1.

---

### Phase 1 — You can make a watchlist

*What you get:* `rig watchlist add/list/show`, watchlists in `config.toml` and
on the TUI Config screen, and your existing `[universe]` block still working
exactly as it does today.

1. **`Instrument` learns what it is** (`rigger/core/models.py`). New fields,
   all defaulted so every existing call site and test literal is untouched:

   ```python
   AssetClass = Literal["equity", "etf", "bond", "fx", "commodity", "crypto", "cash", "other"]

   asset_class: AssetClass = "equity"
   watchlists: tuple[str, ...] = ()
   tags: frozenset[str] = frozenset()      # "mining", "lithium" — free-form
   industry: str | None = None
   meta: dict[str, Any] = {}               # {"coupon": 4.25, "maturity": "2035-04-21"}
   ```

   `asset_class` is a closed `Literal`, not a tag, because code has to *decide*
   things from it — which fee model, whether `volume` means anything, whether
   Momentum may include it. As a loose string, each of those becomes a text
   comparison scattered through the codebase.

   `rigger/core/ids.py` gains `make_instrument_id(market, symbol)` so the
   `f"US:{s}"` f-strings stop being hand-rolled in each plugin.

2. **`WatchlistPlugin`** (`rigger/core/plugin.py`) — one class, instantiated
   once per `[watchlists.<name>]` table rather than once per entry point, so a
   single built-in class can back all of your watchlists:

   ```python
   class WatchlistPlugin(Plugin):
       kind: str                 # entry-point name in the new `rigger.watchlists` group
       name: str = ""            # the [watchlists.<name>] key
       label: str = ""

       def instruments(self) -> list[Instrument]: ...

       # optional, all default to "behave exactly as today"
       def brief_sections(self, default): return default
       analyst_template: str | None = None
       prompt_dir: Path | None = None
       def prompt_vars(self, inst, brief): return {}
   ```

   A separate `rigger.watchlists` entry-point group, because the existing
   `rigger.plugins` group keys on a class attribute and assumes one instance
   per class — mixing multi-instance factories in would break that contract or
   force people to subclass just to get a name.

3. **`TickerWatchlist`**, the built-in kind (~30 lines,
   `rigger/plugins/watchlists/tickers.py`). Reads `market`, `tickers`, and
   optional `asset_class`, `sector`, `tags`, `max_pct` and per-symbol overrides.
   This is what makes a watchlist zero-Python.

4. **Wiring** (`rigger/runtime.py`). `Rigger.universe()` becomes the union of
   watchlist instruments, deduped by id with `watchlists` tuples merged. The
   existing `[universe] asx = [...]` is translated by the loader into an
   implicit `TickerWatchlist` — **it keeps working permanently**; nobody has to
   migrate their config. This also removes two live bugs at `runtime.py:30-33`:
   a `[universe]` key that happens to match a non-market plugin silently wipes
   that plugin's config, and `ASXMarket.configure` clobbers the rest of
   `[plugins.asx]`.

5. **Surface.** `rig watchlist add/remove/list/show`, writing `config.toml`
   through the same `load_toml` → mutate → `tomli_w.dumps` path as
   `set_plugin_enabled` (`rigger/services.py:350`) so nobody hand-edits TOML.
   Validation failures say what to do: *"watchlist 'mining' names market 'asz';
   known markets are asx, us"*. TUI Config screen lists watchlists instead of
   `cfg.universe` (`rigger/tui/screens/config.py:28-34`).

*Tests:* new `tests/test_watchlists.py` — several watchlists from one kind, the
`[universe]` shim producing byte-identical instruments to today, a ticker in two
watchlists merging rather than duplicating, unknown market failing loudly.
`tests/test_services.py:47-52` needs a small edit (it constructs `USMarket()`
directly) — **the only existing test this whole plan touches.**

---

### Phase 2 — Your watchlist stays in its own lane

*What you get:* adding bonds, or any non-share, stops quietly changing the
signals you get for your shares. This is a safety fix, and it must land before
anyone adds a non-share watchlist.

1. **`Scope`** — a small declarative filter on `StrategyPlugin` and
   `DataPlugin` (`watchlists`, `asset_classes`, `markets`, `tags`; AND across
   axes, OR within one), settable from `[plugins.<name>].scope`. Each strategy
   changes one line: `for inst in ctx.universe:` becomes
   `for inst in self.universe(ctx):` — four call sites (`momentum.py:62`,
   `llm_analyst.py:88`, `ensemble.py:41`, `critic.py:59`). `DataPlugin.market`
   folds into `scope.markets`, so no data plugin needs editing.

2. **Momentum ranks within a group.** Scope alone would only exclude bonds; it
   wouldn't stop ASX small caps being ranked against US mega caps, which is the
   same bug in a milder form. `Momentum` gains
   `rank_by: Literal["watchlist", "asset_class", "market", "all"] = "asset_class"`
   and runs its sort and top-quintile cut per group.

*Tests:* new `tests/test_scope.py`; new group-independence cases in
`test_momentum.py` — its five US shares are one group under every `rank_by`
value, so the existing cases stay unedited.

---

### Phase 3 — Your watchlist gets its own research

*What you get:* a mining watchlist whose briefs include the iron ore price and
whose analyst prompt knows it's looking at a miner; a bond watchlist whose brief
leads with yields instead of a 20-day moving average.

1. **Brief sections become a list, not a tuple** (`rigger/brief.py`). A
   `SectionBuilder` protocol with `key`, `required` and `build()`;
   `CORE_SECTIONS` in today's exact order; a `replace_section()` helper for
   watchlist authors. `Brief` keeps its named `prices`/`news`/`events`/
   `fundamentals`/`calendar` accessors verbatim.

   Byte-compatibility is structural, not lucky: an instrument in no watchlist
   gets exactly `CORE_SECTIONS` in exactly the current order, so
   `tests/test_brief.py` passes **unedited** — that is the acceptance test for
   this phase.

   The "no price bars means no brief" rule stops being welded into the
   assembler and becomes `PricesSection.required = True`. A bond watchlist that
   swaps in `YieldSection(required=True)` inherits the same "don't analyse
   something we have no data on" guarantee through its own section.

2. **Prompt and model per watchlist.** `analyst_template_for(ctx, inst)`;
   `render_prompt` gains a `ChoiceLoader` search path so a watchlist can ship
   its own templates; extra Jinja variables (`market`, `asset_class`, `sector`,
   `tags`, `watchlist`, plus `prompt_vars()`).

   Two details that keep this safe: Jinja's `StrictUndefined` fires only on
   *missing* variables, never extra ones, so `analyst_v1.j2` renders
   byte-identically and **existing `LLMClient` cache entries stay valid** —
   which matters for Phase 3's backtest replay economics. And `prompt_version`
   is still the template filename, so a watchlist template must use a distinct
   name (`analyst_mining_v1.j2`), which keeps the scorecard's per-prompt
   breakdown meaningful for free.

   Model choice composes with `docs/MODEL_SELECTION_PLAN.md` by widening its
   resolver one parameter — `[watchlists.<w>].model` →
   `[plugins.<p>].model` → `[llm.routing].<task>` → loud `KeyError`.

*Tests:* `test_brief.py` unedited is the gate; new
`tests/test_brief_sections.py` and `tests/test_prompt_seam.py`, the latter
asserting the rendered prompt *and its cache key* are byte-identical for an
instrument in no watchlist.

---

### Phase 4 — Your watchlist gets its own limits

*What you get:* `max_pct = 35` on a watchlist actually stops the app putting
more than a third of your money in it, and the existing sector limit stops being
dead weight.

1. **Limits generalise.** The single sector gate in `rigger/paper/risk.py`
   becomes a loop over buckets keyed by watchlist, sector, asset class, market
   or tag — **gate order preserved exactly**. `AppConfig.risk` becomes a typed
   `RiskConfig` so the new tables parse. `max_sector_pct = 25` keeps working
   through a shim, and `sector_exposure_pct` survives as a deprecated keyword
   seeding the sector bucket, which is what keeps `test_risk.py` unedited.

   This is also what un-breaks the sector limit: today
   `if instrument.sector and ...` means every ASX holding skips it, because
   their sector is always `None`. Watchlists fill sector in, and a watchlist
   bucket is never null.

2. **Exposure measured properly.** `services.execute.exposures()` currently
   measures your holdings at what you paid (`pos.qty * pos.avg_price`), so the
   limit drifts from reality as prices move. Fix to use the latest price.

3. **Brokerage stops guessing.** `fee_bps`/`fee_min` move onto the market
   plugin; `MARKET_FEES` and the dead `ASXMarket.fee()` are deleted; an unknown
   market now **raises** rather than silently trading you free of charge.

*Tests:* new `tests/test_risk_buckets.py` and `tests/test_market_fees.py`; one
new `test_services.py` case asserting exposure moves with price.
`test_risk.py` and `test_portfolio.py` unedited.

---

## Bonds, honestly

A bond watchlist works after Phase 4 — it gets its own instruments, its own
brief, its own prompt, its own limits, and it stays out of Momentum's ranking.
But be straight about what it does *not* do, in the app's own docs as well as
here:

- **No interest accrual or coupons.** A bond is valued at price × units, so its
  return is understated by the interest you'd actually receive. The mitigation
  is a hook, not an implementation: `rigger/core/policy.py` with an
  `AssetClassPolicy` protocol (`lot_size`, `position_value`,
  `price_is_meaningful`) and a registry with only the shares implementation
  filled in. Everything else falls back to it, so interest accrual can land
  later behind a seam that already exists.
- **No minimum parcel sizes.** You'll get fractional units.
- **Price data is your problem.** No free source ships Australian government
  bond prices the way yfinance ships share prices; a bond watchlist needs its
  own data plugin. The cheap route needs no schema change — store price bars
  with `volume = 0.0` and put yields into the fundamentals table as metric rows,
  which the brief already renders generically.

## Deferred

- **Writing `InstrumentTable`.** The table exists and nothing has ever written
  to it. Until it does, brokerage keeps resolving from the instrument id prefix,
  and Phase 3's scorecard sector grouping keeps reading `"unknown"` for every
  row — blocking that workstream. Bounded and reversible.
- Deleting the hardcoded `if name == "yfinance":` FX special case in the ingest
  loop by making FX a built-in watchlist.
- Trading calendars (`is_open`/`next_open` still have no caller anywhere),
  short selling, multiple lots per holding, and moving `[universe]` out of
  `config.toml` — the compatibility shim is permanent.

**What must not be deferred:** `asset_class` in Phase 1. Everything downstream
decides things from it, and retrofitting it after strategies, limits, briefs and
prompts have been written against loose tags is the one part of this that gets
dramatically more expensive with delay.

## Two sharp edges to document

- **A holding can hit two limits.** A mining share has both a sector and a
  watchlist, so the tighter of the two applies. That's the right behaviour, but
  undocumented it makes a rejection baffling — the skip message must name which
  limit stopped it.
- **`EventKind` is defined in three places** (`models.py:48-57`,
  `extract_v1.j2:20`, `extract.py:32`). A bond or commodity watchlist can't add
  an event kind like `rate_decision` without editing all three plus a template,
  which contradicts "a watchlist is one file". First thing that will bite a
  non-share watchlist; worth its own small step sourcing the list from one
  exported tuple.

## Verification

1. `pytest` green at every phase; the 16 existing modules unedited except
   `test_services.py` in Phase 1. `mypy --strict rigger/core rigger/llm` and
   `ruff check .` clean.
2. `rig watchlist add mining --market asx --tickers BHP,RIO,FMG` then
   `rig watchlist list` — six holdings, no Python written, `config.toml` valid.
3. `rig watchlist add mining --market asz` — fails with a message naming the
   known markets.
4. `rig ingest` — yfinance fetches the mining names with `.AX` suffixes.
5. `rig analyse`, then
   `sqlite3 data/rigger.db "select instrument_id, prompt_version from signal"` —
   mining names carry `analyst_mining_v1`, everything else `analyst_v1`.
6. **The regression that matters:** `rig analyse --strategy momentum` with a
   bond watchlist enabled produces equity signals byte-identical to a run with
   it disabled.
7. `rig execute` then `rig paper status` — a mining buy is refused once
   `max_pct` is reached, and the skip line says so by name.
