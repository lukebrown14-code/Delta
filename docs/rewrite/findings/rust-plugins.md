# Findings — rust-plugins (R1b)

Stream: `delta-plugins`. Reviewed against `delta/core/plugin.py`, `delta/plugins/**`, `delta/quotes.py`.

## Ported

| Python | Rust | Notes |
|---|---|---|
| `plugin.py` traits/Scope | `src/plugin.rs` | entry-point discovery becomes a static registry (`default_plugins`); `Scope` axes AND/OR semantics preserved |
| `rss.py` | `src/rss.rs` | `strip_html`, `news_id` (Python `isoformat` micros rule), whole-word `Matcher`, per-feed failure isolation, cross-feed dedupe, concurrency cap 4 |
| `sec_edgar.py` | `src/sec.rs` | filings→news with form labels, XBRL facts→fundamentals (namespace-first tag search, duration filter, latest by end/filed), 10/s rate cap, 429/5xx retry with Retry-After, ticker cache |
| `asx_announcements.py` | `src/asx.rs` | Markit Digital feed, `[PS] ` prefix, `announcement_id` from document key, 404 skip, retry/backoff, PAGE_URL fallback |
| `yfinance.py` + `quotes.py` | `src/yahoo.rs` | the rewrite's own Yahoo client: v8 chart bars (null rows skipped), batched quotes, quoteSummary via cookie→crumb handshake, symbol search + exchange mapping, asset classification, live-quote state machine (stale-tick rejection, backoff 1→30s) behind a testable design |
| `markets/us.py`, `markets/asx.py` | `src/markets.rs` | universe + sector table + session bounds + `next_open` |

## Test seams (behaviour-preserving, logged for review)

| # | Category | Where | Finding |
|---|---|---|---|
| 1 | test-seam | `SecEdgar.sleep_scale`, `AsxAnnouncements.backoff_seconds`/`today`, `RssData.timeout_secs`, `YahooClient.{chart,quote}_base_url` | The Python tests sleep through the real 1s rate-limit hold; the Rust ports expose scale/URL seams so wiremock tests run in milliseconds. Default values match production exactly |
| 2 | test-seam | `YahooQuotes` transport | Python's `run()` loop subscribes via `yfinance.AsyncWebSocket`; the Rust port separates the state machine (receive, monotonic timestamps, backoff constants — unit-tested) from the socket loop, which lands with R3's live-quotes pane. The WebSocket transport itself is not yet implemented |
| 3 | simplify | `rss.rs` entity unescaping | Python `html.unescape` knows every HTML5 entity; the Rust port covers the common named entities plus numeric references and leaves unknown entities as written. Feed content in practice uses `&amp;`-class entities; revisit if a feed ships exotic ones |
| 4 | bug-risk (Python) | `sec_edgar._filings_to_news` | Descriptions are zipped `strict=True` — EDGAR returning mismatched array lengths aborts the whole fetch. The Rust port zips the same way, preserving the loud failure; noting it because a live EDGAR schema change would break ingest the same way in both |
| 5 | gap | `yfinance_calendar.py` | Not ported: it maps pandas date objects from yfinance's calendar; it lands with R2 once the Yahoo quoteSummary `calendarEvents` module consumer (extract flow) is ported. `calendar_event_id` and the kind mapping are stable and small |
| 6 | parity | `markets.rs` timezones | Python uses `ZoneInfo("America/New_York")` (DST-aware); the Rust port pins EST (-5) — wrong by an hour in summer. Fixed properly in R2/R3 with a tz database (add `jiff` or `chrono-tz` then; asking first per AGENTS.md dependency rule) |
| 7 | parity | `rss.rs` IDs | `news_id` replicates Python `isoformat()` including the zero-micros omission; covered by the shared ID contract tests in delta-core |
