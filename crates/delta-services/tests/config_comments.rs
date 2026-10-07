//! D6 (finding rust-core #3): every `update_config` write path keeps the
//! comments and key order of the surrounding config.toml. Python's writer
//! rewrote the whole file (`tomli_w`); the Rust port edits in place with
//! `toml_edit`, so each screen-facing config operation must round-trip the
//! user's comments intact.

use std::path::Path;

use delta_llm::catalog::{
    set_llm_custom, set_llm_model, set_llm_provider, set_llm_route, set_plugin_model,
};
use delta_services::config_ops::{
    add_target, remove_market, remove_target, save_market, set_plugin_enabled,
};

const COMMENTED: &str = "\
# Delta configuration. Hand-edits welcome; the app preserves your comments.

# The reporting currency for every portfolio figure.
base_currency = \"AUD\"

[llm]
# Which provider serves the default route.
provider = \"openrouter\"
model = \"openai/gpt-4o-mini\"

[plugins.sec_edgar]
# SEC User-Agent rule: a real contact is required.
contact = \"a@b.c\"
";

fn write_commented(path: &Path) {
    std::fs::write(path, COMMENTED).unwrap();
}

fn assert_comments_intact(path: &Path) {
    let text = std::fs::read_to_string(path).unwrap();
    assert!(text.contains("# The reporting currency for every portfolio figure."));
    assert!(text.contains("# Which provider serves the default route."));
    assert!(text.contains("# SEC User-Agent rule: a real contact is required."));
    // Key order of the untouched sections survives.
    let base = text.find("base_currency").unwrap();
    let llm = text.find("[llm]").unwrap();
    let sec = text.find("[plugins.sec_edgar]").unwrap();
    assert!(base < llm && llm < sec);
}

fn tmp_config(tag: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(format!("{tag}.toml"));
    write_commented(&path);
    (dir, path)
}

#[test]
fn llm_route_path_preserves_comments() {
    let (_dir, path) = tmp_config("route");
    set_llm_route(&path, "report", "anthropic/claude").unwrap();
    assert_comments_intact(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("anthropic/claude"));
    // The new [llm.routing] section lands inside [llm], before [plugins].
    let routing = text.find("[llm.routing]").unwrap();
    assert!(
        text.find("[llm]").unwrap() < routing
            && routing < text.find("[plugins.sec_edgar]").unwrap()
    );
}

#[test]
fn llm_model_and_provider_paths_preserve_comments() {
    let (_dir, path) = tmp_config("model");
    set_llm_model(&path, "openai/gpt-4o").unwrap();
    set_llm_provider(&path, "openai").unwrap();
    assert_comments_intact(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("model = \"openai/gpt-4o\""));
    assert!(text.contains("provider = \"openai\""));

    let (_dir, path) = tmp_config("custom");
    set_llm_custom(&path, "http://localhost:11434/v1", "CUSTOM_API_KEY").unwrap();
    assert_comments_intact(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("http://localhost:11434/v1"));
    assert!(text.contains("api_key_env = \"CUSTOM_API_KEY\""));
}

#[test]
fn plugin_model_and_enabled_paths_preserve_comments() {
    let (_dir, path) = tmp_config("plugin_model");
    set_plugin_model(&path, "sec_edgar", Some("openai/gpt-4.1")).unwrap();
    assert_comments_intact(&path);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("model = \"openai/gpt-4.1\""));
    set_plugin_model(&path, "sec_edgar", None).unwrap();
    assert_comments_intact(&path);
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("openai/gpt-4.1"));

    let (_dir, path) = tmp_config("plugin_enabled");
    set_plugin_enabled(&path, "sec_edgar", false).unwrap();
    assert_comments_intact(&path);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("enabled = false"));
}

#[test]
fn market_paths_preserve_comments() {
    let (_dir, path) = tmp_config("market");
    save_market(&path, "nzx", "New Zealand", "NZD", ".NZ").unwrap();
    assert_comments_intact(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[markets.nzx]"));
    assert!(text.contains("currency = \"NZD\""));
    // save_market edits on an existing [markets] table keep their shape.
    save_market(&path, "nzx", "New Zealand Exchange", "NZD", ".NZ").unwrap();
    assert_comments_intact(&path);
    assert!(std::fs::read_to_string(&path)
        .unwrap()
        .contains("New Zealand Exchange"));

    remove_market(&path, "nzx").unwrap();
    assert_comments_intact(&path);
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("[markets.nzx]"));
}

#[test]
fn target_paths_preserve_comments() {
    let (_dir, path) = tmp_config("target");
    let tickers = vec!["AAPL".to_string()];
    add_target(
        &path,
        "chip",
        "company",
        "us",
        &tickers,
        &["ai".to_string()],
        "watch the cycle",
        Some("Chip cycle"),
        "equity",
    )
    .unwrap();
    assert_comments_intact(&path);
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.contains("[targets.chip]"));
    assert!(text.contains("\"AAPL\""));

    remove_target(&path, "chip").unwrap();
    assert_comments_intact(&path);
    assert!(!std::fs::read_to_string(&path)
        .unwrap()
        .contains("[targets.chip]"));
}

/// Resolve a Python interpreter for the parity check: `DELTA_TEST_PYTHON`
/// wins, then `uv run python` (the repo's dev setup), then bare `python3`
/// (CI runners ship Python without `uv` on the cargo job's PATH).
fn python_command(script: &str) -> (std::process::Command, String) {
    let on_path = |name: &str| {
        std::env::var_os("PATH")
            .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(name).exists()))
            .unwrap_or(false)
    };
    if let Ok(py) = std::env::var("DELTA_TEST_PYTHON") {
        let mut cmd = std::process::Command::new(&py);
        cmd.arg("-c").arg(script);
        return (cmd, py);
    }
    if on_path("uv") {
        let mut cmd = std::process::Command::new("uv");
        cmd.args(["run", "python", "-c", script]);
        return (cmd, "uv run python".to_string());
    }
    let mut cmd = std::process::Command::new("python3");
    cmd.arg("-c").arg(script);
    (cmd, "python3".to_string())
}

/// The written file must stay valid TOML for the Python app: parse it with
/// the stdlib `tomllib` Python 3.12's `delta.core.config` reads with.
#[test]
fn written_config_parses_in_python() {
    let (_dir, path) = tmp_config("python_check");
    set_llm_route(&path, "report", "anthropic/claude").unwrap();
    set_plugin_enabled(&path, "sec_edgar", false).unwrap();
    add_target(
        &path,
        "chip",
        "company",
        "us",
        &["AAPL".to_string()],
        &[],
        "",
        None,
        "equity",
    )
    .unwrap();
    let (mut command, label) = python_command(&format!(
        "import tomllib, json\nwith open({:?}, 'rb') as f:\n    data = tomllib.load(f)\nprint(json.dumps(data))",
        path
    ));
    let output = command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap_or_else(|e| panic!("{label} not runnable: {e}"));
    assert!(
        output.status.success(),
        "python failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("anthropic/claude"));
    assert!(stdout.contains("\"chip\""));
    assert!(stdout.contains("\"enabled\": false"));
}
