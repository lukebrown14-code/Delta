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
use crate::screens::home::{draw_home_live, HomeFeed, HomeLiveData, HomeState};
use crate::screens::research::{draw_research_live, ResearchData, ResearchState};
use crate::screens::settings::{
    draw_settings, draw_settings_narrow, draw_settings_wide, SettingsData, SettingsState,
    SettingsView,
};
use crate::screens::theses::{draw_theses_live, ThesesState};
use crate::screens::watchlist::{
    draw_watchlist, draw_watchlist_narrow, draw_watchlist_narrow_list, draw_watchlist_wide,
    grouped, MetricsData, WatchEntry, WatchlistState,
};

/// Render one live scenario at its manifest size, or `None` when no builder
/// is registered for `state`.
pub fn render_live(state: &str, fixture: &serde_json::Value, db_path: &Path) -> Option<Screen> {
    let w = fixture["size"][0].as_u64()? as usize;
    let h = fixture["size"][1].as_u64()? as usize;
    match state {
        // One line per populated screen state; each later screen ticket
        // registers its builder here.
        "live-settings" => Some(live_settings(fixture, db_path, w, h)),
        "live-home" => Some(live_home(db_path, w, h)),
        "live-watchlist" | "live-watchlist-inspector" | "live-watchlist-range" => {
            Some(live_watchlist(state, db_path, w, h))
        }
        "live-theses" => Some(live_theses(fixture, db_path, w, h)),
        "live-research"
        | "live-research-search"
        | "live-research-citation"
        | "live-research-no-report"
        | "live-research-kind-news"
        | "live-research-zoom-report"
        | "live-research-zoom-evidence"
        | "live-research-generating"
        | "live-research-error" => Some(live_research(state, fixture, db_path, w, h)),
        "live-ask"
        | "live-ask-typing"
        | "live-ask-waiting"
        | "live-ask-answered"
        | "live-ask-citation-selected"
        | "live-ask-confirm"
        | "live-ask-error" => Some(live_ask(state, fixture, db_path, w, h)),
        "live-decisions"
        | "live-decisions-review"
        | "live-decisions-confirm"
        | "live-decisions-review-form"
        | "live-decisions-empty" => live_decisions(fixture, db_path, w, h),
        _ => None,
    }
}

fn live_home(db_path: &Path, w: usize, h: usize) -> Screen {
    let db = delta_core::db::Db::open(db_path).expect("golden seed database");
    let bars = db.bars("US:AAPL").expect("seed AAPL bars");
    let closes: Vec<f64> = bars[bars.len() - 40..]
        .iter()
        .map(|bar| bar.close)
        .collect();
    let last = closes[closes.len() - 1];
    let prev = closes[closes.len() - 2];
    let state = HomeState {
        clock: "Monday 21 September 2026 · 09:30:00 UTC".into(),
        symbol: "AAPL".into(),
        last: grouped(last),
        chg_label: format!("{:+.2}%", (last / prev - 1.0) * 100.0),
        spark: crate::braille::BrailleGraph::filled(closes.clone()).rows(17, 1)[0].clone(),
        since_stamp: "since Mon 09:30".into(),
        closes,
        feed: HomeFeed::seed(),
    };
    let now = chrono::NaiveDate::from_ymd_opt(2026, 9, 21)
        .expect("frozen date")
        .and_hms_opt(9, 30, 0)
        .expect("frozen time");
    let ids = vec!["US:AAPL".to_string()];
    let pulse =
        delta_services::analytics::pulse(&db, &ids, now - chrono::Duration::days(7), 30, now)
            .expect("seed pulse");
    let headline = delta_services::analytics::latest_headline(&db, &ids, 50)
        .expect("seed headline")
        .map(|item| {
            let days = (now - item.ts).num_days();
            (item.title, format!("{days}d"))
        });
    let upcoming = delta_services::analytics::upcoming_events(&db, &ids, 5, now)
        .expect("seed upcoming")
        .into_iter()
        .map(|event| {
            (
                event
                    .instrument_id
                    .split(':')
                    .next_back()
                    .unwrap_or_default()
                    .to_string(),
                event.kind,
                event.ts.format("%d %b").to_string(),
                format!("in {}d", (event.ts.date() - now.date()).num_days()),
                event.summary,
            )
        })
        .collect();
    let theses = delta_services::thesis_fleet(&db, Some(now))
        .expect("seed thesis fleet")
        .into_iter()
        .map(|row| {
            let state = row.state().as_str().to_string();
            let ratio = row.result.as_ref().map_or_else(
                || "—".to_string(),
                |result| format!("{} / {}", result.support, result.against),
            );
            (row.thesis.claim, ratio, state)
        })
        .collect();
    let due_reviews = delta_services::due_reviews(&db, Some(now.date()))
        .expect("seed due reviews")
        .len();
    let health = delta_services::analytics::data_health(&db).expect("seed health");
    let stale_days = health
        .latest_bar
        .get("US:AAPL")
        .map_or(0, |last| (now - *last).num_days().max(0));
    let data = HomeLiveData {
        articles: pulse.articles,
        filings: pulse.filings,
        events: pulse.events,
        pulse: pulse.daily,
        busiest: pulse.busiest.map(|(id, count)| {
            (
                id.split(':').next_back().unwrap_or_default().to_string(),
                count,
            )
        }),
        headline,
        stale_sources: usize::from(stale_days > 0),
        stale_text: format!("{stale_days}d old"),
        due_reviews,
        review_context: "6 decisions".into(),
        upcoming,
        theses,
    };
    let mut screen = Screen::new(w, h);
    draw_home_live(&mut screen, &state, &data);
    apply_populated_focus(&mut screen);
    paint_spend(
        &mut screen,
        delta_services::analytics::total_spend(&db, None),
    );
    screen
}

fn live_watchlist(state_name: &str, db_path: &Path, w: usize, h: usize) -> Screen {
    let db = delta_core::db::Db::open(db_path).expect("golden seed database");
    let bars = db.bars("US:AAPL").expect("seed AAPL bars");
    let window = &bars[bars.len() - 30..];
    let series: Vec<f64> = window.iter().map(|bar| bar.close).collect();
    let times: Vec<String> = window
        .iter()
        .map(|bar| format!("{}+00:00", bar.ts.format("%Y-%m-%dT%H:%M:%S")))
        .collect();
    let first = series[0];
    let last = *series.last().expect("nonempty seed bars");
    let card = |name: &str| -> String {
        let value = db
            .fundamentals("US:AAPL")
            .expect("seed fundamentals")
            .iter()
            .find(|row| row.metric == name)
            .expect("seed metric")
            .value;
        if value.abs() >= 1e12 {
            format!("{:.1}T", value / 1e12)
        } else {
            format!("{value:.1}")
        }
    };
    let data = WatchlistState {
        range: if state_name == "live-watchlist-range" {
            "6m"
        } else {
            "1m"
        },
        scrub: None,
        entries: vec![WatchEntry {
            symbol: "AAPL".into(),
            asset_class: "equity".into(),
        }],
        selected: 0,
        metric: Some(MetricsData {
            symbol: "AAPL".into(),
            market: "us".into(),
            asset_class: "equity".into(),
            currency: "USD".into(),
            current: grouped(last),
            change_label: Some(format!("{:+.1}%", (last / first - 1.0) * 100.0)),
            period_high: Some(series.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            period_low: Some(series.iter().copied().fold(f64::INFINITY, f64::min)),
            history_start: times.first().cloned(),
            history_end: times.last().cloned(),
            series,
            series_times: times,
            source: "Yahoo Finance".into(),
            values: vec![
                ("Current price".into(), grouped(last)),
                ("Market cap".into(), card("Market cap")),
                ("P/E".into(), card("P/E")),
            ],
        }),
    };
    let mut screen = Screen::new(w, h);
    if w < 100 {
        if state_name == "live-watchlist" {
            draw_watchlist_narrow_list(&mut screen, &data);
        } else {
            draw_watchlist_narrow(&mut screen, &data);
        }
    } else if w >= 160 {
        draw_watchlist_wide(&mut screen, &data);
    } else {
        draw_watchlist(&mut screen, &data);
    }
    apply_populated_focus(&mut screen);
    paint_spend(
        &mut screen,
        delta_services::analytics::total_spend(&db, None),
    );
    screen
}

/// Textual's populated fixture starts with the TUI unfocused: its primary
/// blue, positive chart line and active chip resolve to these subdued tokens.
fn apply_populated_focus(screen: &mut Screen) {
    for cell in &mut screen.cells {
        cell.fg = match cell.fg {
            Some(crate::screen::color::BLUE) => Some("#898989"),
            Some(crate::screen::color::GREEN) => Some("#9b9b9b"),
            Some(crate::screen::color::AMBER) => Some("#a6a6a6"),
            other => other,
        };
        if cell.bg == Some(crate::screen::color::BLUE_BG) {
            cell.bg = Some("#494949");
        }
    }
}

fn paint_spend(screen: &mut Screen, spend: f64) {
    let text = format!("${:.2}", spend.max(0.0));
    let y = screen.h - 1;
    let row: Vec<char> = screen.cells[y * screen.w..(y + 1) * screen.w]
        .iter()
        .map(|cell| cell.ch)
        .collect();
    if let Some(x) = row
        .windows(5)
        .position(|window| window == ['$', '0', '.', '0', '0'])
    {
        for (offset, ch) in text.chars().enumerate() {
            screen.cells[y * screen.w + x + offset].ch = ch;
        }
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
    let reports =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(if state_name == "live-research-no-report" {
            "../../fixtures/no_reports"
        } else {
            "../../fixtures/golden_reports"
        });
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
    } else if state_name == "live-research-kind-news" {
        state.cycle_kind();
    } else if state_name == "live-research-zoom-report" {
        state.view = crate::screens::research::ResearchView::Report;
        state.zoom = true;
    } else if state_name == "live-research-zoom-evidence" {
        state.zoom = true;
    } else if state_name == "live-research-generating" {
        state.busy = true;
        state.activity =
            Some("generating report for US:AAPL via test-model — esc to cancel…".into());
    } else if state_name == "live-research-error" {
        state.error = Some("report has no supported claims".into());
    }
    draw_research_live(&mut screen, &data, &state);
    screen
}

fn live_ask(
    state_name: &str,
    fixture: &serde_json::Value,
    db_path: &Path,
    w: usize,
    h: usize,
) -> Screen {
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
    match state_name {
        "live-ask-typing" => {
            state.history.clear();
            state.input = "What changed at Apple?".into();
            state.editing = true;
        }
        "live-ask-waiting" | "live-ask-error" => {
            state.history.clear();
            state.history.push(ChatMessage {
                role: "user".into(),
                text: "What changed at Apple?".into(),
                citations: vec![],
                source: "user".into(),
            });
            state.busy = state_name == "live-ask-waiting";
            if state_name == "live-ask-error" {
                state.error = Some("offline test — press i to try again".into());
            }
        }
        "live-ask-citation-selected" => state.walk_citation(1),
        "live-ask-confirm" => state.clear_pending = true,
        _ => {}
    }
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
    let decisions = if fixture["state"] == "live-decisions-empty" {
        Vec::new()
    } else {
        delta_services::list_decisions(&db, None, true).ok()?
    };
    let selected = if fixture["state"] == "live-decisions-review" {
        1
    } else {
        0
    };
    let (reviews, current_price) = if let Some(decision) = decisions.get(selected) {
        (
            delta_services::review_history(&db, &decision.id).ok()?,
            delta_services::recent_closes(&db, &decision.instrument_id, 1)
                .ok()?
                .last()
                .copied(),
        )
    } else {
        (Vec::new(), None)
    };
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
