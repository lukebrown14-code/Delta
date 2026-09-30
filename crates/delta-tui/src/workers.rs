//! Background workers over the action bus: live Yahoo quotes and on-demand
//! ingest (`docs/RUST_REWRITE_PLAN.md` TUI pattern — nothing on the render
//! path blocks; workers report through `Action`s).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use delta_core::models::Instrument;
use delta_plugins::{decode_stream_frame, yf_symbol, DataPlugin, YahooClient, YahooQuotes};
use tokio::sync::mpsc;

use crate::desk::now_naive;
use crate::Action;

/// Spawn the background workers. Returns the gather-request channel the UI
/// sends [`Action::Gather`] through.
pub fn spawn(
    bus: mpsc::UnboundedSender<Action>,
    universe: Vec<Instrument>,
    db_path: Option<PathBuf>,
    quotes_enabled: bool,
) -> mpsc::UnboundedSender<()> {
    if quotes_enabled {
        tokio::spawn(stream_quotes_worker(bus.clone(), universe.clone()));
        tokio::spawn(metrics_worker(bus.clone(), universe.clone()));
    }
    let (gather_tx, gather_rx) = mpsc::unbounded_channel::<()>();
    tokio::spawn(gather_worker(bus.clone(), universe, db_path, gather_rx));
    gather_tx
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
const METRICS_MODULES: &str = "price,summaryDetail,defaultKeyStatistics,financialData,assetProfile";

async fn metrics_worker(bus: mpsc::UnboundedSender<Action>, universe: Vec<Instrument>) {
    let client = YahooClient::default();
    loop {
        for inst in &universe {
            let symbol = yf_symbol(inst, &client.suffixes);
            match client.quote_summary(&symbol, METRICS_MODULES).await {
                Ok(payload) => {
                    if let Some(info) = delta_plugins::merge_quote_summary(&payload) {
                        let rows = delta_plugins::headline_values(&info);
                        let _ = bus.send(Action::Metrics {
                            instrument: inst.id.clone(),
                            rows,
                        });
                    }
                }
                Err(e) => {
                    let _ = bus.send(Action::Status(format!("metrics error: {e}")));
                }
            }
        }
        tokio::time::sleep(METRICS_INTERVAL).await;
    }
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
