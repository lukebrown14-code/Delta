//! Port of the service parts of `tests/test_provider_setup.py` and
//! `tests/test_data_sources.py` (the `.env`/`config.toml` writebacks and the
//! provider setup contract), plus `test_services.py`'s `setup_checks` case.
//! The modal/picker cases are the settings stream's.

use std::collections::{BTreeMap, BTreeSet};

use chrono::NaiveDateTime;
use delta_core::config::{load_toml, read_env_value_named, AppConfig};
use delta_core::db::{Db, StoreItem};
use delta_core::models::Bar;
use delta_plugins::{DataProviderField, DataProviderSpec};
use delta_services::config_ops::{remove_market, save_market};
use delta_services::error::ServiceError;
use delta_services::setup::{configure_data_provider, data_provider_status, setup_checks};

/// `LicensedSource` from `test_data_sources.py`.
static LICENSED_SPEC: DataProviderSpec = DataProviderSpec {
    label: "Licensed source",
    fields: &[
        DataProviderField {
            name: "endpoint",
            label: "Endpoint",
            required: true,
            secret: false,
            env_var: "",
            placeholder: "",
        },
        DataProviderField {
            name: "api_key",
            label: "API key",
            required: true,
            secret: true,
            env_var: "FT_API_KEY",
            placeholder: "",
        },
    ],
    primary_disclosure: false,
    notice: "",
};

fn licensed_specs() -> BTreeMap<String, &'static DataProviderSpec> {
    BTreeMap::from([("licensed".to_string(), &LICENSED_SPEC)])
}

#[track_caller]
fn assert_invalid<T>(result: Result<T, ServiceError>, needle: &str) {
    match result {
        Ok(_) => panic!("expected error containing {needle:?}"),
        Err(ServiceError::Invalid { message }) => assert!(
            message.contains(needle),
            "message {message:?} does not contain {needle:?}"
        ),
        Err(other) => panic!("unexpected error kind: {other}"),
    }
}

/// The `[plugins]` table of a raw config, typed for `AppConfig`.
fn plugins_of(raw: &BTreeMap<String, serde_json::Value>) -> BTreeMap<String, serde_json::Value> {
    raw.get("plugins")
        .and_then(serde_json::Value::as_object)
        .map(|map| map.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

fn values(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// `test_licensed_source_persists_key_only_in_dotenv`.
#[test]
fn licensed_source_persists_key_only_in_dotenv() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let env = dir.path().join(".env");
    save_market(&config, "lse", "London", "GBP", ".L").unwrap();
    configure_data_provider(
        &config,
        &env,
        &licensed_specs(),
        "licensed",
        &values(&[
            ("endpoint", "https://api.example.test/v1"),
            ("api_key", "secret-value"),
        ]),
        Some(&["lse".to_string()]),
    )
    .unwrap();

    let text = std::fs::read_to_string(&config).unwrap();
    assert!(text.contains("https://api.example.test/v1"));
    assert!(!text.contains("secret-value"));
    // Structurally: the market scope landed under the plugin's scope table.
    let raw = load_toml(&config).unwrap();
    assert_eq!(
        raw["plugins"]["licensed"]["scope"]["markets"],
        serde_json::json!(["lse"])
    );
    assert_eq!(
        raw["plugins"]["licensed"]["endpoint"],
        "https://api.example.test/v1"
    );
    assert_eq!(
        raw["plugins"]["licensed"]["enabled"],
        serde_json::json!(true)
    );
    // The secret went to .env, never to config.toml.
    assert_eq!(read_env_value_named("FT_API_KEY", &env), "secret-value");

    // A market still referenced by a configured source cannot be removed
    // (the dependency check reads the persisted config).
    let err = remove_market(&config, "lse").unwrap_err();
    assert!(err.to_string().contains("licensed"), "{err}");
}

/// `test_licensed_source_rejects_missing_required_key`.
#[test]
fn licensed_source_rejects_missing_required_key() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let env = dir.path().join(".env");
    let result = configure_data_provider(
        &config,
        &env,
        &licensed_specs(),
        "licensed",
        &values(&[("endpoint", "https://api.example.test"), ("api_key", "")]),
        None,
    );
    assert_invalid(result, "API key is required");
}

#[test]
fn unknown_provider_and_settings_are_rejected() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let env = dir.path().join(".env");
    assert_invalid(
        configure_data_provider(&config, &env, &licensed_specs(), "nope", &values(&[]), None),
        "unknown configurable data provider: nope",
    );
    assert_invalid(
        configure_data_provider(
            &config,
            &env,
            &licensed_specs(),
            "licensed",
            &values(&[("mystery", "x")]),
            None,
        ),
        "unknown settings for licensed: mystery",
    );
    assert_invalid(
        configure_data_provider(
            &config,
            &env,
            &licensed_specs(),
            "licensed",
            &values(&[("endpoint", "https://x"), ("api_key", "k")]),
            Some(&["nowhere".to_string()]),
        ),
        "unknown markets: nowhere",
    );
}

/// A secret whose spec forgot its env var cannot be accepted: it must never
/// land in config.toml.
#[test]
fn secret_without_env_var_is_rejected() {
    static BAD_SPEC: DataProviderSpec = DataProviderSpec {
        label: "Broken source",
        fields: &[DataProviderField {
            name: "key",
            label: "API key",
            required: true,
            secret: true,
            env_var: "",
            placeholder: "",
        }],
        primary_disclosure: false,
        notice: "",
    };
    let specs = BTreeMap::from([("broken".to_string(), &BAD_SPEC)]);
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let env = dir.path().join(".env");
    assert_invalid(
        configure_data_provider(
            &config,
            &env,
            &specs,
            "broken",
            &values(&[("key", "k")]),
            None,
        ),
        "has no declared environment variable",
    );
}

/// `data_provider_status` over the licensed spec: configured = required
/// fields present (secrets read .env), enabled from the config table.
#[test]
fn data_provider_status_reads_config_and_env() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let env = dir.path().join(".env");

    // Unconfigured: nothing in config, nothing in .env.
    let statuses = data_provider_status(&AppConfig::default(), &env, &licensed_specs());
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].name, "licensed");
    assert_eq!(statuses[0].label, "Licensed source");
    assert!(!statuses[0].configured);
    assert!(statuses[0].enabled);

    // The real registry's only spec today: sec_edgar, configured through the
    // contact field, and primary disclosure.
    let specs = delta_plugins::provider_specs();
    let statuses = data_provider_status(&AppConfig::default(), &env, &specs);
    assert_eq!(statuses[0].name, "sec_edgar");
    assert_eq!(statuses[0].label, "SEC EDGAR");
    assert!(!statuses[0].configured);
    assert!(statuses[0].primary_disclosure);
    assert!(statuses[0].notice.contains("SEC requires"));

    // Configure it; the contact lands in config.toml and configured flips.
    configure_data_provider(
        &config,
        &env,
        &licensed_specs(),
        "licensed",
        &values(&[
            ("endpoint", "https://api.example.test/v1"),
            ("api_key", "secret-value"),
        ]),
        None,
    )
    .unwrap();
    let raw = load_toml(&config).unwrap();
    let cfg = AppConfig {
        plugins: plugins_of(&raw),
        ..AppConfig::default()
    };
    let statuses = data_provider_status(&cfg, &env, &licensed_specs());
    assert!(statuses[0].configured);
    assert!(statuses[0].enabled);
}

/// `test_setup_checks_flag_missing_key` (test_services.py): no key, no
/// contact, but price history present.
#[test]
fn setup_checks_flag_missing_key() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let env = dir.path().join(".env");
    std::env::remove_var("OPENROUTER_API_KEY");
    let mut db = Db::open_memory().unwrap();
    // Price history: one recent bar for one instrument.
    db.store_items(&[StoreItem::Bar(Bar {
        instrument_id: "US:AAPL".to_string(),
        ts: NaiveDateTime::parse_from_str("2026-09-21 00:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
        open: 1.0,
        high: 1.0,
        low: 1.0,
        close: 1.0,
        volume: 1.0,
        source: "test".to_string(),
    })])
    .unwrap();
    let cfg = AppConfig {
        llm_provider: "openrouter".to_string(),
        plugins: BTreeMap::from([("sec_edgar".to_string(), serde_json::json!({}))]),
        ..AppConfig::default()
    };

    let checks = setup_checks(
        &delta_core::config::Settings::default(),
        &cfg,
        &db,
        &config,
        &env,
    )
    .unwrap();
    let by_name: BTreeMap<&str, &delta_services::setup::Check> =
        checks.iter().map(|c| (c.name.as_str(), c)).collect();

    assert!(!by_name["LLM provider (openrouter)"].ok);
    assert!(by_name["LLM provider (openrouter)"]
        .fix
        .contains("OPENROUTER_API_KEY"));
    assert!(by_name["Price history"].ok);
    assert!(!by_name["SEC EDGAR contact"].ok);
    assert!(by_name["Database"].ok);
    // No config.toml in the bare temp dir.
    assert!(!by_name["Config file"].ok);
}

/// The full happy path: key in Settings, contact configured, config file
/// present.
#[test]
fn setup_checks_go_green_when_configured() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(
        &config,
        "[llm]\nprovider = \"openrouter\"\n[plugins.sec_edgar]\ncontact = \"a@b.c\"\n",
    )
    .unwrap();
    let env = dir.path().join(".env");
    std::env::remove_var("OPENROUTER_API_KEY");
    std::fs::write(&env, "OPENROUTER_API_KEY=sk-or\n").unwrap();
    let db = Db::open_memory().unwrap();
    let raw = load_toml(&config).unwrap();
    let cfg = AppConfig {
        llm_provider: "openrouter".to_string(),
        plugins: plugins_of(&raw),
        ..AppConfig::default()
    };

    let settings = delta_core::config::load_settings(&env);
    let checks = setup_checks(&settings, &cfg, &db, &config, &env).unwrap();
    let by_name: BTreeSet<&str> = checks
        .iter()
        .filter(|c| c.ok)
        .map(|c| c.name.as_str())
        .collect();
    assert!(by_name.contains("LLM provider (openrouter)"));
    assert!(by_name.contains("Config file"));
    assert!(by_name.contains("Database"));
    assert!(by_name.contains("SEC EDGAR contact"));
    // Empty in-memory db: no bars yet.
    assert!(!by_name.contains("Price history"));
}

/// An unknown provider name degrades into a loud, named check.
#[test]
fn setup_checks_report_unknown_provider() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let env = dir.path().join(".env");
    let db = Db::open_memory().unwrap();
    let cfg = AppConfig {
        llm_provider: "litellm".to_string(),
        ..AppConfig::default()
    };
    let checks = setup_checks(
        &delta_core::config::Settings::default(),
        &cfg,
        &db,
        &config,
        &env,
    )
    .unwrap();
    assert_eq!(checks.len(), 5);
    assert_eq!(checks[0].name, "LLM provider (litellm)");
    assert!(!checks[0].ok);
    assert!(checks[0]
        .fix
        .contains("anthropic, custom, openai, openrouter"));
}
