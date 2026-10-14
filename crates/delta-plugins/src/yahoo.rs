//! Yahoo Finance client: bars (chart), quotes, quoteSummary with the
//! cookie/crumb handshake, symbol search, and the live quote stream state
//! machine (R1b: the rewrite writes its own Yahoo client — there is no
//! Rust `yfinance`).
//!
//! Test seams: `chart_base_url`, `quote_base_url` and the quotes loop's
//! transport trait replace the production endpoints under wiremock.

use std::collections::BTreeMap;

use chrono::{NaiveDateTime, TimeZone, Utc};
use delta_core::models::{Bar, Instrument};
use serde_json::Value;

use crate::plugin::PluginError;

pub const CHART_URL: &str = "https://query1.finance.yahoo.com/v8/finance/chart/{symbol}";
pub const QUOTE_URL: &str = "https://query1.finance.yahoo.com/v7/finance/quote?symbols={symbols}";
pub const QUOTE_SUMMARY_URL: &str =
    "https://query1.finance.yahoo.com/v10/finance/quoteSummary/{symbol}?modules={modules}";
pub const SEARCH_URL: &str =
    "https://query1.finance.yahoo.com/v1/finance/search?q={query}&quotesCount={max}";
pub const COOKIE_URL: &str = "https://fc.yahoo.com";
pub const CRUMB_URL: &str = "https://query1.finance.yahoo.com/v1/test/getcrumb";

pub const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36";

/// Default exchange suffixes (port of `DEFAULT_SUFFIXES`).
pub fn default_suffixes() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("us".to_string(), String::new()),
        ("asx".to_string(), ".AX".to_string()),
    ])
}

/// Market suffixes for one configured source: the defaults, then
/// `[markets]` profiles, then any `[plugins.<source>.suffixes]` overrides.
pub fn configured_suffixes(
    cfg: &delta_core::config::AppConfig,
    source: &str,
) -> BTreeMap<String, String> {
    let mut suffixes = default_suffixes();
    for (market, profile) in &cfg.markets {
        suffixes.insert(market.clone(), profile.yahoo_suffix.clone());
    }
    if let Some(overrides) = cfg
        .plugins
        .get(source)
        .and_then(|table| table.get("suffixes"))
        .and_then(Value::as_object)
    {
        for (market, suffix) in overrides {
            if let Some(suffix) = suffix.as_str() {
                suffixes.insert(market.clone(), suffix.into());
            }
        }
    }
    suffixes
}

/// yfinance ticker for an instrument: BHP on asx -> BHP.AX. One source of truth.
pub fn yf_symbol(inst: &Instrument, suffixes: &BTreeMap<String, String>) -> String {
    format!(
        "{}{}",
        inst.symbol.to_uppercase(),
        suffixes.get(&inst.market).cloned().unwrap_or_default()
    )
}

/// Map Yahoo quote metadata to Delta's supported asset classes
/// (`classify_yahoo_asset`).
pub fn classify_yahoo_asset(symbol: &str, quote_type: &str) -> &'static str {
    let value = quote_type.to_lowercase();
    let upper = symbol.to_uppercase();
    if value.contains("crypto") || upper.ends_with("-USD") {
        return "crypto";
    }
    if value.contains("etf") {
        return "etf";
    }
    if value.contains("bond") || value.contains("fixed income") {
        return "bond";
    }
    if value.contains("currency") || value.contains("forex") {
        return "fx";
    }
    if value.contains("commodity") || value.contains("future") {
        return "commodity";
    }
    if value.contains("cash") {
        return "cash";
    }
    "equity"
}

/// Convert a Yahoo provider symbol to the symbol stored in a target spec
/// (`canonical_symbol`).
pub fn canonical_symbol(symbol: &str, market: &str, suffixes: &BTreeMap<String, String>) -> String {
    let symbol = symbol.trim().to_uppercase();
    let suffix = suffixes
        .get(&market.to_lowercase())
        .cloned()
        .unwrap_or_default();
    if !suffix.is_empty() && symbol.ends_with(&suffix.to_uppercase()) {
        return symbol[..symbol.len() - suffix.len()].to_string();
    }
    symbol
}

/// A Yahoo symbol-search hit (`SearchResult`).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub symbol: String,
    pub name: String,
    pub market: String,
    pub currency: String,
    pub exchange: String,
    pub asset_class: String,
}

fn market_for_exchange(exchange: &str) -> &str {
    let code = exchange.to_uppercase();
    if code.contains("ASX") {
        "asx"
    } else {
        // Unknown listings stay usable as US results for compatibility with
        // Yahoo's incomplete exchange metadata.
        "us"
    }
}

/// Map search entries to `SearchResult`s (`yahoo_search`'s body).
pub fn parse_search(payload: &Value, max_results: usize) -> Vec<SearchResult> {
    let mut results = Vec::new();
    for item in payload["quotes"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or(&[])
    {
        let symbol = item["symbol"].as_str().unwrap_or("").trim().to_uppercase();
        if symbol.is_empty() {
            continue;
        }
        let name = item["longname"]
            .as_str()
            .or_else(|| item["shortname"].as_str())
            .unwrap_or(&symbol)
            .trim()
            .to_string();
        let exchange = item["exchange"]
            .as_str()
            .or_else(|| item["fullExchangeName"].as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let market = market_for_exchange(&exchange).to_string();
        let currency = item["currency"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| {
                if market == "asx" {
                    "AUD".to_string()
                } else {
                    "USD".to_string()
                }
            });
        let quote_type = item["quoteType"]
            .as_str()
            .or_else(|| item["typeDisp"].as_str())
            .unwrap_or("")
            .to_lowercase();
        let asset_class = classify_yahoo_asset(&symbol, &quote_type).to_string();
        results.push(SearchResult {
            symbol,
            name,
            market,
            currency,
            exchange,
            asset_class,
        });
        if results.len() >= max_results {
            break;
        }
    }
    results
}

/// Bars from a v8 chart payload (`_history_bars`' conversion): NaN rows are
/// skipped, timestamps are bar-close UTC.
pub fn chart_bars(inst: &Instrument, payload: &Value) -> Vec<Bar> {
    let result = &payload["chart"]["result"][0];
    let stamps = result["timestamp"].as_array().cloned().unwrap_or_default();
    let quote = &result["indicators"]["quote"][0];
    let series = |key: &str| -> Vec<Option<f64>> {
        quote[key]
            .as_array()
            .map(|a| a.iter().map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    let (opens, highs, lows, closes, volumes) = (
        series("open"),
        series("high"),
        series("low"),
        series("close"),
        series("volume"),
    );
    let mut bars = Vec::new();
    for (i, stamp) in stamps.iter().enumerate() {
        let Some(ts) = stamp.as_f64() else { continue };
        let pick = |v: &[Option<f64>]| v.get(i).copied().flatten();
        let (Some(open), Some(high), Some(low), Some(close)) =
            (pick(&opens), pick(&highs), pick(&lows), pick(&closes))
        else {
            // FX and thin symbols return rows with NaN/missing prices; the DB
            // would store NULLs and violate the bar NOT NULL columns.
            continue;
        };
        if ![open, high, low, close].iter().all(|v| v.is_finite()) {
            continue;
        }
        bars.push(Bar {
            instrument_id: inst.id.clone(),
            ts: Utc
                .timestamp_opt(ts as i64, 0)
                .single()
                .map(|d| d.naive_utc())
                .unwrap_or_default(),
            open,
            high,
            low,
            close,
            volume: pick(&volumes).unwrap_or(0.0),
            source: "yfinance".to_string(),
        });
    }
    bars
}

/// An ephemeral quote (`Quote`).
#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub price: f64,
    pub currency: String,
    pub change_pct: Option<f64>,
    pub timestamp: NaiveDateTime,
    pub received_at: NaiveDateTime,
}

/// Parse one live-quote message; None for unusable values (`parse_quote`).
pub fn parse_quote(message: &Value, currency: &str, received_at: NaiveDateTime) -> Option<Quote> {
    let price = message["price"].as_f64()?;
    let secs = message["time"].as_f64()?;
    let timestamp = Utc
        .timestamp_opt(secs as i64 / 1000, 0)
        .single()?
        .naive_utc();
    let mut change = message.get("change_percent").and_then(Value::as_f64);
    if !price.is_finite() || price <= 0.0 {
        return None;
    }
    if let Some(c) = change {
        if !c.is_finite() {
            change = None;
        }
    }
    Some(Quote {
        price,
        currency: message["currency"]
            .as_str()
            .map(str::to_string)
            .unwrap_or_else(|| currency.to_string()),
        change_pct: change,
        timestamp,
        received_at,
    })
}

/// The live quote stream state machine (`YahooQuotes`): per-symbol latest
/// quote, monotonic timestamps, reconnect with exponential backoff capped at
/// 30s. The transport is a trait so tests drive it without a socket.
pub struct YahooQuotes {
    /// yf symbol -> instrument id.
    pub symbols: BTreeMap<String, String>,
    /// Currency per instrument id.
    pub currencies: BTreeMap<String, String>,
    pub quotes: BTreeMap<String, Quote>,
    pub state: String,
    now: Box<dyn Fn() -> NaiveDateTime + Send>,
}

impl YahooQuotes {
    pub fn new(instruments: &[Instrument], suffixes: &BTreeMap<String, String>) -> Self {
        let mut symbols = BTreeMap::new();
        let mut currencies = BTreeMap::new();
        for inst in instruments {
            symbols.insert(yf_symbol(inst, suffixes), inst.id.clone());
            currencies.insert(inst.id.clone(), inst.currency.clone());
        }
        Self {
            symbols,
            currencies,
            quotes: BTreeMap::new(),
            state: String::new(),
            now: Box::new(|| Utc::now().naive_utc()),
        }
    }

    #[doc(hidden)]
    pub fn with_clock(now: Box<dyn Fn() -> NaiveDateTime + Send>) -> Self {
        Self {
            now,
            ..Self::new(&[], &default_suffixes())
        }
    }

    /// Ingest one decoded message (`receive`).
    pub fn receive(&mut self, message: &Value) {
        let Some(id) = message
            .get("id")
            .and_then(Value::as_str)
            .and_then(|id| self.symbols.get(id).cloned())
        else {
            return;
        };
        let currency = self.currencies.get(&id).cloned().unwrap_or_default();
        let Some(quote) = parse_quote(message, &currency, (self.now)()) else {
            return;
        };
        match self.quotes.get(&id) {
            // A stale tick must not overwrite a newer one.
            Some(previous) if quote.timestamp < previous.timestamp => {}
            _ => {
                self.quotes.insert(id, quote);
            }
        }
    }

    /// Exponential backoff delay for the next reconnect attempt
    /// (`delay` in Python's `run`): 1s first, doubling, capped at 30s.
    pub fn reconnect_delay(delay: u64) -> u64 {
        (delay * 2).min(30)
    }

    pub fn initial_delay() -> u64 {
        1
    }
}

/// One connection to Yahoo: chart bars for a symbol since a date, batched
/// quotes, a quoteSummary module set (via the cookie/crumb handshake), and
/// symbol search (`YFinanceData` + `yahoo_search`, owns the HTTP client).
pub struct YahooClient {
    pub http: reqwest::Client,
    /// Test seams: empty string means the production URL.
    pub chart_base_url: String,
    pub quote_base_url: String,
    pub suffixes: BTreeMap<String, String>,
}

impl Default for YahooClient {
    fn default() -> Self {
        Self {
            http: reqwest::Client::builder()
                .user_agent(USER_AGENT)
                .cookie_store(true)
                .build()
                .expect("static client config"),
            chart_base_url: String::new(),
            quote_base_url: String::new(),
            suffixes: default_suffixes(),
        }
    }
}

impl YahooClient {
    /// The base for quote-family endpoints (production or the test seam).
    fn base(&self) -> String {
        if self.quote_base_url.is_empty() {
            "https://query1.finance.yahoo.com".to_string()
        } else {
            self.quote_base_url.trim_end_matches('/').to_string()
        }
    }

    /// The cookie/crumb handshake quoteSummary requires (`crumb` flow:
    /// fc.yahoo.com sets the cookie, getcrumb returns it).
    pub async fn crumb(&self) -> Result<String, PluginError> {
        let cookie_url = if self.quote_base_url.is_empty() {
            COOKIE_URL.to_string()
        } else {
            format!("{}/cookie", self.base())
        };
        // Any response is fine; the cookie jar is what matters.
        let _ = self.http.get(&cookie_url).send().await;
        let crumb_url = if self.quote_base_url.is_empty() {
            CRUMB_URL.to_string()
        } else {
            format!("{}/getcrumb", self.base())
        };
        let crumb = self
            .http
            .get(&crumb_url)
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;
        Ok(crumb.trim().to_string())
    }

    /// Daily bars for one symbol since `since` (v8 chart endpoint).
    pub async fn chart(
        &self,
        inst: &Instrument,
        since: NaiveDateTime,
    ) -> Result<Vec<Bar>, PluginError> {
        let symbol = yf_symbol(inst, &self.suffixes);
        let url = if self.chart_base_url.is_empty() {
            CHART_URL.replace("{symbol}", &symbol)
        } else {
            format!(
                "{}/v8/finance/chart/{symbol}",
                self.chart_base_url.trim_end_matches('/')
            )
        };
        let period1 = since.and_utc().timestamp();
        let payload = self
            .http
            .get(&url)
            .query(&[
                ("period1", period1.to_string()),
                ("period2", Utc::now().timestamp().to_string()),
                ("interval", "1d".to_string()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(chart_bars(inst, &payload))
    }

    /// Batched regular-market quotes (`quote` endpoint).
    pub async fn quotes(&self, symbols: &[String]) -> Result<Value, PluginError> {
        let url = format!("{}/v7/finance/quote", self.base());
        let crumb = self.crumb().await?;
        self.http
            .get(&url)
            .query(&[("symbols", symbols.join(",")), ("crumb", crumb)])
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
            .map_err(Into::into)
    }

    /// quoteSummary modules for one symbol, via cookie/crumb.
    pub async fn quote_summary(&self, symbol: &str, modules: &str) -> Result<Value, PluginError> {
        let crumb = self.crumb().await?;
        let url = format!("{}/v10/finance/quoteSummary/{symbol}", self.base());
        self.http
            .get(&url)
            .query(&[("crumb", crumb), ("modules", modules.to_string())])
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await
            .map_err(Into::into)
    }

    /// Symbol search (`yahoo_search`'s HTTP half).
    pub async fn search(
        &self,
        query: &str,
        max_results: usize,
    ) -> Result<Vec<SearchResult>, PluginError> {
        let url = format!("{}/v1/finance/search", self.base());
        let payload = self
            .http
            .get(&url)
            .query(&[
                ("q", query),
                ("quotesCount", max_results.to_string().as_str()),
            ])
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        Ok(parse_search(&payload, max_results))
    }
}

/// Daily bars from Yahoo's chart endpoint, as a `DataPlugin`
/// (port of `YFinanceData`; the blocking yfinance download becomes a direct
/// HTTP call to the same v8 chart endpoint).
#[derive(Default)]
pub struct YfinanceBars {
    pub client: std::sync::Arc<tokio::sync::Mutex<Option<YahooClient>>>,
    pub suffixes: BTreeMap<String, String>,
}

impl YfinanceBars {
    pub fn configure_suffixes(&mut self, cfg: &Value) {
        if let Some(table) = cfg.get("suffixes").and_then(Value::as_object) {
            for (k, v) in table {
                if let Some(suffix) = v.as_str() {
                    self.suffixes.insert(k.clone(), suffix.to_string());
                }
            }
        }
    }
}

#[async_trait::async_trait]
impl crate::plugin::DataPlugin for YfinanceBars {
    fn name(&self) -> &'static str {
        "yfinance"
    }

    fn configure(&mut self, cfg: &Value) {
        self.configure_suffixes(cfg);
    }

    async fn fetch(
        &self,
        instruments: &[Instrument],
        since: NaiveDateTime,
    ) -> Result<Vec<delta_core::db::StoreItem>, PluginError> {
        let client = YahooClient {
            suffixes: self.suffixes.clone(),
            ..YahooClient::default()
        };
        let mut bars = Vec::new();
        for inst in instruments {
            // One failed symbol must not abort the whole ingest.
            if let Ok(rows) = client.chart(inst, since).await {
                bars.extend(rows.into_iter().map(delta_core::db::StoreItem::Bar));
            }
        }
        Ok(bars)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inst(market: &str, symbol: &str) -> Instrument {
        Instrument {
            id: format!("{}:{symbol}", market.to_uppercase()),
            market: market.to_string(),
            symbol: symbol.to_string(),
            name: None,
            currency: if market == "asx" { "AUD" } else { "USD" }.to_string(),
            sector: None,
            asset_class: delta_core::models::AssetClass::Equity,
            watchlists: Vec::new(),
            tags: Default::default(),
            industry: None,
            meta: Default::default(),
        }
    }

    #[test]
    fn yf_symbol_applies_market_suffix() {
        let suffixes = default_suffixes();
        assert_eq!(yf_symbol(&inst("us", "AAPL"), &suffixes), "AAPL");
        assert_eq!(yf_symbol(&inst("asx", "BHP"), &suffixes), "BHP.AX");
        // Config-backed suffix table wins.
        let mut custom = default_suffixes();
        custom.insert("us".to_string(), ".TO".to_string());
        assert_eq!(yf_symbol(&inst("us", "RY"), &custom), "RY.TO");
    }

    #[test]
    fn asset_classification_matches_python() {
        assert_eq!(classify_yahoo_asset("BTC-USD", "CRYPTOCURRENCY"), "crypto");
        assert_eq!(classify_yahoo_asset("VDHG", "ETF"), "etf");
        assert_eq!(classify_yahoo_asset("TLT", ""), "equity");
        assert_eq!(classify_yahoo_asset("AUDUSD=X", "CURRENCY"), "fx");
        assert_eq!(classify_yahoo_asset("GC=F", "FUTURE"), "commodity");
    }

    #[test]
    fn canonical_symbol_strips_suffix() {
        let suffixes = default_suffixes();
        assert_eq!(canonical_symbol("bhp.ax", "asx", &suffixes), "BHP");
        assert_eq!(canonical_symbol("AAPL", "us", &suffixes), "AAPL");
    }

    #[test]
    fn search_mapping() {
        let payload: Value = serde_json::json!({"quotes": [
            {"symbol": "bhp.ax", "longname": "BHP Group", "exchange": "ASX",
             "currency": "AUD", "quoteType": "EQUITY"},
            {"symbol": "", "longname": "no symbol"},
            {"symbol": "AAPL", "shortname": "Apple", "exchange": "NMS", "quoteType": "EQUITY"}
        ]});
        let results = parse_search(&payload, 8);
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].symbol, "BHP.AX");
        assert_eq!(results[0].market, "asx");
        assert_eq!(results[0].asset_class, "equity");
        assert_eq!(results[1].market, "us");
    }

    #[test]
    fn chart_bars_skip_null_rows() {
        let inst = inst("us", "AAPL");
        let payload: Value = serde_json::json!({
            "chart": {"result": [{
                "timestamp": [1700000000, 1700086400, 1700172800],
                "indicators": {"quote": [{
                    "open": [1.0, null, 3.0],
                    "high": [1.5, 2.5, 3.5],
                    "low": [0.5, 1.5, 2.5],
                    "close": [1.2, 2.2, 3.2],
                    "volume": [100.0, 200.0, 300.0]
                }]}
            }]}
        });
        let bars = chart_bars(&inst, &payload);
        assert_eq!(bars.len(), 2);
        assert_eq!(bars[0].instrument_id, "US:AAPL");
        assert_eq!(bars[0].close, 1.2);
        assert_eq!(bars[1].open, 3.0);
    }

    #[test]
    fn quote_parsing_rejects_bad_values() {
        let now = NaiveDateTime::default();
        let good = serde_json::json!({"id": "AAPL", "price": 123.4, "time": 1700000000000i64,
                                     "currency": "USD", "change_percent": 0.5});
        let q = parse_quote(&good, "USD", now).unwrap();
        assert_eq!(q.price, 123.4);
        assert_eq!(q.change_pct, Some(0.5));
        // Non-positive price, missing fields and NaN change are unusable.
        assert!(parse_quote(&serde_json::json!({"price": -1.0, "time": 0}), "USD", now).is_none());
        assert!(parse_quote(&serde_json::json!({"price": 1.0}), "USD", now).is_none());
        assert!(parse_quote(
            &serde_json::json!({"price": 1.0, "time": 0, "change_percent": serde_json::Value::Null}),
            "USD",
            now
        )
        .unwrap()
        .change_pct
        .is_none());
    }

    #[test]
    fn quotes_ignore_stale_ticks() {
        let mut yq = YahooQuotes::new(&[inst("us", "AAPL")], &default_suffixes());
        yq.receive(&serde_json::json!({"id": "AAPL", "price": 100.0, "time": 2000}));
        yq.receive(&serde_json::json!({"id": "AAPL", "price": 90.0, "time": 1000}));
        assert_eq!(yq.quotes["US:AAPL"].price, 100.0);
        yq.receive(&serde_json::json!({"id": "AAPL", "price": 110.0, "time": 3000}));
        assert_eq!(yq.quotes["US:AAPL"].price, 110.0);
        // Unknown symbol: ignored.
        yq.receive(&serde_json::json!({"id": "ZZZ", "price": 1.0, "time": 4000}));
        assert_eq!(yq.quotes.len(), 1);
    }

    #[test]
    fn reconnect_backoff_caps_at_thirty() {
        assert_eq!(YahooQuotes::initial_delay(), 1);
        assert_eq!(YahooQuotes::reconnect_delay(1), 2);
        assert_eq!(YahooQuotes::reconnect_delay(16), 30);
        assert_eq!(YahooQuotes::reconnect_delay(30), 30);
    }
}

/// Decode one Yahoo streamer text frame into the JSON shape
/// [`YahooQuotes::receive`] and [`parse_quote`] expect: the frame is
/// base64-encoded protobuf (`PricingData`: `id` = field 1 (string),
/// `price` = field 2 (float32), `time` = field 3 (int64 ms),
/// `changePercent` = field 6 (float32)). Non-base64 frames (heartbeats)
/// return `None`.
pub fn decode_stream_frame(text: &str) -> Option<serde_json::Value> {
    use base64::Engine;
    let raw = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .ok()?;
    let mut out = serde_json::Map::new();
    let mut rest = raw.as_slice();
    while !rest.is_empty() {
        // Tag varint: (field << 3) | wire_type.
        let (tag, consumed) = read_varint(rest)?;
        rest = &rest[consumed..];
        let field = tag >> 3;
        let wire = tag & 0b111;
        match (field, wire) {
            (1, 2) => {
                let (len, c) = read_varint(rest)?;
                rest = &rest[c..];
                let len = usize::try_from(len).ok()?;
                if rest.len() < len {
                    return None;
                }
                out.insert(
                    "id".to_string(),
                    serde_json::Value::String(String::from_utf8_lossy(&rest[..len]).to_string()),
                );
                rest = &rest[len..];
            }
            (2, 5) => {
                if rest.len() < 4 {
                    return None;
                }
                let v = f32::from_le_bytes(rest[..4].try_into().ok()?);
                out.insert("price".to_string(), serde_json::Value::from(v as f64));
                rest = &rest[4..];
            }
            (3, 0) => {
                let (v, c) = read_varint(rest)?;
                rest = &rest[c..];
                // yfinance's `time` is already ms.
                out.insert("time".to_string(), serde_json::Value::from(v as f64));
            }
            (6, 5) => {
                if rest.len() < 4 {
                    return None;
                }
                let v = f32::from_le_bytes(rest[..4].try_into().ok()?);
                out.insert(
                    "change_percent".to_string(),
                    serde_json::Value::from(v as f64),
                );
                rest = &rest[4..];
            }
            (_, 0) => {
                let (_, c) = read_varint(rest)?;
                rest = &rest[c..];
            }
            (_, 1) => {
                if rest.len() < 8 {
                    return None;
                }
                rest = &rest[8..];
            }
            (_, 2) => {
                let (len, c) = read_varint(rest)?;
                rest = &rest[c..];
                let len = usize::try_from(len).ok()?;
                if rest.len() < len {
                    return None;
                }
                rest = &rest[len..];
            }
            (_, 5) => {
                if rest.len() < 4 {
                    return None;
                }
                rest = &rest[4..];
            }
            _ => return None,
        }
    }
    if out.contains_key("id") {
        Some(serde_json::Value::Object(out))
    } else {
        None
    }
}

/// A LEB128 varint; returns `(value, bytes_consumed)`.
fn read_varint(bytes: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    let mut shift = 0;
    for (i, b) in bytes.iter().enumerate() {
        value |= u64::from(b & 0x7f) << shift;
        if b & 0x80 == 0 {
            return Some((value, i + 1));
        }
        shift += 7;
        if shift > 63 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod stream_tests {
    use super::*;
    use base64::Engine;

    fn varint(v: u64) -> Vec<u8> {
        let mut out = Vec::new();
        let mut v = v;
        loop {
            let b = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                out.push(b);
                break;
            }
            out.push(b | 0x80);
        }
        out
    }

    fn tagged(field: u64, wire: u64) -> Vec<u8> {
        varint((field << 3) | wire)
    }

    #[test]
    fn decodes_a_pricing_data_frame() {
        let mut body = Vec::new();
        // id = "AAPL"
        body.extend(tagged(1, 2));
        body.push(4);
        body.extend(b"AAPL");
        // price = 250.5 (float32)
        body.extend(tagged(2, 5));
        body.extend(250.5f32.to_le_bytes());
        // time = 1_700_000_000_000 ms
        body.extend(tagged(3, 0));
        body.extend(varint(1_700_000_000_000));
        // changePercent = -0.25 (float32)
        body.extend(tagged(6, 5));
        body.extend((-0.25f32).to_le_bytes());
        let frame = base64::engine::general_purpose::STANDARD.encode(&body);
        let value = decode_stream_frame(&frame).expect("decodes");
        assert_eq!(value["id"], "AAPL");
        assert!((value["price"].as_f64().unwrap() - 250.5).abs() < 1e-3);
        assert_eq!(value["time"].as_f64().unwrap(), 1_700_000_000_000.0);
        assert!((value["change_percent"].as_f64().unwrap() + 0.25).abs() < 1e-4);
    }

    #[test]
    fn heartbeats_and_garbage_return_none() {
        assert!(decode_stream_frame("Pong").is_none());
        assert!(decode_stream_frame("!!!not base64!!!").is_none());
        // base64 of a body with an id-less field layout.
        let body = tagged(2, 5)
            .into_iter()
            .chain(1.0f32.to_le_bytes())
            .collect::<Vec<u8>>();
        let frame = base64::engine::general_purpose::STANDARD.encode(&body);
        assert!(decode_stream_frame(&frame).is_none());
    }
}
