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

/// A scoped request or cancellation of the active operation.
#[derive(Debug)]
pub enum WorkRequest<T> {
    Run(T),
    Cancel,
}

/// Poll requests alongside the operation so cancellation drops its future.
/// Repeated Run requests received while busy are discarded.
async fn run_requests<T, F, Fut>(
    mut requests: mpsc::UnboundedReceiver<WorkRequest<T>>,
    bus: mpsc::UnboundedSender<Action>,
    busy: fn(bool) -> Action,
    label: &str,
    mut run: F,
) where
    F: FnMut(T) -> Fut,
    Fut: std::future::Future<Output = ()>,
{
    while let Some(request) = requests.recv().await {
        let WorkRequest::Run(input) = request else {
            continue;
        };
        let _ = bus.send(busy(true));
        let mut closed = false;
        {
            let operation = run(input);
            tokio::pin!(operation);
            loop {
                tokio::select! {
                    biased;
                    request = requests.recv() => match request {
                        Some(WorkRequest::Run(_)) => {},
                        Some(WorkRequest::Cancel) => {
                            let _ = bus.send(Action::Status(format!("{label} cancelled")));
                            break;
                        }
                        None => {
                            closed = true;
                            break;
                        }
                    },
                    () = &mut operation => break,
                }
            }
        }
        let _ = bus.send(busy(false));
        if closed {
            break;
        }
    }
}

fn configured_suffixes() -> BTreeMap<String, String> {
    let cfg = delta_core::config::load_config(std::path::Path::new("config.toml"))
        .map(|(_, cfg)| cfg)
        .unwrap_or_default();
    delta_plugins::configured_suffixes(&cfg, "yfinance")
}

/// Refresh cached diagnostics without database reads in rendering.
pub fn spawn_settings(
    bus: mpsc::UnboundedSender<Action>,
    db_path: Option<PathBuf>,
) -> mpsc::UnboundedSender<()> {
    let (tx, mut requests) = mpsc::unbounded_channel();
    tokio::spawn(async move {
        while requests.recv().await.is_some() {
            let path = db_path.clone();
            let result = tokio::task::spawn_blocking(move || {
                let path = path.ok_or_else(|| "no configured database".to_string())?;
                let db = delta_core::db::Db::open(&path).map_err(|error| error.to_string())?;
                let now = chrono::Utc::now();
                let health = delta_services::data_health(&db).map_err(|error| error.to_string())?;
                let costs =
                    delta_services::llm_costs(&db, None).map_err(|error| error.to_string())?;
                let today = now.date_naive().and_hms_opt(0, 0, 0).expect("midnight");
                let today_usd = delta_services::llm_costs(&db, Some(today))
                    .map_err(|error| error.to_string())?
                    .iter()
                    .map(|row| row.cost_usd)
                    .sum();
                let database_size = std::fs::metadata(&path)
                    .ok()
                    .map(|metadata| {
                        let name = path.file_name().unwrap_or_default().to_string_lossy();
                        format!("{name} · {}", human_size(metadata.len()))
                    })
                    .unwrap_or_default();
                Ok(crate::settings_view::SettingsDiagnostics {
                    health,
                    costs,
                    today_usd,
                    database_size,
                    refreshed: now.format("%H:%M:%S UTC").to_string(),
                })
            })
            .await
            .unwrap_or_else(|error| Err(format!("diagnostics worker: {error}")));
            let _ = bus.send(Action::SettingsDiagnostics(result));
        }
    });
    tx
}

fn human_size(bytes: u64) -> String {
    let mut size = bytes as f64;
    for unit in ["B", "KB", "MB", "GB"] {
        if size < 1024.0 || unit == "GB" {
            return if unit == "B" || unit == "KB" {
                format!("{size:.0} {unit}")
            } else {
                format!("{size:.1} {unit}")
            };
        }
        size /= 1024.0;
    }
    unreachable!()
}

pub fn spawn_models(bus: mpsc::UnboundedSender<Action>) -> mpsc::UnboundedSender<()> {
    let (tx, mut requests) = mpsc::unbounded_channel::<()>();
    tokio::spawn(async move {
        while requests.recv().await.is_some() {
            match delta_services::model_catalog(
                std::path::Path::new("config.toml"),
                std::path::Path::new("data/model_catalog.json"),
            )
            .await
            {
                Ok(models) => {
                    let _ = bus.send(Action::ModelsReady(models));
                }
                Err(error) => {
                    let _ = bus.send(Action::Status(format!("model catalog: {error}")));
                }
            }
        }
    });
    tx
}

/// Provider verification runs outside the event loop.
pub fn spawn_provider_setup(
    bus: mpsc::UnboundedSender<Action>,
) -> mpsc::UnboundedSender<delta_services::ProviderSetup> {
    let (tx, mut requests) = mpsc::unbounded_channel::<delta_services::ProviderSetup>();
    tokio::spawn(async move {
        while let Some(setup) = requests.recv().await {
            match delta_services::connect_provider(
                std::path::Path::new("config.toml"),
                std::path::Path::new(".env"),
                &setup,
            )
            .await
            {
                Ok(verified) => {
                    let _ = bus.send(Action::ProviderConnected {
                        name: setup.name,
                        verified,
                    });
                }
                Err(error) => {
                    let _ = bus.send(Action::Status(format!("provider setup: {error}")));
                }
            }
        }
    });
    tx
}

/// Inspector metrics for the selected instrument and chart range.
pub struct MetricsRequest {
    pub instrument: Instrument,
    pub range: String,
}

/// Own background task lifetimes so changes to watch targets replace old feeds.
pub struct Workers {
    pub gather: mpsc::UnboundedSender<WorkRequest<Vec<String>>>,
    pub metrics: mpsc::UnboundedSender<MetricsRequest>,
    bus: mpsc::UnboundedSender<Action>,
    db_path: Option<PathBuf>,
    quotes_enabled: bool,
    last_seen: chrono::NaiveDateTime,
    live: Vec<tokio::task::JoinHandle<()>>,
    gather_task: tokio::task::JoinHandle<()>,
    metrics_task: tokio::task::JoinHandle<()>,
}

impl Workers {
    pub fn refresh(&mut self, universe: Vec<Instrument>) {
        for task in self.live.drain(..) {
            task.abort();
        }
        if self.quotes_enabled && !universe.is_empty() {
            self.live.push(tokio::spawn(stream_quotes_worker(
                self.bus.clone(),
                universe.clone(),
            )));
            self.live.push(tokio::spawn(metrics_worker(
                self.bus.clone(),
                universe.clone(),
                self.db_path.clone(),
            )));
        }
        if let Some(path) = &self.db_path {
            let ids = universe.iter().map(|inst| inst.id.clone()).collect();
            self.live.push(tokio::spawn(home_refresh_worker(
                self.bus.clone(),
                path.clone(),
                ids,
                self.last_seen,
            )));
        }
    }
}

impl Drop for Workers {
    fn drop(&mut self) {
        for task in &self.live {
            task.abort();
        }
        self.gather_task.abort();
        self.metrics_task.abort();
    }
}

/// Spawn managed feeds and the serialized gather worker.
pub fn spawn(
    bus: mpsc::UnboundedSender<Action>,
    universe: Vec<Instrument>,
    db_path: Option<PathBuf>,
    quotes_enabled: bool,
) -> Workers {
    let (gather_tx, gather_rx) = mpsc::unbounded_channel::<WorkRequest<Vec<String>>>();
    let (metrics, mut metric_requests) = mpsc::unbounded_channel::<MetricsRequest>();
    let metric_bus = bus.clone();
    let metric_db_path = db_path.clone();
    let metrics_task = tokio::spawn(async move {
        while let Some(request) = metric_requests.recv().await {
            refresh_metrics(
                &metric_bus,
                &[request.instrument],
                &request.range,
                metric_db_path.as_deref(),
            )
            .await;
        }
    });
    let gather_task = tokio::spawn(gather_worker(bus.clone(), db_path.clone(), gather_rx));
    let last_seen = db_path
        .as_ref()
        .map(|path| delta_core::state::read_last_seen(&path.to_string_lossy()).naive_utc())
        .unwrap_or_else(|| chrono::Utc::now().naive_utc());
    let mut workers = Workers {
        gather: gather_tx,
        metrics,
        metrics_task,
        bus,
        db_path,
        quotes_enabled,
        last_seen,
        live: Vec::new(),
        gather_task,
    };
    workers.refresh(universe);
    workers
}

/// Report requests are serialized so repeated keypresses cannot write the
/// same day's report concurrently.
pub fn spawn_reports(
    bus: mpsc::UnboundedSender<Action>,
    db_path: Option<PathBuf>,
) -> mpsc::UnboundedSender<WorkRequest<String>> {
    let (tx, requests) = mpsc::unbounded_channel();
    let worker_bus = bus.clone();
    tokio::spawn(run_requests(
        requests,
        bus,
        Action::ReportBusy,
        "report",
        move |target_id: String| {
            let bus = worker_bus.clone();
            let db_path = db_path.clone();
            async move {
                let Some(db_path) = db_path else {
                    let _ = bus.send(Action::Status("report: no configured database".into()));
                    return;
                };
                let cfg = match delta_core::config::load_config(std::path::Path::new("config.toml"))
                {
                    Ok((_, cfg)) => cfg,
                    Err(err) => {
                        let _ = bus.send(Action::Status(format!("report config failed: {err}")));
                        return;
                    }
                };
                let mut db = match delta_core::db::Db::open(&db_path) {
                    Ok(db) => db,
                    Err(err) => {
                        let _ = bus.send(Action::Status(format!("report database failed: {err}")));
                        return;
                    }
                };
                let _ = bus.send(Action::Status(format!("writing report for {target_id}…")));
                match delta_services::generate_report_configured(&mut db, &cfg, &target_id).await {
                    Ok((report, path)) => {
                        let _ = bus.send(Action::ReportReady {
                            target_id,
                            markdown: delta_services::render_markdown(&report, true),
                        });
                        let _ =
                            bus.send(Action::Status(format!("report saved: {}", path.display())));
                    }
                    Err(err) => {
                        let _ = bus.send(Action::Status(format!("report failed: {err}")));
                    }
                }
            }
        },
    ));
    tx
}

/// A complete Ask turn and the instrument ids currently in scope.
pub struct AskRequest {
    pub generation: u64,
    pub history: Vec<delta_services::ChatMessage>,
    pub targets: Vec<String>,
}

pub fn spawn_chat(
    bus: mpsc::UnboundedSender<Action>,
    db_path: Option<PathBuf>,
) -> mpsc::UnboundedSender<WorkRequest<AskRequest>> {
    let (tx, requests) = mpsc::unbounded_channel();
    let worker_bus = bus.clone();
    tokio::spawn(run_requests(
        requests,
        bus,
        Action::AskBusy,
        "ask",
        move |request: AskRequest| {
            let bus = worker_bus.clone();
            let db_path = db_path.clone();
            async move {
                let Some(db_path) = &db_path else {
                    let _ = bus.send(Action::ChatFinished {
                        generation: request.generation,
                        result: Err("ask: no configured database".into()),
                    });
                    return;
                };
                let cfg = match delta_core::config::load_config(std::path::Path::new("config.toml"))
                {
                    Ok((_, cfg)) => cfg,
                    Err(err) => {
                        let _ = bus.send(Action::ChatFinished {
                            generation: request.generation,
                            result: Err(format!("ask config failed: {err}")),
                        });
                        return;
                    }
                };
                let mut db = match delta_core::db::Db::open(db_path) {
                    Ok(db) => db,
                    Err(err) => {
                        let _ = bus.send(Action::ChatFinished {
                            generation: request.generation,
                            result: Err(format!("ask database failed: {err}")),
                        });
                        return;
                    }
                };
                let _ = bus.send(Action::Status(
                    "answering from gathered evidence…".to_string(),
                ));
                match delta_services::chat_configured(
                    &mut db,
                    &cfg,
                    &request.history,
                    &request.targets,
                )
                .await
                {
                    Ok(answer) => {
                        let _ = bus.send(Action::ChatFinished {
                            generation: request.generation,
                            result: Ok(answer),
                        });
                    }
                    Err(err) => {
                        let _ = bus.send(Action::ChatFinished {
                            generation: request.generation,
                            result: Err(format!("ask failed: {err}")),
                        });
                    }
                }
            }
        },
    ));
    tx
}

/// Propose thesis evidence in the background; candidates still need user acceptance.
pub fn spawn_thesis_proposals(
    bus: mpsc::UnboundedSender<Action>,
    db_path: Option<PathBuf>,
) -> mpsc::UnboundedSender<String> {
    let (tx, mut requests) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(thesis_id) = requests.recv().await {
            let Some(path) = &db_path else {
                let _ = bus.send(Action::Status("thesis: no configured database".into()));
                continue;
            };
            let cfg = match delta_core::config::load_config(std::path::Path::new("config.toml")) {
                Ok((_, cfg)) => cfg,
                Err(e) => {
                    let _ = bus.send(Action::Status(format!("thesis config: {e}")));
                    continue;
                }
            };
            let mut db = match delta_core::db::Db::open(path) {
                Ok(db) => db,
                Err(e) => {
                    let _ = bus.send(Action::Status(format!("thesis database: {e}")));
                    continue;
                }
            };
            let _ = bus.send(Action::Status("finding thesis evidence…".into()));
            match delta_services::propose_thesis_evidence_configured(&mut db, &cfg, &thesis_id, 100)
                .await
            {
                Ok(links) => {
                    let _ = bus.send(Action::ThesisProposed(links.len()));
                }
                Err(e) => {
                    let _ = bus.send(Action::Status(format!("thesis proposal failed: {e}")));
                }
            }
        }
    });
    tx
}

pub fn spawn_thesis_summaries(
    bus: mpsc::UnboundedSender<Action>,
    db_path: Option<PathBuf>,
) -> mpsc::UnboundedSender<String> {
    let (tx, mut requests) = mpsc::unbounded_channel::<String>();
    tokio::spawn(async move {
        while let Some(thesis_id) = requests.recv().await {
            let result = async {
                let path = db_path.as_ref().ok_or_else(|| {
                    delta_services::ServiceError::invalid("no configured database")
                })?;
                let (_, cfg) = delta_core::config::load_config(std::path::Path::new("config.toml"))
                    .map_err(|e| delta_services::ServiceError::invalid(e.to_string()))?;
                let mut db = delta_core::db::Db::open(path)?;
                delta_services::summarize_thesis_configured(&mut db, &cfg, &thesis_id).await
            }
            .await;
            match result {
                Ok(summary) => {
                    let text=format!("{}\nState: {}\n\n{}\n\nSupport: {}\n\nCounter: {}\n\nUnknowns:\n{}\n\nSources: {}",summary.claim,summary.state.as_str(),summary.summary,summary.strongest_support,summary.strongest_counter,summary.unknowns.join("\n"),summary.citations.join(", "));
                    let _ = bus.send(Action::ThesisSummaryReady(text));
                }
                Err(error) => {
                    let _ = bus.send(Action::Status(format!("thesis summary: {error}")));
                }
            }
        }
    });
    tx
}

/// Re-run the Home analytics queries and push them to the bus, low-frequency
/// local SQL on the same cadence as the metrics worker — no new pollers.
const HOME_REFRESH_INTERVAL: Duration = Duration::from_secs(60);

async fn home_refresh_worker(
    bus: mpsc::UnboundedSender<Action>,
    db_path: PathBuf,
    ids: Vec<String>,
    last_seen: chrono::NaiveDateTime,
) {
    loop {
        tokio::time::sleep(HOME_REFRESH_INTERVAL).await;
        if let Ok(db) = delta_core::db::Db::open(&db_path) {
            let provider = delta_core::config::load_config(std::path::Path::new("config.toml"))
                .map(|(_, cfg)| cfg.llm_provider)
                .unwrap_or_default();
            if let Ok(state) = crate::footer::FooterState::load(&db, provider) {
                let _ = bus.send(Action::FooterRefresh(state));
            }
            let _ = bus.send(Action::HomeRefresh(crate::desk::load_feed_since(
                &db, &ids, last_seen,
            )));
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
    let suffixes = configured_suffixes();
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
async fn refresh_metrics(
    bus: &mpsc::UnboundedSender<Action>,
    universe: &[Instrument],
    range: &str,
    db_path: Option<&std::path::Path>,
) {
    let client = YahooClient {
        suffixes: configured_suffixes(),
        ..YahooClient::default()
    };
    let db = db_path.and_then(|path| match delta_core::db::Db::open(path) {
        Ok(db) => Some(db),
        Err(error) => {
            let _ = bus.send(Action::Status(format!("metrics local database: {error}")));
            None
        }
    });
    for inst in universe {
        let metrics =
            delta_services::asset_metrics::fetch_asset_metrics(&client, inst, range, db.as_ref())
                .await;
        let _ = bus.send(Action::AssetMetrics {
            range: range.to_string(),
            metrics,
        });
    }
}

async fn metrics_worker(
    bus: mpsc::UnboundedSender<Action>,
    universe: Vec<Instrument>,
    db_path: Option<PathBuf>,
) {
    loop {
        refresh_metrics(&bus, &universe, "1m", db_path.as_deref()).await;
        tokio::time::sleep(METRICS_INTERVAL).await;
    }
}

/// Run the configured ingest, extract, and sentiment pipeline on `Gather`.
async fn gather_worker(
    bus: mpsc::UnboundedSender<Action>,
    db_path: Option<PathBuf>,
    requests: mpsc::UnboundedReceiver<WorkRequest<Vec<String>>>,
) {
    let worker_bus = bus.clone();
    run_requests(requests, bus, Action::GatherBusy, "gather", move |ids| {
        let bus = worker_bus.clone();
        let db_path = db_path.clone();
        async move {
            let Some(db_path) = db_path else {
                let _ = bus.send(Action::Status("offline desk: nothing to gather".into()));
                return;
            };
            let _ = bus.send(Action::Status("gathering…".into()));
            let cfg = match delta_core::config::load_config(std::path::Path::new("config.toml")) {
                Ok((_, cfg)) => cfg,
                Err(e) => {
                    let _ = bus.send(Action::Status(format!("config load failed: {e}")));
                    return;
                }
            };
            let mut universe =
                match delta_services::configured_universe(std::path::Path::new("config.toml")) {
                    Ok(universe) => universe,
                    Err(error) => {
                        let _ = bus.send(Action::Status(format!("watch targets: {error}")));
                        return;
                    }
                };
            if !ids.is_empty() {
                universe.retain(|instrument| ids.contains(&instrument.id));
                if universe.is_empty() {
                    let _ = bus.send(Action::Status(
                        "gather: selected instruments are no longer configured".into(),
                    ));
                    return;
                }
            }
            let mut db = match delta_core::db::Db::open(&db_path) {
                Ok(db) => db,
                Err(e) => {
                    let _ = bus.send(Action::Status(format!("db open failed: {e}")));
                    return;
                }
            };
            match delta_services::gather_configured(&mut db, &cfg, &universe, |_| {}).await {
                Ok(result) => {
                    let _ = bus.send(Action::Gathered {
                        counts: result.ingested.counts,
                        events: result.extracted.events,
                        sentiment: result.sentiment,
                        warnings: result.warnings,
                    });
                }
                Err(e) => {
                    let _ = bus.send(Action::Status(format!("gather failed: {e}")));
                }
            }
        }
    })
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cancellation_drops_work_and_discards_repeated_runs() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        struct DropMarker(Arc<AtomicUsize>);
        impl Drop for DropMarker {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }
        let (tx, requests) = mpsc::unbounded_channel();
        let (bus, mut actions) = mpsc::unbounded_channel();
        let starts = Arc::new(AtomicUsize::new(0));
        let drops = Arc::new(AtomicUsize::new(0));
        let worker_starts = starts.clone();
        let worker_drops = drops.clone();
        let task = tokio::spawn(run_requests(
            requests,
            bus,
            Action::GatherBusy,
            "gather",
            move |_: ()| {
                worker_starts.fetch_add(1, Ordering::SeqCst);
                let marker = DropMarker(worker_drops.clone());
                async move {
                    let _marker = marker;
                    std::future::pending::<()>().await;
                }
            },
        ));
        tx.send(WorkRequest::Run(())).unwrap();
        assert!(matches!(
            actions.recv().await,
            Some(Action::GatherBusy(true))
        ));
        tx.send(WorkRequest::Run(())).unwrap();
        tx.send(WorkRequest::Cancel).unwrap();
        assert!(
            matches!(actions.recv().await, Some(Action::Status(message)) if message == "gather cancelled")
        );
        assert!(matches!(
            actions.recv().await,
            Some(Action::GatherBusy(false))
        ));
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(drops.load(Ordering::SeqCst), 1);
        tx.send(WorkRequest::Run(())).unwrap();
        assert!(matches!(
            actions.recv().await,
            Some(Action::GatherBusy(true))
        ));
        drop(tx);
        task.await.unwrap();
        assert!(matches!(
            actions.recv().await,
            Some(Action::GatherBusy(false))
        ));
        assert_eq!(drops.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn unavailable_report_clears_busy_state() {
        let (bus, mut actions) = mpsc::unbounded_channel();
        let requests = spawn_reports(bus, None);
        requests.send(WorkRequest::Run("US:AAPL".into())).unwrap();
        assert!(matches!(
            actions.recv().await,
            Some(Action::ReportBusy(true))
        ));
        assert!(
            matches!(actions.recv().await, Some(Action::Status(message)) if message.contains("no configured database"))
        );
        assert!(matches!(
            actions.recv().await,
            Some(Action::ReportBusy(false))
        ));
    }

    #[tokio::test]
    async fn unavailable_ask_keeps_generation_and_clears_busy_state() {
        let (bus, mut actions) = mpsc::unbounded_channel();
        let requests = spawn_chat(bus, None);
        requests
            .send(WorkRequest::Run(AskRequest {
                generation: 42,
                history: vec![],
                targets: vec![],
            }))
            .unwrap();
        assert!(matches!(actions.recv().await, Some(Action::AskBusy(true))));
        assert!(
            matches!(actions.recv().await, Some(Action::ChatFinished { generation: 42, result: Err(message) }) if message.contains("no configured database"))
        );
        assert!(matches!(actions.recv().await, Some(Action::AskBusy(false))));
    }

    #[test]
    fn diagnostics_sizes_match_python() {
        assert_eq!(human_size(500), "500 B");
        assert_eq!(human_size(1536), "2 KB");
        assert_eq!(human_size(1_572_864), "1.5 MB");
    }

    #[tokio::test]
    async fn refreshing_workers_cancels_old_feed_tasks() {
        let (bus, _rx) = mpsc::unbounded_channel();
        let mut workers = spawn(bus, vec![], Some(PathBuf::from("unused.db")), false);
        let old = workers.live[0].abort_handle();
        workers.refresh(vec![]);
        tokio::task::yield_now().await;
        assert!(old.is_finished());
        assert_eq!(workers.live.len(), 1);
    }
}
