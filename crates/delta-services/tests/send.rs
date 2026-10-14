//! Public async service entry points the TUI spawns on tokio must return
//! `Send` futures. A non-`Send` future forces every spawn site into a
//! blocking-pool workaround, so `Send`-ness is asserted here, at the seam
//! that produces the futures.

fn require_send<T: Send>(future: T) {
    drop(future);
}

#[test]
fn worker_spawned_service_futures_are_send() {
    let mut db = delta_core::db::Db::open_memory().unwrap();
    let cfg = delta_core::config::AppConfig::default();
    let universe: Vec<delta_core::models::Instrument> = Vec::new();
    let history: Vec<delta_services::chat::ChatMessage> = Vec::new();
    let targets: Vec<String> = Vec::new();
    let env = std::path::Path::new(".env");

    require_send(delta_services::gather_configured(
        &mut db,
        &cfg,
        &universe,
        |_| {},
    ));
    require_send(delta_services::chat::chat_configured(
        &mut db, &cfg, &history, &targets,
    ));
    require_send(delta_services::generate_report_configured(
        &mut db, &cfg, "US:AAPL",
    ));
    require_send(delta_services::propose_evidence_configured(
        &mut db, &cfg, env, "t1",
    ));
    require_send(delta_services::summarize_thesis_configured(
        &mut db, &cfg, env, "t1",
    ));
}
