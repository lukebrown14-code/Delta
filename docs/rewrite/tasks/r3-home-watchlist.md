# R3.2 — Home + Watchlist

- **Branch:** `rewrite/r3-home-watchlist`
- **Runs:** parallel, after R3.1
- **Findings:** `docs/rewrite/findings/rust-screens.md`

## Owns

`crates/delta-tui/src/screens/{home,watchlist}.rs`, `crates/delta-tui/src/{chart,desk,workers}.rs`
(quote parts), `tests/golden_scenarios/{home,watchlist}.py`.

## Python sources

`delta/tui/screens/home.py`, `targets.py` (Watchlist: `Targets`, `TargetAddModal`,
`MetricHelpModal`), `components.py` (`QuoteFeedMixin`), `delta/quotes.py`,
`delta/asset_metrics.py`. Python tests: `test_home.py`, `test_targets_inspector.py`,
`test_watch_targets.py`, `test_quotes.py`.

## Work

- Rebuild both screens data-driven; retire `draw_home*` and `draw_watchlist*`.
- Home: agenda, first-run checklist (`setup_checks`), review queue, due
  decisions, `↑↓`/`enter`.
- Watchlist bindings as Python: `enter` inspect, `r`/`R` range, `i` metric
  help, `a` add (symbol search: offline/pending/online states), `d` remove,
  `/` filter, `space` group, `←`/`→` member, `ctrl+t` more, `esc`.
- Chart scrub and benchmark series (widgets #3, K9).
- Quotes stream by default, as in Python; `DELTA_QUOTES` removed (findings #11).

## Golden scenarios

Home: populated, empty, first-run. Watchlist: populated list, inspector,
range-cycled, scrubbed, add-modal at each search state, metric-help, filter.

## Ticket #34 acceptance

- [x] Watchlist add/remove persists through `delta-services` and reloads the selected universe.
- [x] Chart scrub moves over the loaded series and paints its date, value and cursor.
- [x] Non-equity profiles use asset-aware provider metrics and correct bond yield units.
- [x] App/service tests, existing Home/Watchlist goldens, fmt, clippy and Rust workspace tests pass.

The broader R3.2 screen scenarios and performance gates below remain for the
full cutover validation (#35); the Python oracle has no scrub state yet.

## Done

- [ ] All scenarios Tier A green at 3 sizes; existing 12 Home/Watchlist goldens still green.
- [ ] Bindings match Python.
- [ ] Scrub at 60 fps with frame log on (release build).
