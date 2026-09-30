//! Background workers over the action bus: live Yahoo quotes and on-demand
//! ingest (`docs/RUST_REWRITE_PLAN.md` TUI pattern — nothing on the render
//! path blocks; workers report through `Action`s).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use delta_core::models::Instrument;
use delta_plugins::{yf_symbol, DataPlugin, YahooClient};
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
        tokio::spawn(quotes_worker(bus.clone(), universe.clone()));
    }
    let (gather_tx, gather_rx) = mpsc::unbounded_channel::<()>();
    tokio::spawn(gather_worker(bus.clone(), universe, db_path, gather_rx));
    gather_tx
}

/// Poll batched Yahoo quotes every `QUOTES_INTERVAL`, reporting live prices.
const QUOTES_INTERVAL: Duration = Duration::from_secs(30);

async fn quotes_worker(bus: mpsc::UnboundedSender<Action>, universe: Vec<Instrument>) {
    if universe.is_empty() {
        return;
    }
    let client = YahooClient::default();
    let symbols: Vec<String> = universe
        .iter()
        .map(|inst| yf_symbol(inst, &client.suffixes))
        .collect();
    let ids: BTreeMap<String, String> = universe
        .iter()
        .map(|inst| (yf_symbol(inst, &client.suffixes), inst.id.clone()))
        .collect();
    loop {
        match client.quotes(&symbols).await {
            Ok(payload) => {
                let mut prices = BTreeMap::new();
                if let Some(rows) = payload
                    .get("quote")
                    .and_then(|q| q.get("result"))
                    .and_then(|r| r.as_array())
                {
                    for row in rows {
                        if let (Some(sym), Some(px)) = (
                            row.get("symbol").and_then(|s| s.as_str()),
                            row.get("regularMarketPrice").and_then(|p| p.as_f64()),
                        ) {
                            if let Some(id) = ids.get(sym) {
                                prices.insert(id.clone(), px);
                            }
                        }
                    }
                }
                if !prices.is_empty() {
                    let _ = bus.send(Action::Quotes(prices));
                }
            }
            Err(e) => {
                let _ = bus.send(Action::Status(format!("quotes error: {e}")));
            }
        }
        tokio::time::sleep(QUOTES_INTERVAL).await;
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
