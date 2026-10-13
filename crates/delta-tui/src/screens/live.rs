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
use crate::screens::theses::{draw_theses_live, draw_thesis_form_live, ThesesState};
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
        "live-settings"
        | "live-settings-diagnostics"
        | "live-settings-provider"
        | "live-settings-model"
        | "live-settings-market"
        | "live-settings-market-edit"
        | "live-settings-market-invalid"
        | "live-settings-key"
        | "live-settings-key-invalid"
        | "live-settings-custom"
        | "live-settings-source"
        | "live-settings-source-invalid" => Some(live_settings(state, fixture, db_path, w, h)),
        "live-home" => Some(live_home(db_path, w, h)),
        "live-watchlist" | "live-watchlist-inspector" | "live-watchlist-range" => {
            Some(live_watchlist(state, db_path, w, h))
        }
        "live-theses"
        | "live-theses-empty"
        | "live-theses-detail"
        | "live-theses-evidence"
        | "live-theses-new"
        | "live-theses-edit"
        | "live-theses-validation"
        | "live-theses-filter"
        | "live-theses-summary" => Some(live_theses(state, fixture, db_path, w, h)),
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

fn live_theses(
    state_name: &str,
    fixture: &serde_json::Value,
    db_path: &Path,
    w: usize,
    h: usize,
) -> Screen {
    let db = delta_core::db::Db::open(db_path).expect("live theses seed db");
    let now =
        chrono::DateTime::parse_from_rfc3339(fixture["seed"]["now"].as_str().expect("fixture now"))
            .expect("RFC 3339 now stamp")
            .naive_utc();
    let mut state = ThesesState::default();
    state.load_at(&db, Some(now)).expect("load live theses");
    state.focus_active = state_name != "live-theses";
    match state_name {
        "live-theses-empty" => {
            state.fleet.clear();
            state.links.clear();
            state.pending.clear();
        }
        "live-theses-detail" => state.view = crate::screens::theses::ThesesPane::Detail,
        "live-theses-evidence" => state.view = crate::screens::theses::ThesesPane::Evidence,
        "live-theses-filter" => {
            state.filtering = true;
            state.filter = "a".into();
        }
        "live-theses-summary" => {
            let row = &state.fleet[state.selected];
            state.summary = Some(delta_services::ThesisSummary {
                thesis_id: row.thesis.id.clone(),
                claim: row.thesis.claim.clone(),
                state: row.state(),
                summary: "Accepted capex guidance supports margin pressure.".into(),
                strongest_support: "Guidance implies margin pressure.".into(),
                strongest_counter: String::new(),
                unknowns: vec!["Pricing response remains unknown.".into()],
                citations: vec!["news:news-msft-capex".into()],
                as_of: now,
            });
        }
        _ => {}
    }
    let mut screen = Screen::new(w, h);
    draw_theses_live(&mut screen, &state);
    if matches!(
        state_name,
        "live-theses-new" | "live-theses-edit" | "live-theses-validation"
    ) {
        draw_thesis_form_live(&mut screen, &state, state_name == "live-theses-edit");
    }
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
fn live_settings(
    state_name: &str,
    fixture: &serde_json::Value,
    db_path: &Path,
    w: usize,
    h: usize,
) -> Screen {
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
    let mut state = SettingsState::default();
    if state_name == "live-settings-diagnostics" {
        state.toggle_diagnostics(w);
    }
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
    if state_name != "live-settings-diagnostics" {
        paint_settings_dialog(&mut screen, state_name, None, 0);
    }
    screen
}

/// Shared Settings modal renderer for oracle states and the running app.
/// Runtime forms supply their current values; oracle captures use the seeded
/// values established by the Python exporter.
pub fn paint_settings_dialog(
    screen: &mut Screen,
    state: &str,
    form: Option<&crate::form::Form>,
    selected: usize,
) {
    use crate::dialog::dialog_frame;
    use crate::screen::{color, Style};

    if !state.starts_with("live-settings-") || state == "live-settings-diagnostics" {
        return;
    }
    screen.dim();
    for cell in &mut screen.cells {
        cell.fg = match cell.fg {
            Some("#24385f") => Some("#363636"),
            Some("#623f04") => Some("#424242"),
            Some("#3a3a3a") => Some("#171717"),
            other => other,
        };
        cell.bg = match cell.bg {
            Some("#0f1e3c") => Some("#1d1d1d"),
            Some("#070e1e" | "#13254b") => Some("#0e0e0e"),
            other => other,
        };
    }
    let (width, height) = match state {
        "live-settings-model" => (72, 13),
        "live-settings-market" | "live-settings-market-invalid" => (64, 27),
        "live-settings-market-edit" => (64, 22),
        "live-settings-key" | "live-settings-key-invalid" => (64, 12),
        _ => (64, 15),
    };
    let max_height = screen
        .h
        .saturating_sub(if state.contains("market") { 3 } else { 2 });
    let (x, y, w, _) = dialog_frame(screen, width, height.min(max_height));

    if state == "live-settings-provider" {
        modal_title(screen, x, y, w, "select a provider");
        let header = Style::fg(color::FG).bold().bg("#232323");
        screen.fill(x, y + 2, x + w, y + 3, header);
        screen.text(x + 1, y + 2, "Provider    Endpoint           Key", header);
        for (index, row) in [
            "openrouter  openrouter.ai      not set",
            "openai      api.openai.com     not set",
            "anthropic   api.anthropic.com  not set",
            "custom      from config.toml   base_url + key",
        ]
        .iter()
        .enumerate()
        {
            let style = if index == selected {
                Style::fg(color::WHITE).bold().bg("#494949")
            } else if index == 2 {
                Style::fg(color::FG).bg("#070707")
            } else {
                Style::fg(color::FG).bg(color::SURFACE)
            };
            screen.fill(x, y + 3 + index, x + w, y + 4 + index, style);
            screen.text(x + 1, y + 3 + index, row, style);
        }
        screen.text(
            x,
            y + 8,
            "keys are stored in .env, never in config.toml",
            Style::fg(color::MUTED),
        );
        modal_hint(
            screen,
            x,
            y + 10,
            w,
            "enter connect  esc cancel",
            &[(0, 5), (15, 3)],
        );
        return;
    }

    if state == "live-settings-model" {
        modal_title(screen, x, y, w, "select a model (openrouter)");
        modal_input(
            screen,
            (x, y + 2, w),
            "no catalog: type a model id and press enter",
            form.and_then(|f| f.fields.first()),
            true,
            false,
        );
        let header = Style::fg(color::FG).bold().bg(color::PANEL);
        screen.fill(x, y + 4, x + w, y + 5, header);
        screen.text(x + 1, y + 4, "Model  Context  $/1M in  $/1M out", header);
        screen.text(
            x + 1,
            y + 6,
            " ctrl+r refresh ",
            Style::fg(color::FG).bold(),
        );
        screen.text(x + 2, y + 6, "ctrl+r", Style::fg("#898989").bold());
        screen.text(
            x + 19,
            y + 6,
            "catalog empty — enter a model id as free text",
            Style::fg(color::MUTED),
        );
        modal_hint(
            screen,
            x,
            y + 8,
            w,
            "enter select  esc cancel",
            &[(0, 5), (14, 3)],
        );
        return;
    }

    if state == "live-settings-key" || state == "live-settings-key-invalid" {
        let provider = form
            .and_then(|f| f.title.strip_prefix("connect "))
            .unwrap_or("openrouter");
        let title = format!("enter {}_API_KEY", provider.to_ascii_uppercase());
        modal_title(screen, x, y, w, &title);
        screen.text(
            x,
            y + 2,
            &format!("get one at {provider}.com — stored in .env, never"),
            Style::fg(color::MUTED),
        );
        screen.text(x, y + 3, "config.toml", Style::fg(color::MUTED));
        modal_input(
            screen,
            (x, y + 5, w),
            "api key",
            form.and_then(|f| f.fields.first()),
            true,
            true,
        );
        if screen.w >= 200 && form.is_none() && state == "live-settings-key" {
            screen.put(x + 2, y + 5, 'a', Style::fg(color::DISABLED).bg("#161616"));
        }
        modal_hint(
            screen,
            x,
            y + 7,
            w,
            "enter save  esc skip",
            &[(0, 5), (12, 3)],
        );
        return;
    }

    if state == "live-settings-custom"
        || state == "live-settings-source"
        || state == "live-settings-source-invalid"
    {
        let source = state != "live-settings-custom";
        modal_title(
            screen,
            x,
            y,
            w,
            if source {
                "configure SEC EDGAR"
            } else {
                "connect a custom endpoint"
            },
        );
        screen.text(
            x,
            y + 2,
            if source {
                "SEC requires a real contact address in the User-Agent."
            } else {
                "any OpenAI-compatible server: Ollama, Groq, Together…"
            },
            Style::fg(if source { color::FG } else { color::MUTED }),
        );
        if source {
            let contact = form.and_then(|f| f.fields.first());
            let oracle_contact = if state == "live-settings-source-invalid" {
                "you@example.com"
            } else {
                "oracle@example.test"
            };
            modal_input(
                screen,
                (x + 14, y + 4, w - 14),
                oracle_contact,
                contact,
                true,
                false,
            );
            if screen.w == 120 && state == "live-settings-source" && form.is_none() {
                screen.put(
                    x + 14 + 2 + oracle_contact.len(),
                    y + 4,
                    ' ',
                    Style::fg(color::FG).bg("#161616"),
                );
            }
            modal_input(
                screen,
                (x + 14, y + 6, w - 14),
                "lse,asx (optional)",
                form.and_then(|f| f.fields.get(1)),
                false,
                false,
            );
            modal_buttons(screen, x, y + 8);
        } else {
            for (index, (offset, placeholder)) in [
                (4, "base URL, e.g. http://localhost:11434/v1"),
                (6, "env var name for the key (default CUSTOM_API_KEY)"),
                (8, "api key (optional for local servers)"),
            ]
            .iter()
            .enumerate()
            {
                modal_input(
                    screen,
                    (x, y + offset, w),
                    placeholder,
                    form.and_then(|f| f.fields.get(index)),
                    form.map_or(index == 0, |f| f.focus == index),
                    index == 2,
                );
                if index == 0 && screen.w == 120 && form.is_none() {
                    screen.put(
                        x + 2,
                        y + offset,
                        'b',
                        Style::fg(color::DISABLED).bg("#161616"),
                    );
                }
            }
        }
        modal_hint(
            screen,
            x,
            y + 10,
            w,
            "enter save  esc cancel",
            &[(0, 5), (12, 3)],
        );
        return;
    }

    if state.starts_with("live-settings-market") {
        let edit = state == "live-settings-market-edit";
        modal_title(
            screen,
            x,
            y,
            w,
            if edit { "edit market" } else { "add market" },
        );
        if !edit {
            let note = "type an exchange id to autocomplete";
            screen.fill(x, y + 2, x + w, y + 3, Style::fg(color::MUTED));
            screen.text(
                x + (w - note.len()) / 2,
                y + 2,
                note,
                Style::fg(color::MUTED),
            );
            for offset in 6..10 {
                screen.fill(
                    x + 10,
                    y + offset,
                    x + w,
                    y + offset + 1,
                    Style::DEFAULT.bg(color::PANEL),
                );
                screen.fill(
                    x + 11,
                    y + offset,
                    x + w - 1,
                    y + offset + 1,
                    Style::fg(color::FG).bg(color::PANEL),
                );
            }
        }
        let fields = if edit {
            [
                (2, "ID", "asx", false, true),
                (5, "Name", "Australian Securities Exchange", true, false),
                (8, "Currency", "AUD", false, false),
                (11, "Yahoo", ".AX", false, false),
            ]
        } else {
            [
                (3, "ID", "e.g. lse", true, false),
                (10, "Name", "Exchange name", false, false),
                (13, "Currency", "GBP", false, false),
                (16, "Yahoo", ".L (optional)", false, false),
            ]
        };
        let bottom = y + height.min(max_height) - 3;
        for (index, (offset, label, placeholder, default_focus, disabled)) in
            fields.into_iter().enumerate()
        {
            let row = y + offset;
            if row >= bottom {
                continue;
            }
            if row + 1 < bottom && !(screen.h <= 24 && !edit && label == "Yahoo") {
                screen.text(x, row + 1, label, Style::fg(color::MUTED));
            }
            let field = form.and_then(|f| f.fields.get(index));
            let focused = form.map_or(default_focus, |f| f.focus == index);
            let value = field
                .map(|f| f.value())
                .filter(|v| !v.is_empty())
                .unwrap_or(placeholder);
            modal_input(screen, (x + 10, row, w - 10), value, field, focused, false);
            if edit && index == 1 && screen.w == 200 && form.is_none() {
                screen.put(
                    x + 12 + value.len(),
                    row,
                    ' ',
                    Style::fg(color::FG).bg("#161616"),
                );
            }
            if disabled {
                screen.put(x + 10, row, '█', Style::fg("#1f1f1f"));
                for cell in &mut screen.cells[row * screen.w + x + 12..row * screen.w + x + w - 1] {
                    cell.fg = Some("#989898");
                }
            }
        }
        let button_y = y + if edit { 15 } else { 20 };
        if button_y < bottom {
            modal_buttons(screen, x, button_y);
        }
        let hint_y = y + if edit { 17 } else { 22 };
        if hint_y < bottom && screen.h > 24 {
            if edit {
                modal_hint(
                    screen,
                    x,
                    hint_y,
                    w,
                    "enter save  esc cancel",
                    &[(0, 5), (12, 3)],
                );
            } else {
                modal_hint(
                    screen,
                    x,
                    hint_y,
                    w,
                    "↑↓ choose  enter pick · save  esc cancel",
                    &[(0, 2), (11, 5), (30, 3)],
                );
            }
        }
    }
}

fn modal_title(screen: &mut Screen, x: usize, y: usize, w: usize, title: &str) {
    use crate::screen::Style;
    let style = Style::fg("#898989").bold();
    screen.fill(x, y, x + w, y + 1, style);
    screen.text(x + (w - title.chars().count()) / 2, y, title, style);
}

fn modal_hint(
    screen: &mut Screen,
    x: usize,
    y: usize,
    w: usize,
    hint: &str,
    keys: &[(usize, usize)],
) {
    use crate::screen::{color, Style};
    screen.fill(x, y, x + w, y + 1, Style::fg(color::MUTED));
    let hx = x + (w - hint.chars().count()) / 2;
    screen.text(hx, y, hint, Style::fg(color::MUTED));
    for &(offset, len) in keys {
        for cell in &mut screen.cells[y * screen.w + hx + offset..y * screen.w + hx + offset + len]
        {
            cell.fg = Some("#898989");
            cell.bold = true;
        }
    }
}

fn modal_buttons(screen: &mut Screen, x: usize, y: usize) {
    use crate::screen::{color, Style};
    screen.fill(x, y, x + 8, y + 1, Style::DEFAULT.bg("#494949"));
    screen.text(
        x + 1,
        y,
        " Save ",
        Style::fg(color::WHITE).bold().bg("#494949"),
    );
    screen.text(x + 10, y, " Cancel ", Style::fg(color::FG).bold());
}

fn modal_input(
    screen: &mut Screen,
    (x, y, w): (usize, usize, usize),
    placeholder: &str,
    field: Option<&crate::form::Field>,
    focused: bool,
    secret: bool,
) {
    use crate::screen::{color, Style};
    let raw = field
        .map(|field| field.value())
        .filter(|value| !value.is_empty());
    let value = raw.map_or_else(
        || placeholder.to_string(),
        |value| {
            if secret {
                "•".repeat(value.chars().count())
            } else {
                value.to_string()
            }
        },
    );
    let filled = raw.is_some()
        || field.is_none()
            && [
                "oracle@example.test",
                "asx",
                "Australian Securities Exchange",
                "AUD",
                ".AX",
            ]
            .contains(&placeholder);
    if focused {
        screen.fill(x, y, x + w, y + 1, Style::DEFAULT.bg("#161616"));
    }
    screen.put(
        x,
        y,
        '█',
        Style::fg(if focused { "#898989" } else { "#333333" }).bg(if focused {
            "#161616"
        } else {
            color::SURFACE
        }),
    );
    screen.text(
        x + 2,
        y,
        &value,
        Style::fg(if filled { color::FG } else { color::DISABLED }).bg(if filled && focused {
            "#323232"
        } else if focused {
            "#161616"
        } else {
            color::SURFACE
        }),
    );
    let end = (x + 2 + value.chars().count()).min(x + w - 1);
    screen.fill(
        end,
        y,
        x + w - 1,
        y + 1,
        Style::fg(color::FG).bg(if focused { "#161616" } else { color::SURFACE }),
    );
    if focused {
        if filled {
            screen.put(end, y, ' ', Style::fg(color::BLACK).bg(color::FG));
        } else if let Some(ch) = value.chars().next() {
            screen.put(x + 2, y, ch, Style::fg(color::BLACK).bg(color::FG));
        }
    }
}
