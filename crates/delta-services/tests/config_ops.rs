//! Config-operation tests (`set_plugin_enabled`, markets, targets), checked
//! against the Python behaviours they port.

use delta_services::{
    add_target, market_profiles, remove_market, remove_target, save_market, set_plugin_enabled,
    target_specs,
};

fn temp_config(contents: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config.toml");
    std::fs::write(&path, contents).unwrap();
    (dir, path)
}

#[test]
fn set_plugin_enabled_writes_toml() {
    let (_dir, path) = temp_config("[plugins.rss]\nfeeds = [\"https://x\"]\n");
    set_plugin_enabled(&path, "rss", false).unwrap();
    let raw = delta_core::config::load_toml(&path).unwrap();
    assert_eq!(raw["plugins"]["rss"]["enabled"], false);
    // Feeds survive: only `enabled` changes.
    assert_eq!(raw["plugins"]["rss"]["feeds"][0], "https://x");
}

#[test]
fn market_profiles_include_defaults_and_config() {
    let (_dir, path) = temp_config("[markets.gb]\nlabel = \"London\"\ncurrency = \"GBP\"\n");
    let profiles = market_profiles(&path).unwrap();
    assert_eq!(profiles["us"].currency, "USD");
    assert_eq!(profiles["asx"].yahoo_suffix, ".AX");
    assert_eq!(profiles["gb"].currency, "GBP");
}

#[test]
fn save_market_validates_and_edits() {
    let (_dir, path) = temp_config("");
    // Validation: id shape, label, currency.
    assert!(save_market(&path, "Bad ID", "x", "USD", "").is_err());
    assert!(save_market(&path, "gb", "", "GBP", "").is_err());
    assert!(save_market(&path, "gb", "London", "12", "").is_err());
    // Built-ins cannot be edited unless already config-backed.
    assert!(save_market(&path, "us", "United States", "USD", "").is_err());
    save_market(&path, "gb", "London", "gpb ", " .L ").unwrap();
    let raw = delta_core::config::load_toml(&path).unwrap();
    // Currency is trimmed and uppercased on save.
    assert_eq!(raw["markets"]["gb"]["currency"], "GPB");
    assert_eq!(raw["markets"]["gb"]["yahoo_suffix"], ".L");
}

#[test]
fn remove_market_blocks_dependencies() {
    let (_dir, path) = temp_config(
        "[markets.gb]\nlabel = \"London\"\ncurrency = \"GBP\"\n\
         [targets.miners]\nkind = \"company\"\nmarket = \"gb\"\ntickers = [\"RIO\"]\n",
    );
    let err = remove_market(&path, "gb").unwrap_err().to_string();
    assert!(err.contains("still used by: miners"), "{err}");
    // Removing the dependency frees the market.
    remove_target(&path, "miners").unwrap();
    remove_market(&path, "gb").unwrap();
    let raw = delta_core::config::load_toml(&path).unwrap();
    assert!(raw
        .get("markets")
        .map(|m| m.get("gb").is_none())
        .unwrap_or(true));
    // Built-in-only file: nothing to remove.
    let (_dir2, path2) = temp_config("");
    assert!(remove_market(&path2, "gb").is_err());
}

#[test]
fn target_specs_merge_legacy_tables() {
    let (_dir, path) = temp_config(
        "[targets.quality]\nmarket = \"us\"\ntickers = [\"AAPL\"]\ntags = [\"core\"]\n\
         [watchlists.legacy]\ntickers = [\"BHP\", \"RIO\"]\n",
    );
    let specs = target_specs(&path).unwrap();
    let quality = specs.get("quality").unwrap();
    assert_eq!(quality.kind, "company");
    assert_eq!(quality.markets, vec!["us".to_string()]);
    assert_eq!(quality.tags, ["core".to_string()].into_iter().collect());
    // Legacy table with two tickers models as a theme.
    let legacy = specs.get("legacy").unwrap();
    assert_eq!(legacy.kind, "theme");
    assert_eq!(legacy.tickers, vec!["BHP".to_string(), "RIO".to_string()]);
}

#[test]
fn add_target_validates_and_persists() {
    let (_dir, path) = temp_config("");
    assert!(add_target(
        &path,
        "t",
        "nope",
        "us",
        &["AAPL".to_string()],
        &[],
        "",
        None,
        "equity"
    )
    .is_err());
    assert!(add_target(
        &path,
        "t",
        "company",
        "mars",
        &["AAPL".to_string()],
        &[],
        "",
        None,
        "equity"
    )
    .is_err());
    assert!(add_target(&path, "t", "company", "us", &[], &[], "", None, "equity").is_err());
    assert!(add_target(
        &path,
        "m",
        "market",
        "us",
        &["AAPL".to_string()],
        &[],
        "",
        None,
        "equity"
    )
    .is_err());
    assert!(add_target(
        &path,
        "t",
        "company",
        "us",
        &["AAPL".to_string()],
        &[],
        "",
        None,
        "spark"
    )
    .is_err());
    add_target(
        &path,
        "t",
        "company",
        "us",
        &["aapl".to_string()],
        &["core".to_string()],
        "note",
        Some("Label"),
        "equity",
    )
    .unwrap();
    assert!(add_target(
        &path,
        "t",
        "company",
        "us",
        &["AAPL".to_string()],
        &[],
        "",
        None,
        "equity"
    )
    .is_err());
    let specs = target_specs(&path).unwrap();
    assert_eq!(specs["t"].tickers, vec!["AAPL".to_string()]);
    assert_eq!(specs["t"].name, "Label");
    assert_eq!(specs["t"].notes, "note");
    remove_target(&path, "t").unwrap();
    assert!(!target_specs(&path).unwrap().contains_key("t"));
    assert!(remove_target(&path, "ghost").is_err());
}

#[test]
fn update_target_keeps_other_saved_details() {
    let (_dir, path) = temp_config(
        "# keep\n[targets.chips]\nkind = \"company\"\nmarket = \"us\"\n\
         tickers = [\"NVDA\"]\ntags = [\"watch\"]\nnotes = \"Long view\"\n",
    );
    delta_services::config_ops::update_target(
        &path,
        "chips",
        "theme",
        "asx",
        &["bhp".to_string(), "rio".to_string()],
    )
    .unwrap();
    let specs = target_specs(&path).unwrap();
    assert_eq!(specs["chips"].kind, "theme");
    assert_eq!(specs["chips"].markets, ["asx"]);
    assert_eq!(specs["chips"].tickers, ["BHP", "RIO"]);
    assert_eq!(specs["chips"].notes, "Long view");
    assert!(specs["chips"].tags.contains("watch"));
    assert!(std::fs::read_to_string(&path).unwrap().contains("# keep"));
}
