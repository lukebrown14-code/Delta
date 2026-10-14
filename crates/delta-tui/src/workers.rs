//! Background workers over the action bus: live Yahoo quotes and on-demand
//! ingest (`docs/RUST_REWRITE_PLAN.md` TUI pattern — nothing on the render
//! path blocks; workers report through `Action`s).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use delta_core::models::Instrument;
use delta_plugins::{decode_stream_frame, YahooClient, YahooQuotes};
use tokio::sync::mpsc;

use crate::Action;

/// Active workers for the current watched universe. Replacing this handle
/// stops obsolete subscriptions after a target is added or removed.
pub struct Workers {
    pub gather_tx: mpsc::UnboundedSender<Option<String>>,
    pub metrics_tx: mpsc::UnboundedSender<(Instrument, String)>,
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl Drop for Workers {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

/// Spawn the background workers for the current watched universe.
pub fn spawn(
    bus: mpsc::UnboundedSender<Action>,
    universe: Vec<Instrument>,
    db_path: Option<PathBuf>,
    config_path: Option<PathBuf>,
    quotes_enabled: bool,
) -> Workers {
    let mut tasks = Vec::new();
    let (metrics_tx, metrics_rx) = mpsc::unbounded_channel();
    if quotes_enabled {
        tasks.push(tokio::spawn(stream_quotes_worker(
            bus.clone(),
            universe.clone(),
        )));
        tasks.push(tokio::spawn(metrics_worker(
            bus.clone(),
            universe.clone(),
            metrics_rx,
        )));
    }
    let ids: Vec<String> = universe.iter().map(|i| i.id.clone()).collect();
    let (gather_tx, gather_rx) = mpsc::unbounded_channel::<Option<String>>();
    tasks.push(tokio::spawn(gather_worker(
        bus.clone(),
        universe,
        db_path.clone(),
        config_path,
        gather_rx,
    )));
    if let Some(db_path) = db_path {
        tasks.push(tokio::spawn(home_refresh_worker(bus, db_path, ids)));
    }
    Workers {
        gather_tx,
        metrics_tx,
        tasks,
    }
}

/// Re-run the Home analytics queries and push them to the bus, low-frequency
/// local SQL on the same cadence as the metrics worker — no new pollers.
const HOME_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

async fn home_refresh_worker(
    bus: mpsc::UnboundedSender<Action>,
    db_path: PathBuf,
    ids: Vec<String>,
) {
    loop {
        tokio::time::sleep(HOME_REFRESH_INTERVAL).await;
        if let Ok(db) = delta_core::db::Db::open(&db_path) {
            let _ = bus.send(Action::HomeRefresh(crate::desk::load_feed(&db, &ids)));
        }
    }
}

/// Stream Yahoo quotes over the websocket: connect, decode each text frame
/// with [`delta_plugins::decode_stream_frame`] into the
/// [`delta_plugins::YahooQuotes`] state machine, and report the latest
/// prices on the bus. Reconnects with the state machine's exponential
/// backoff (1s doubling, capped at 30s).
const STREAM_URL: &str = "wss://streamer.finance.yahoo.com/?version=2";

async fn stream_quotes_worker(bus: mpsc::UnboundedSender<Action>, universe: Vec<Instrument>) {
    use futures::{SinkExt, StreamExt};
    use tokio_tungstenite::tungstenite::Message;

    if universe.is_empty() {
        return;
    }
    let suffixes = delta_plugins::yahoo::default_suffixes();
    let mut quotes = YahooQuotes::new(&universe, &suffixes);
    let mut delay = YahooQuotes::initial_delay();
    loop {
        match tokio_tungstenite::connect_async(STREAM_URL).await {
            Ok((ws, _)) => {
                delay = YahooQuotes::initial_delay();
                let _ = bus.send(Action::Status("live: streaming quotes".to_string()));
                let (mut sink, mut stream) = ws.split();
                // Yahoo requires a subscription stanza per symbol.
                let sub =
                    serde_json::json!({ "subscribe": quotes.symbols.keys().collect::<Vec<_>>() });
                if let Err(e) = sink.send(Message::text(sub.to_string())).await {
                    let _ = bus.send(Action::Status(format!("stream subscribe failed: {e}")));
                }
                loop {
                    match stream.next().await {
                        Some(Ok(Message::Text(text))) => {
                            if let Some(value) = decode_stream_frame(&text) {
                                quotes.receive(&value);
                                let prices: BTreeMap<String, f64> = quotes
                                    .quotes
                                    .iter()
                                    .map(|(id, q)| (id.clone(), q.price))
                                    .collect();
                                if !prices.is_empty() {
                                    let _ = bus.send(Action::Quotes(prices));
                                }
                            }
                        }
                        Some(Ok(Message::Ping(p))) => {
                            let _ = sink.send(Message::Pong(p)).await;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(e)) => {
                            let _ = bus.send(Action::Status(format!("stream error: {e}")));
                            break;
                        }
                        None => break,
                    }
                }
            }
            Err(e) => {
                let _ = bus.send(Action::Status(format!("stream connect failed: {e}")));
            }
        }
        let wait = YahooQuotes::reconnect_delay(delay);
        delay = wait;
        tokio::time::sleep(Duration::from_secs(wait)).await;
    }
}

/// Refresh the inspector metrics (`quoteSummary`) every `METRICS_INTERVAL`.
const METRICS_INTERVAL: Duration = Duration::from_secs(60);

async fn metrics_worker(
    bus: mpsc::UnboundedSender<Action>,
    universe: Vec<Instrument>,
    mut requests: mpsc::UnboundedReceiver<(Instrument, String)>,
) {
    let client = YahooClient::default();
    let mut refresh = tokio::time::interval(METRICS_INTERVAL);
    loop {
        tokio::select! {
            _ = refresh.tick() => {
                for inst in &universe {
                    send_metrics(&bus, &client, inst, "1m").await;
                }
            }
            Some((inst, range)) = requests.recv() => {
                send_metrics(&bus, &client, &inst, &range).await;
            }
        }
    }
}

async fn send_metrics(
    bus: &mpsc::UnboundedSender<Action>,
    client: &YahooClient,
    inst: &Instrument,
    range: &str,
) {
    let data = delta_services::asset_metrics::fetch_asset_metrics(client, inst, range, None).await;
    let _ = bus.send(Action::AssetMetrics {
        range: range.to_string(),
        data: Box::new(data),
    });
}

/// Run a `!Send` async block to completion without leaving the `Send`
/// world: `build` executes on a blocking-pool thread and returns the
/// future there, so non-Send captures never cross into a spawned task;
/// the future is driven on a dedicated single-threaded runtime. Async
/// callers stay plain abortable tasks and just await the outcome.
pub async fn host_non_send<T, F, Fut>(build: F) -> Result<T, String>
where
    F: FnOnce() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = T>,
    T: Send + 'static,
{
    tokio::task::spawn_blocking(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| format!("runtime failed: {e}"))?;
        Ok(runtime.block_on(build()))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Run the configured gather pipeline over the universe on `Gather`
/// requests: ingest + Extraction + sentiment, with per-stage progress on
/// the status line.
async fn gather_worker(
    bus: mpsc::UnboundedSender<Action>,
    universe: Vec<Instrument>,
    db_path: Option<PathBuf>,
    config_path: Option<PathBuf>,
    mut requests: mpsc::UnboundedReceiver<Option<String>>,
) {
    let (Some(db_path), Some(config_path)) = (db_path, config_path) else {
        // Offline desk: acknowledge gathers so the UI can say so.
        while requests.recv().await.is_some() {
            let _ = bus.send(Action::Status(
                "offline desk: nothing to gather".to_string(),
            ));
        }
        return;
    };
    while let Some(company) = requests.recv().await {
        let _ = bus.send(Action::Status("gathering…".to_string()));
        let mut db = match delta_core::db::Db::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                let _ = bus.send(Action::Status(format!("db open failed: {e}")));
                continue;
            }
        };
        // Config is loaded per request, so Settings edits (plugins,
        // provider, routing) apply on the very next gather.
        let (_, cfg) = match delta_core::config::load_config(&config_path) {
            Ok(loaded) => loaded,
            Err(e) => {
                let _ = bus.send(Action::Status(format!("config load failed: {e}")));
                continue;
            }
        };
        let selected: Vec<Instrument> = universe
            .iter()
            .filter(|inst| company.as_ref().is_none_or(|id| &inst.id == id))
            .cloned()
            .collect();
        // Services keep their worker-spawned futures `Send` (asserted in
        // delta-services tests), so the worker awaits the pipeline directly
        // and stays a plain abortable task.
        let status_tx = bus.clone();
        let run = delta_services::gather_configured(&mut db, &cfg, &selected, |message| {
            let _ = status_tx.send(Action::Status(message.to_string()));
        })
        .await;
        match run {
            Ok(result) => {
                let _ = bus.send(Action::Gathered(result));
            }
            Err(e) => {
                let _ = bus.send(Action::Status(format!("gather failed: {e}")));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn aapl() -> Instrument {
        Instrument {
            id: "US:AAPL".to_string(),
            market: "us".to_string(),
            symbol: "AAPL".to_string(),
            name: None,
            currency: "USD".to_string(),
            sector: None,
            asset_class: delta_core::models::AssetClass::Equity,
            watchlists: Vec::new(),
            tags: Default::default(),
            industry: None,
            meta: Default::default(),
        }
    }

    /// A config whose plugins are all disabled and whose LLM points at a
    /// local port nothing listens on: with no news in the database the
    /// pipeline makes zero network calls, so the run is fully offline.
    fn all_plugins_disabled_config() -> String {
        "db_path = \"test.db\"\nreports_dir = \"reports\"\n\
         [llm]\nprovider = \"custom\"\nbase_url = \"http://127.0.0.1:9\"\n\
         model = \"test/model\"\n\
         [plugins.rss]\nenabled = false\n\
         [plugins.asx_announcements]\nenabled = false\n\
         [plugins.sec_edgar]\nenabled = false\n\
         [plugins.yfinance]\nenabled = false\n\
         [plugins.yfinance_calendar]\nenabled = false\n"
            .to_string()
    }

    /// The same offline config but with no `[llm] model` and no
    /// `[llm.routing]`: model resolution for the extract and sentiment
    /// stages fails before any client is built, so both stages degrade
    /// into warnings without a single network call.
    fn unrouted_llm_config() -> String {
        "db_path = \"test.db\"\nreports_dir = \"reports\"\n\
         [llm]\nprovider = \"custom\"\nbase_url = \"http://127.0.0.1:9\"\n\
         [plugins.rss]\nenabled = false\n\
         [plugins.asx_announcements]\nenabled = false\n\
         [plugins.sec_edgar]\nenabled = false\n\
         [plugins.yfinance]\nenabled = false\n\
         [plugins.yfinance_calendar]\nenabled = false\n"
            .to_string()
    }

    async fn next(bus: &mut mpsc::UnboundedReceiver<Action>) -> Action {
        tokio::time::timeout(Duration::from_secs(10), bus.recv())
            .await
            .expect("worker action within timeout")
            .expect("bus stays open")
    }

    #[tokio::test]
    async fn gather_worker_runs_the_configured_pipeline_and_reports_the_result() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, all_plugins_disabled_config()).unwrap();
        let (bus_tx, mut bus_rx) = mpsc::unbounded_channel();
        let (tx, rx) = mpsc::unbounded_channel::<Option<String>>();
        let worker = tokio::spawn(gather_worker(
            bus_tx,
            vec![aapl()],
            Some(dir.path().join("test.db")),
            Some(config_path),
            rx,
        ));
        tx.send(None).unwrap();
        // Progress first, then the finished action carrying the whole result.
        assert_eq!(
            next(&mut bus_rx).await,
            Action::Status("gathering…".to_string())
        );
        assert_eq!(
            next(&mut bus_rx).await,
            Action::Gathered(delta_services::pipeline::GatherResult {
                ingested: delta_services::pipeline::IngestResult::default(),
                extracted: delta_services::pipeline::ExtractResult {
                    events: 0,
                    instruments: 0
                },
                sentiment: 0,
                warnings: Vec::new(),
            })
        );
        drop(tx);
        let _ = worker.await;
    }

    #[tokio::test]
    async fn gather_worker_acknowledges_an_offline_desk_without_a_config() {
        let dir = tempfile::tempdir().unwrap();
        let (bus_tx, mut bus_rx) = mpsc::unbounded_channel();
        let (tx, rx) = mpsc::unbounded_channel::<Option<String>>();
        let worker = tokio::spawn(gather_worker(
            bus_tx,
            vec![aapl()],
            Some(dir.path().join("test.db")),
            None,
            rx,
        ));
        tx.send(None).unwrap();
        assert_eq!(
            next(&mut bus_rx).await,
            Action::Status("offline desk: nothing to gather".to_string())
        );
        drop(tx);
        let _ = worker.await;
    }

    #[tokio::test]
    async fn gather_worker_degrades_to_warnings_when_model_routing_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("config.toml");
        std::fs::write(&config_path, unrouted_llm_config()).unwrap();
        let (bus_tx, mut bus_rx) = mpsc::unbounded_channel();
        let (tx, rx) = mpsc::unbounded_channel::<Option<String>>();
        let worker = tokio::spawn(gather_worker(
            bus_tx,
            vec![aapl()],
            Some(dir.path().join("test.db")),
            Some(config_path),
            rx,
        ));
        tx.send(None).unwrap();
        assert_eq!(
            next(&mut bus_rx).await,
            Action::Status("gathering…".to_string())
        );
        // The run still finishes; the unrouted model stages ride along as
        // warnings on the gathered action, which the app composes the
        // `· warn:` status suffix from.
        match next(&mut bus_rx).await {
            Action::Gathered(result) => {
                assert!(result.ingested.counts.is_empty());
                assert_eq!(result.extracted.events, 0);
                assert_eq!(result.sentiment, 0);
                assert_eq!(
                    result.warnings.first().map(String::as_str),
                    Some(
                        "extract: no model routed for task \"extract\"; \
                         set [llm] model or add [llm.routing].extract to config.toml"
                    )
                );
                assert_eq!(
                    result.warnings.get(1).map(String::as_str),
                    Some(
                        "sentiment: no model routed for task \"sentiment\"; \
                         set [llm] model or add [llm.routing].sentiment to config.toml"
                    )
                );
            }
            other => panic!("expected Action::Gathered, got {other:?}"),
        }
        drop(tx);
        let _ = worker.await;
    }
}
