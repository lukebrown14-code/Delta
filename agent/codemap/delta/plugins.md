# delta/plugins

Discovered plugins: markets, data sources, watch-target kinds. Registered as entry-points in `pyproject.toml` (`delta.plugins` = markets+data, `delta.targets` = target kinds). Depends on `delta/core` (plugin protocols, models).

## Files

- `markets/us.py` — `USMarket(MarketPlugin)`
- `markets/asx.py` — `ASXMarket(MarketPlugin)`
- `data/yfinance.py` — `YFinanceData(DataPlugin)`: price bars + fundamentals via yfinance; `yf_symbol`; base class `YFinanceSymbols`
- `data/yfinance_calendar.py` — `YFinanceCalendar(YFinanceSymbols)`: earnings/dividend dates as events
- `data/rss.py` — `RSSData(DataPlugin)`: feed ingestion; `parse_feed`, `strip_html`, `news_id`
- `data/sec_edgar.py` — `SECEdgar(DataPlugin)`: US filings + company-facts fundamentals (needs contact email in User-Agent)
- `data/asx_announcements.py` — `ASXAnnouncements(DataPlugin)`: ASX disclosure documents; `announcement_id`
- `targets/tickers.py` — watch-target kinds: `CompanyTarget`, `SectorTarget`, `IndustryTarget`, `ThemeTarget`, `MarketTarget`, `LegacyTickersTarget` (base `TickerTarget`)

## To change Y, edit Z

- Add a data source → new `data/<name>.py` subclassing `DataPlugin` + entry-point in `pyproject.toml` + optional `[plugins.<name>]` defaults in `config.toml` + test file `tests/test_<name>.py`
- Add a market → new `markets/<name>.py` + entry-point
- Add a target kind → `targets/tickers.py` + entry-point in `delta.targets` group; kind literal in `delta/targets.py`
- Tune a plugin's settings schema → `DataProviderSpec`/`DataProviderField` on the plugin class

## Gotchas

- Discovery is entry-point based: forgetting `pyproject.toml` = plugin silently absent.
- No provider-specific conditionals outside this package — screens/services stay generic.
- Tests mock HTTP with `respx`; never hit live yfinance/SEC/ASX/RSS endpoints.
