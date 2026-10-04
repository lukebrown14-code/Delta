//! Populated Settings screen oracle exported by the canonical Python app.
use chrono::Duration;
use delta_core::config::AppConfig;
use delta_services::DataHealth;
use delta_tui::footer::FooterState;
use delta_tui::screen::Screen;
use delta_tui::settings_view::{SettingsDiagnostics, SettingsSource, SettingsState, SettingsView};
use serde_json::Value;

fn fixture(width: usize, height: usize) -> Value {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../fixtures/golden_screens/live-settings-{width}x{height}.json"
    ));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}
fn render(seed: &Value, width: usize, height: usize) -> Screen {
    let config = AppConfig {
        llm_provider: seed["config"]["llm"]["provider"].as_str().unwrap().into(),
        llm_model: seed["config"]["llm"]["model"].as_str().unwrap().into(),
        plugins: serde_json::from_value(seed["config"]["plugins"].clone()).unwrap(),
        markets: serde_json::from_value(seed["config"]["markets"].clone()).unwrap(),
        ..AppConfig::default()
    };
    let now = chrono::DateTime::parse_from_rfc3339(seed["now"].as_str().unwrap())
        .unwrap()
        .naive_utc();
    let start = chrono::DateTime::parse_from_rfc3339(seed["bars"]["start"].as_str().unwrap())
        .unwrap()
        .naive_utc();
    let latest = start + Duration::days(seed["bars"]["count"].as_i64().unwrap() - 1);
    let health = DataHealth {
        counts: [
            ("bar", 80),
            ("event", 1),
            ("fundamental", 0),
            ("llmcall", 0),
            ("newsitem", 1),
        ]
        .into_iter()
        .map(|(key, val)| (key.to_string(), val))
        .collect(),
        latest_bar: [(seed["instrument"]["id"].as_str().unwrap().into(), latest)]
            .into_iter()
            .collect(),
        last_llm: None,
    };
    let diagnostics = Ok(SettingsDiagnostics {
        health,
        costs: vec![],
        today_usd: 0.0,
        database_size: "delta.db · 4 KB".into(),
        refreshed: now.format("%H:%M:%S").to_string(),
    });
    let sources = vec![SettingsSource {
        id: "sec_edgar".into(),
        label: "SEC EDGAR".into(),
        status: "ready".into(),
    }];
    let state = SettingsState::default();
    let mut screen = Screen::new(width, height);
    SettingsView {
        config: &config,
        state: &state,
        sources: &sources,
        diagnostics: &diagnostics,
        provider_connected: true,
    }
    .paint(&mut screen);
    FooterState {
        latest_bar: Some(latest),
        spend: 0.0,
        provider: config.llm_provider,
        unavailable: false,
    }
    .paint_at(&mut screen, "Settings", "", now);
    screen
}
#[test]
fn populated_settings_matches_python_cells_at_all_sizes() {
    for (width, height) in [(80, 24), (120, 40), (200, 50)] {
        let golden = fixture(width, height);
        let screen = render(&golden["seed"], width, height);
        let mut mismatches = Vec::new();
        for (y, row) in golden["rows"].as_array().unwrap().iter().enumerate() {
            for (x, expected) in row.as_array().unwrap().iter().enumerate() {
                let got = &screen.cells[y * screen.w + x];
                let expected_attrs = expected["attrs"].as_array().unwrap();
                let has = |name: &str| {
                    expected_attrs
                        .iter()
                        .any(|attr| attr.as_str() == Some(name))
                };
                let bold = has("bold");
                let reverse = has("reverse");
                let italic = has("italic");
                let underline = has("underline");
                let unsupported: Vec<_> = expected_attrs
                    .iter()
                    .filter(|attr| {
                        !["bold", "reverse", "italic", "underline"]
                            .contains(&attr.as_str().unwrap_or(""))
                    })
                    .collect();
                if got.symbol != expected["ch"].as_str().unwrap()
                    || got.fg != expected["fg"].as_str()
                    || got.bg != expected["bg"].as_str()
                    || got.bold != bold
                    || got.reverse != reverse
                    || got.italic != italic
                    || got.underline != underline
                    || !unsupported.is_empty()
                {
                    mismatches.push(format!("({x},{y}) expected {} {:?}/{:?} attrs={expected_attrs:?} unsupported={unsupported:?}; got {:?} {:?}/{:?} attrs=({},{},{},{})",expected["ch"],expected["fg"],expected["bg"],got.symbol,got.fg,got.bg,got.bold,got.reverse,got.italic,got.underline));
                }
            }
        }
        assert!(
            mismatches.is_empty(),
            "{width}x{height}: {} mismatched cells:\n{}",
            mismatches.len(),
            mismatches
                .iter()
                .take(35)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}
