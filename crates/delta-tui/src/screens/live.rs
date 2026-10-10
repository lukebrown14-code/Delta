//! The populated-state ("live") golden registry.
//!
//! Each `live-*` state in `fixtures/golden_screens/live-manifest.json` maps
//! to one builder here that renders the screen from the shared seed DB plus
//! the fixture's embedded seed block (the config the exporter booted with,
//! the frozen clock, the pinned sizes). Adding a screen state is a manifest
//! entry + fixture + one builder line in [`render_live`] — the harness
//! (`tests/live_golden.rs`) never changes.

use std::path::Path;

use delta_core::config::AppConfig;

use crate::screen::Screen;
use crate::screens::settings::{
    draw_settings, draw_settings_narrow, draw_settings_wide, SettingsData, SettingsState,
    SettingsView,
};
use crate::screens::theses::{draw_theses_live, ThesesState};

/// Render one live scenario at its manifest size, or `None` when no builder
/// is registered for `state`.
pub fn render_live(state: &str, fixture: &serde_json::Value, db_path: &Path) -> Option<Screen> {
    let w = fixture["size"][0].as_u64()? as usize;
    let h = fixture["size"][1].as_u64()? as usize;
    match state {
        // One line per populated screen state; each later screen ticket
        // registers its builder here.
        "live-settings" => Some(live_settings(fixture, db_path, w, h)),
        "live-theses" => Some(live_theses(fixture, db_path, w, h)),
        _ => None,
    }
}

fn live_theses(fixture: &serde_json::Value, db_path: &Path, w: usize, h: usize) -> Screen {
    let db = delta_core::db::Db::open(db_path).expect("live theses seed db");
    let now =
        chrono::DateTime::parse_from_rfc3339(fixture["seed"]["now"].as_str().expect("fixture now"))
            .expect("RFC 3339 now stamp")
            .naive_utc();
    let mut state = ThesesState::default();
    state.load_at(&db, Some(now)).expect("load live theses");
    let mut screen = Screen::new(w, h);
    draw_theses_live(&mut screen, &state);
    screen
}

/// The populated Settings screen: config, provider state and pinned sizes
/// from the fixture's seed block; diagnostics from the shared seed DB
/// through the same `SettingsData::load` the live app calls.
fn live_settings(fixture: &serde_json::Value, db_path: &Path, w: usize, h: usize) -> Screen {
    let seed = &fixture["seed"];
    let config_json = &seed["config"];
    let config = AppConfig {
        llm_provider: config_json["llm"]["provider"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        llm_model: config_json["llm"]["model"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        plugins: serde_json::from_value(config_json["plugins"].clone())
            .expect("fixture plugins table"),
        markets: serde_json::from_value(config_json["markets"].clone())
            .expect("fixture markets table"),
        ..AppConfig::default()
    };
    let now = chrono::DateTime::parse_from_rfc3339(seed["now"].as_str().expect("fixture now"))
        .expect("RFC 3339 now stamp")
        .naive_utc();
    // The exporter's environment pins the .env read (`read_env_value` is
    // monkeypatched to a test key) and the database size; both come from
    // the seed block. The env path never exists, so nothing leaks in from
    // the host.
    let env_pin = Path::new("/nonexistent/delta-live-golden/.env");
    let mut data = SettingsData::load(&config, db_path, env_pin, now);
    data.provider_connected = seed["provider_connected"].as_bool().unwrap_or(false);
    if let Ok(diag) = &mut data.diagnostics {
        diag.database_size = seed["database_size"]
            .as_str()
            .unwrap_or_default()
            .to_string();
    }
    let footer = data.footer(now);
    let state = SettingsState::default();
    let view = SettingsView {
        data: &data,
        state: &state,
        footer: &footer,
    };
    let mut screen = Screen::new(w, h);
    if w < 100 {
        draw_settings_narrow(&mut screen, &view);
    } else if w >= 160 {
        draw_settings_wide(&mut screen, &view);
    } else {
        draw_settings(&mut screen, &view);
    }
    screen
}
