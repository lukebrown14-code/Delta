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
use crate::screens::decisions::{draw_decision_review_form, draw_decisions_live, DecisionsData};
use crate::screens::research::{draw_research_live, ResearchData, ResearchState};
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
        "live-research" | "live-research-search" | "live-research-citation" => {
            Some(live_research(state, fixture, db_path, w, h))
        }
        "live-ask" => Some(live_ask(fixture, db_path, w, h)),
        "live-decisions"
        | "live-decisions-review"
        | "live-decisions-confirm"
        | "live-decisions-review-form" => live_decisions(fixture, db_path, w, h),
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

fn live_research(
    state_name: &str,
    _fixture: &serde_json::Value,
    db_path: &Path,
    w: usize,
    h: usize,
) -> Screen {
    let reports = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/golden_reports");
    let data = ResearchData::load(db_path, &reports, "US:AAPL", "apple", "company", "USD")
        .expect("load shared research seed");
    let mut screen = Screen::new(w, h);
    let mut state = ResearchState::default();
    if state_name == "live-research-search" {
        state.search_active = true;
    } else if state_name == "live-research-citation" {
        state.selected_id = Some("news:news-aapl-chip".into());
        state.detail_open = true;
        state.view = crate::screens::research::ResearchView::Report;
    }
    draw_research_live(&mut screen, &data, &state);
    screen
}

fn live_ask(fixture: &serde_json::Value, db_path: &Path, w: usize, h: usize) -> Screen {
    use crate::screens::ask::{
        draw_ask, draw_ask_narrow, draw_ask_wide, paint_ask_state, AskState,
    };
    use delta_services::chat::ChatMessage;
    let db = delta_core::db::Db::open(db_path).expect("golden seed database");
    let mut state = AskState {
        provider: fixture["seed"]["config"]["llm"]["provider"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        model: fixture["seed"]["config"]["llm"]["model"]
            .as_str()
            .unwrap_or_default()
            .to_string(),
        ..Default::default()
    };
    let targets = fixture["seed"]["config"]["targets"]
        .as_object()
        .expect("seed targets");
    state.set_targets(
        targets
            .iter()
            .map(|(id, spec)| {
                delta_services::target_from_spec(id, spec, false).expect("target spec")
            })
            .collect(),
    );
    let mut query = db
        .conn()
        .prepare("SELECT role, text, citations, source FROM chatmessage ORDER BY seq")
        .expect("chatmessage table");
    state.history = query
        .query_map([], |row| {
            let citations: String = row.get(2)?;
            Ok(ChatMessage {
                role: row.get(0)?,
                text: row.get(1)?,
                citations: serde_json::from_str(&citations).unwrap_or_default(),
                source: row.get(3)?,
            })
        })
        .expect("chat turns")
        .map(|turn| turn.expect("chat row"))
        .collect();
    let ids: Vec<String> = state
        .history
        .iter()
        .flat_map(|turn| turn.citations.iter().cloned())
        .collect();
    for id in ids {
        if let Some(item) = delta_services::evidence_by_ids(&db, std::slice::from_ref(&id))
            .expect("citation evidence")
            .into_iter()
            .next()
        {
            let label = format!("{} · {}", item.title, item.ts.format("%-d %b"));
            state.sidebar_labels.insert(id.clone(), label.clone());
            if item.id == id {
                state.citation_labels.insert(id, label);
            }
        }
    }
    let mut screen = Screen::new(w, h);
    if w < 100 {
        draw_ask_narrow(&mut screen);
    } else if w >= 160 {
        draw_ask_wide(&mut screen);
    } else {
        draw_ask(&mut screen);
    }
    paint_ask_state(&mut screen, &state);
    let spend: f64 = db
        .conn()
        .query_row(
            "SELECT COALESCE(SUM(cost_usd), 0) FROM llmcall",
            [],
            |row| row.get(0),
        )
        .expect("seed spend");
    let cost_x = if w < 100 { w - 19 } else { w - 34 };
    screen.text(
        cost_x,
        h - 1,
        &format!("${spend:.2}"),
        crate::screen::Style::fg(crate::screen::color::FG).bg(crate::screen::color::PANEL),
    );
    for cell in &mut screen.cells[(h - 1) * w..h * w] {
        if cell.bg == Some(crate::screen::color::BLUE_BG) {
            cell.bg = Some("#494949");
        }
        if cell.ch == '●' && cell.fg == Some(crate::screen::color::AMBER) {
            cell.fg = Some("#a6a6a6");
        }
    }
    screen
}

fn live_decisions(
    fixture: &serde_json::Value,
    db_path: &Path,
    w: usize,
    h: usize,
) -> Option<Screen> {
    let db = delta_core::db::Db::open(db_path).ok()?;
    let decisions = delta_services::list_decisions(&db, None, true).ok()?;
    let selected = if fixture["state"] == "live-decisions-review" {
        1
    } else {
        0
    };
    let decision = decisions.get(selected)?;
    let reviews = delta_services::review_history(&db, &decision.id).ok()?;
    let current_price = db
        .bars(&decision.instrument_id)
        .ok()?
        .last()
        .map(|bar| bar.close);
    let data = DecisionsData {
        decisions,
        selected,
        reviews,
        current_price,
        filter: String::new(),
        spend: delta_services::analytics::total_spend(&db, None),
        confirm_delete: fixture["state"] == "live-decisions-confirm",
    };
    let mut screen = Screen::new(w, h);
    draw_decisions_live(&mut screen, &data);
    if fixture["state"] == "live-decisions-review-form" {
        draw_decision_review_form(&mut screen, "", "reviewed");
    }
    Some(screen)
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
