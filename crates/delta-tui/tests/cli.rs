//! Headless CLI smoke tests: run the `delta` binary against a temp
//! config/DB with wiremock-served sources and an LLM pointed at a local
//! server — never the network (`docs/rewrite/tasks/r3-cli.md`).

use std::path::Path;
use std::process::{Command, Output};

/// The binary under test (never the developer's installed `delta`).
fn bin() -> Command {
    Command::new(env!("CARGO_BIN_EXE_delta"))
}

fn run_in(dir: &Path, args: &[&str]) -> Output {
    bin().args(args).current_dir(dir).output().expect("spawn")
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).to_string()
}

fn stderr(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).to_string()
}

/// A temp workspace: `config.toml` + `.env` + a copy of the shared seed DB
/// (decisions due 2026-09-21 and 2026-10-27), so the headless commands run
/// against real stored state offline.
struct Workspace {
    dir: tempfile::TempDir,
}

impl Workspace {
    fn seed() -> Workspace {
        let ws = Workspace::empty();
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        std::fs::copy(
            root.join("../../fixtures/golden_seed.db"),
            ws.path().join("seed.db"),
        )
        .unwrap();
        ws
    }

    fn empty() -> Workspace {
        Workspace {
            dir: tempfile::tempdir().unwrap(),
        }
    }

    fn path(&self) -> &Path {
        self.dir.path()
    }

    /// Write `config.toml` with the given body (the schema the TUI reads).
    fn config(&self, body: &str) {
        std::fs::write(self.path().join("config.toml"), body).unwrap();
    }

    fn env(&self, name: &str, value: &str) {
        std::fs::write(self.path().join(".env"), format!("{name}={value}\n")).unwrap();
    }
}

#[test]
fn version_prints_and_exits_zero_without_a_terminal() {
    let temp = tempfile::tempdir().unwrap();
    let out = run_in(temp.path(), &["--version"]);
    assert!(out.status.success());
    assert_eq!(
        stdout(&out),
        format!("delta {}\n", env!("CARGO_PKG_VERSION"))
    );
}

#[test]
fn unknown_command_prints_usage_to_stderr_and_exits_two() {
    let temp = tempfile::tempdir().unwrap();
    let out = run_in(temp.path(), &["frobnicate"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr(&out).contains("usage:"), "stderr: {}", stderr(&out));
    assert!(stdout(&out).is_empty());
}

#[test]
fn help_prints_usage_to_stdout_and_exits_zero() {
    let temp = tempfile::tempdir().unwrap();
    let out = run_in(temp.path(), &["--help"]);
    assert!(out.status.success());
    assert!(stdout(&out).contains("usage:"), "stdout: {}", stdout(&out));
}

fn seed_config() -> String {
    "db_path = \"seed.db\"\nreports_dir = \"reports\"\n\
     [targets.apple]\nkind = \"company\"\nmarket = \"us\"\nasset_class = \"equity\"\n\
     tickers = [\"AAPL\"]\n"
        .to_string()
}

/// A config whose only enabled source is an RSS feed at `feed_url`, with the
/// LLM pointed at `llm_url` (the `custom` provider reads `CUSTOM_API_KEY`
/// from `.env`); the other registry plugins are disabled.
fn offline_config(feed_url: &str, llm_url: &str) -> String {
    format!(
        "db_path = \"seed.db\"\nreports_dir = \"reports\"\n\
         [llm]\nprovider = \"custom\"\nbase_url = \"{llm_url}\"\nmodel = \"test/model\"\n\
         [plugins.rss]\nfeeds = [\"{feed_url}\"]\n\
         [plugins.asx_announcements]\nenabled = false\n\
         [plugins.sec_edgar]\nenabled = false\n\
         [plugins.yfinance]\nenabled = false\n\
         [plugins.yfinance_calendar]\nenabled = false\n\
         [targets.apple]\nkind = \"company\"\nmarket = \"us\"\nasset_class = \"equity\"\n\
         tickers = [\"AAPL\"]\n"
    )
}

/// A dated feed whose items sit outside the 14-day extract/sentiment window
/// (so gather ingests rows without needing the LLM) but inside ingest's
/// 365-day default lookback.
fn dated_feed_body() -> Vec<u8> {
    let old =
        (chrono::Utc::now() - chrono::Duration::days(30)).format("%a, %d %b %Y 10:00:00 +0000");
    format!(
        r#"<?xml version="1.0"?>
<rss version="2.0"><channel>
<title>t</title><link>https://x</link><description>d</description>
<item><title>Apple ships thing</title><link>https://x/1</link>
<pubDate>{old}</pubDate></item>
<item><title>Apple ships another</title><link>https://x/2</link>
<pubDate>{old}</pubDate></item>
</channel></rss>"#
    )
    .into_bytes()
}

#[tokio::test]
async fn gather_ingests_from_configured_sources_and_summarises() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_raw(dated_feed_body(), "application/xml"),
        )
        .mount(&server)
        .await;

    let ws = Workspace::seed();
    ws.config(&offline_config(
        &format!("{}/feed.xml", server.uri()),
        &server.uri(),
    ));
    ws.env("CUSTOM_API_KEY", "test-key");

    let out = run_in(ws.path(), &["gather"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    assert!(text.contains("Ingesting via rss"), "{text}");
    assert!(
        text.contains("2 rows stored; 0 events extracted; 0 stances classified"),
        "{text}"
    );
    assert!(stderr(&out).is_empty(), "{}", stderr(&out));

    // Second run: dated items have stable ids, so nothing new is stored.
    let out = run_in(ws.path(), &["gather"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("0 rows stored; 0 events extracted; 0 stances classified"),
        "{}",
        stdout(&out)
    );
}

#[tokio::test]
async fn gather_target_flag_filters_the_universe() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_raw(dated_feed_body(), "application/xml"),
        )
        .mount(&server)
        .await;

    let ws = Workspace::seed();
    ws.config(&offline_config(
        &format!("{}/feed.xml", server.uri()),
        &server.uri(),
    ));
    ws.env("CUSTOM_API_KEY", "test-key");

    let out = run_in(ws.path(), &["gather", "--target", "apple"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("Ingesting via rss"),
        "{}",
        stdout(&out)
    );

    let out = run_in(ws.path(), &["gather", "--target", "no-such-target"]);
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("no-such-target"), "{}", stderr(&out));
}

#[tokio::test]
async fn gather_skips_a_failing_feed_and_still_exits_zero() {
    let server = wiremock::MockServer::start().await;
    wiremock::Mock::given(wiremock::matchers::method("GET"))
        .respond_with(wiremock::ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let ws = Workspace::seed();
    ws.config(&offline_config(
        &format!("{}/feed.xml", server.uri()),
        &server.uri(),
    ));
    ws.env("CUSTOM_API_KEY", "test-key");

    // A failing feed is skipped (rss design), not a failed gather: not every
    // source failed, so cron keeps going.
    let out = run_in(ws.path(), &["gather"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).contains("0 rows stored; 0 events extracted; 0 stances classified"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn gather_exits_one_when_the_database_cannot_be_opened() {
    let ws = Workspace::empty();
    // `seed.db` is a directory, not a database.
    std::fs::create_dir(ws.path().join("seed.db")).unwrap();
    ws.config(&seed_config());
    let out = run_in(ws.path(), &["gather"]);
    assert_eq!(out.status.code(), Some(1), "stdout: {}", stdout(&out));
    assert!(!stderr(&out).is_empty());
}

/// Serve the canned report draft (`fixtures/llm/report/apple.json`) as an
/// OpenAI-style chat completion from `server`.
async fn serve_report_draft(server: &wiremock::MockServer, fixture: &str) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let canned: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join(fixture)).unwrap()).unwrap();
    let content = serde_json::to_string(&canned["text"]).unwrap();
    wiremock::Mock::given(wiremock::matchers::method("POST"))
        .and(wiremock::matchers::path("/chat/completions"))
        .respond_with(
            wiremock::ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "choices": [{"message": {"role": "assistant", "content": content}}],
                "usage": {"prompt_tokens": 10, "completion_tokens": 10}
            })),
        )
        .mount(server)
        .await;
}

/// All registry plugins off, so `report`'s gather stage is a no-op over the
/// seed DB and the only remote call is the report LLM at `llm_url`.
fn llm_only_config(llm_url: &str) -> String {
    format!(
        "db_path = \"seed.db\"\nreports_dir = \"reports\"\n\
         [llm]\nprovider = \"custom\"\nbase_url = \"{llm_url}\"\nmodel = \"test/model\"\n\
         [plugins.rss]\nenabled = false\n\
         [plugins.asx_announcements]\nenabled = false\n\
         [plugins.sec_edgar]\nenabled = false\n\
         [plugins.yfinance]\nenabled = false\n\
         [plugins.yfinance_calendar]\nenabled = false\n"
    )
}

#[tokio::test]
async fn report_gathers_then_builds_and_prints_the_report_path() {
    let server = wiremock::MockServer::start().await;
    serve_report_draft(&server, "../../fixtures/llm/report/apple.json").await;

    let ws = Workspace::seed();
    ws.config(&llm_only_config(&server.uri()));
    ws.env("CUSTOM_API_KEY", "test-key");

    let out = run_in(ws.path(), &["report", "US:AAPL"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    // stdout carries only the report path, so `p=$(delta report US:AAPL)`
    // works; the gather stage logs to stderr.
    let text = stdout(&out);
    let path = ws.path().join(text.lines().last().expect("a report path"));
    assert_eq!(text.lines().count(), 1, "{text}");
    let markdown = std::fs::read_to_string(&path).unwrap();
    assert!(
        markdown.contains("## Bull case"),
        "report at {}:\n{markdown}",
        path.display()
    );
    // The citation contract, end to end: the draft cites the seed's chip
    // item (kept, citation rendered) and `news:news-aapl-10q` — an id that
    // does not exist (SEC items are `filing:`-prefixed), so that claim is
    // dropped from the persisted report.
    assert!(
        markdown.contains("[rss] Apple announces M6 chip"),
        "{markdown}"
    );
    assert!(
        !markdown.contains("Services revenue keeps double-digit growth"),
        "{markdown}"
    );
}

#[tokio::test]
async fn report_exits_one_when_no_claim_is_supported_by_gathered_evidence() {
    let server = wiremock::MockServer::start().await;
    // Every claim cites evidence ids that were never gathered.
    serve_report_draft(&server, "../../fixtures/llm/report/ghost-citations.json").await;

    let ws = Workspace::seed();
    ws.config(&llm_only_config(&server.uri()));
    ws.env("CUSTOM_API_KEY", "test-key");

    let out = run_in(ws.path(), &["report", "US:AAPL"]);
    assert_eq!(out.status.code(), Some(1), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("no claims supported"),
        "{}",
        stderr(&out)
    );
    assert!(!ws.path().join("reports/US:AAPL").exists());
}

#[tokio::test]
async fn report_fails_cleanly_for_a_target_with_no_evidence() {
    let server = wiremock::MockServer::start().await;
    let ws = Workspace::seed();
    ws.config(&llm_only_config(&server.uri()));
    ws.env("CUSTOM_API_KEY", "test-key");

    let out = run_in(ws.path(), &["report", "US:NOWHERE"]);
    assert_eq!(out.status.code(), Some(1), "stdout: {}", stdout(&out));
    assert!(
        stderr(&out).contains("no evidence gathered"),
        "{}",
        stderr(&out)
    );
}

#[test]
fn review_due_lists_decisions_and_exits_two_when_any_overdue() {
    let ws = Workspace::seed();
    ws.config(&seed_config());
    // 2026-09-21 (AAPL) is due and overdue on 2026-10-09; MSFT (2026-10-27)
    // is not yet due.
    let out = run_in(ws.path(), &["review-due", "--as-of", "2026-10-09"]);
    assert_eq!(out.status.code(), Some(2), "stderr: {}", stderr(&out));
    let text = stdout(&out);
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 1);
    assert!(
        lines[0].starts_with("dec-seed-aapl\t2026-09-21\tUS:AAPL\t"),
        "{lines:?}"
    );
}

#[test]
fn review_due_due_today_is_not_overdue() {
    let ws = Workspace::seed();
    ws.config(&seed_config());
    let out = run_in(ws.path(), &["review-due", "--as-of", "2026-09-21"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(
        stdout(&out).starts_with("dec-seed-aapl\t2026-09-21\t"),
        "{}",
        stdout(&out)
    );
}

#[test]
fn review_due_no_decisions_due_exits_zero_quietly() {
    let ws = Workspace::seed();
    ws.config(&seed_config());
    let out = run_in(ws.path(), &["review-due", "--as-of", "2026-09-01"]);
    assert_eq!(out.status.code(), Some(0), "stderr: {}", stderr(&out));
    assert!(stdout(&out).is_empty());
}

#[test]
fn review_due_rejects_a_malformed_date() {
    let ws = Workspace::seed();
    ws.config(&seed_config());
    let out = run_in(ws.path(), &["review-due", "--as-of", "09/21/2026"]);
    assert_eq!(out.status.code(), Some(1), "stderr: {}", stderr(&out));
    assert!(stderr(&out).contains("as-of"), "stderr: {}", stderr(&out));
}
