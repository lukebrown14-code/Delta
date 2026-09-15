# Watch Targets Plan

> The object a user follows: not just a share, but a company, sector, industry,
> market, or theme.

## Goal

`rig target add solar-panels --kind industry --tickers ENPH,FSLR --tags solar`
and the TUI equivalent. A target names a thing you care about and, optionally,
the tickers that trade it and the tags that classify it.

## Model

```python
class WatchTarget:
    id: str
    kind: Literal["company", "sector", "industry", "market", "theme"]
    name: str
    markets: tuple[str, ...]
    tickers: tuple[str, ...]
    tags: frozenset[str]
    notes: str = ""
```

A company target has one ticker and one market. A sector/industry/theme target
may have many tickers across markets. A market target may have no tickers at all
(it follows an index or region).

## Changes

- Extend the existing `WatchlistPlugin`/`TickerWatchlist` into a `WatchTarget`
  model and a `TargetRegistry` backed by `[targets.<name>]` in `config.toml`.
- Keep the `[universe]` shim: existing ticker-only config keeps working and is
  treated as an implicit set of company targets.
- `Scope` stays the filter used by evidence collection, but its `watchlists`
  axis becomes `targets`.

## Surface

`rig target add/remove/list/show`, and the TUI watchlists panel rebranded to
targets.

## Tests

`tests/test_watch_targets.py`: each kind round-trips; a theme with tickers; a
market target with none; `[universe]` shim produces byte-identical targets to
today's instruments.
