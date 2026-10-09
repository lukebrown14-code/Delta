//! Background workers over the action bus: live Yahoo quotes and on-demand
//! ingest (`docs/RUST_REWRITE_PLAN.md` TUI pattern — nothing on the render
//! path blocks; workers report through `Action`s).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use delta_core::models::Instrument;
use delta_plugins::{decode_stream_frame, DataPlugin, YahooClient, YahooQuotes};
use tokio::sync::mpsc;

use crate::desk::now_naive;
use crate::Action;

/// Active workers for the current watched universe. Replacing this handle
/// stops obsolete subscriptions after a target is added or removed.
pub struct Workers {
    pub gather_tx: mpsc::UnboundedSender<()>,
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
    let (gather_tx, gather_rx) = mpsc::unbounded_channel::<()>();
    tasks.push(tokio::spawn(gather_worker(
        bus.clone(),
        universe,
        db_path.clone(),
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

/// Run the ingest pipeline over the universe on `Gather` requests.
async fn gather_worker(
    bus: mpsc::UnboundedSender<Action>,
    universe: Vec<Instrument>,
    db_path: Option<PathBuf>,
    mut requests: mpsc::UnboundedReceiver<()>,
) {
    let Some(db_path) = db_path else {
        // Offline desk: acknowledge gathers so the UI can say so.
        while requests.recv().await.is_some() {
            let _ = bus.send(Action::Status(
                "offline desk: nothing to gather".to_string(),
            ));
        }
        return;
    };
    // The static registry as shared plugins (Box -> Arc; a small helper in
    // delta-plugins would tidy this up).
    let plugins: Vec<Arc<dyn DataPlugin>> = vec![
        Arc::new(delta_plugins::rss::RssData::default()),
        Arc::new(delta_plugins::asx::AsxAnnouncements::default()),
        Arc::new(delta_plugins::sec::SecEdgar::default()),
        Arc::new(delta_plugins::yahoo::YfinanceBars::default()),
    ];
    while requests.recv().await.is_some() {
        let _ = bus.send(Action::Status("gathering…".to_string()));
        let since = (now_naive() - chrono::Duration::days(30))
            .format("%Y-%m-%d")
            .to_string();
        let mut db = match delta_core::db::Db::open(&db_path) {
            Ok(db) => db,
            Err(e) => {
                let _ = bus.send(Action::Status(format!("db open failed: {e}")));
                continue;
            }
        };
        match delta_services::pipeline::ingest(
            &mut db,
            &plugins,
            &universe,
            None,
            None,
            Some(&since),
            4,
            |_| {},
        )
        .await
        {
            Ok(result) => {
                let _ = bus.send(Action::Ingested(result.counts));
            }
            Err(e) => {
                let _ = bus.send(Action::Status(format!("ingest failed: {e}")));
            }
        }
    }
}
