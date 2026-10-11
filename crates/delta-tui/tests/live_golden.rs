//! The populated-state golden harness (spec #26, seam 1).
//!
//! Generic by construction: it reads `fixtures/golden_screens/live-manifest.json`,
//! renders every scenario through `delta_tui::screens::live::render_live`
//! (the registry that maps a state name to its builder) and diffs
//! cell-for-cell against the Python-oracle fixture at the scenario's size.
//! A new screen state is added with a manifest entry + fixture + one
//! builder registration — never new harness code.
//!
//! Tier A: character, fg, bg and bold must match exactly. Attrs beyond the
//! base cell model (`reverse`, `italic`, `underline` — Textual scrollbar
//! chrome) are tolerated but no other attribute may appear.

use std::path::PathBuf;

use delta_core::db::Db;
use delta_tui::screen::Screen;
use delta_tui::screens::live::render_live;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One live-manifest entry.
struct Entry {
    state: String,
    size: (usize, usize),
    tier: String,
    file: String,
    seed: PathBuf,
}

fn manifest() -> Vec<Entry> {
    let raw =
        std::fs::read_to_string(repo_root().join("fixtures/golden_screens/live-manifest.json"))
            .expect("read fixtures/golden_screens/live-manifest.json");
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse live-manifest.json");
    value["scenarios"]
        .as_array()
        .expect("live-manifest scenarios")
        .iter()
        .map(|s| {
            let size = s["size"].as_str().expect("size").to_string();
            let (w, h) = size.split_once('x').expect("WxH size");
            Entry {
                state: s["state"].as_str().expect("state").to_string(),
                size: (w.parse().expect("width"), h.parse().expect("height")),
                tier: s["tier"].as_str().expect("tier").to_string(),
                file: s["file"].as_str().expect("file").to_string(),
                seed: repo_root().join(s["seed"].as_str().expect("seed")),
            }
        })
        .collect()
}

fn row_text(row: &serde_json::Value) -> String {
    row.as_array()
        .expect("row")
        .iter()
        .map(|cell| {
            cell["ch"]
                .as_str()
                .unwrap_or(" ")
                .chars()
                .next()
                .unwrap_or(' ')
        })
        .collect::<String>()
        .trim_end()
        .to_string()
}

/// Cell-by-cell Tier A diff; also rejects attrs the base cell model cannot
/// represent (anything outside bold/reverse/italic/underline).
fn diff(entry: &Entry, screen: &Screen) -> Vec<String> {
    let path = repo_root()
        .join("fixtures/golden_screens")
        .join(&entry.file);
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", entry.file));
    let value: serde_json::Value = serde_json::from_str(&raw).expect("parse golden JSON");
    let rows = value["rows"].as_array().expect("rows");
    assert_eq!(
        rows.len(),
        screen.h,
        "{}: golden height {} != rendered {}",
        entry.file,
        rows.len(),
        screen.h
    );
    let mut diffs = Vec::new();
    for (y, row) in rows.iter().enumerate() {
        let cells = row.as_array().expect("row");
        assert_eq!(cells.len(), screen.w, "{}: row {y} width", entry.file);
        for (x, cell) in cells.iter().enumerate() {
            // Report prose is Tier B: service tests pin its markdown bytes,
            // while this frame keeps every pane, control, and evidence cell
            // at Tier A. Textual and Rust wrap prose differently.
            if entry.tier == "B"
                && entry.state.starts_with("live-research")
                && entry.size.0 >= 100
                && x >= 38
                && x < entry.size.0 - 42
                && y >= 6
                && y < entry.size.1 - 3
            {
                continue;
            }
            let mine = &screen.cells[y * screen.w + x];
            let want_ch = cell["ch"].as_str().unwrap().chars().next().unwrap();
            if mine.ch != want_ch {
                diffs.push(format!(
                    "{}: row {y} col {x}: want {want_ch:?} got {:?}",
                    entry.file, mine.ch
                ));
                continue;
            }
            let want = (
                cell["fg"].as_str(),
                cell["bg"].as_str(),
                cell["attrs"]
                    .as_array()
                    .expect("attrs")
                    .iter()
                    .any(|a| a == "bold"),
            );
            let got = (mine.fg, mine.bg, mine.bold);
            if got != want {
                diffs.push(format!(
                    "{}: row {y} col {x}: want {want:?} got {got:?}",
                    entry.file
                ));
            }
            for attr in cell["attrs"].as_array().expect("attrs") {
                let name = attr.as_str().unwrap_or_default();
                assert!(
                    ["bold", "reverse", "italic", "underline"].contains(&name),
                    "{}: row {y} col {x}: unsupported attr {name:?}",
                    entry.file
                );
            }
        }
    }
    diffs
}

/// Side-by-side text diff of the rows that differ.
fn print_side_by_side(entry: &Entry, screen: &Screen) {
    let path = repo_root()
        .join("fixtures/golden_screens")
        .join(&entry.file);
    let value: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(path).unwrap_or_default())
            .expect("parse golden JSON");
    let rows = value["rows"].as_array().expect("rows");
    let mut printed = 0;
    for (y, row) in rows.iter().enumerate() {
        let want = row_text(row);
        let got: String = (0..screen.w)
            .map(|x| screen.cells[y * screen.w + x].ch)
            .collect::<String>()
            .trim_end()
            .to_string();
        if want != got {
            println!("{:>4} want |{want}|", "");
            println!("     got  |{got}|");
            printed += 1;
            if printed >= 16 {
                println!("     ... (side-by-side truncated at 16 rows)");
                return;
            }
        }
    }
}

/// Every populated state matches the Python oracle cell-for-cell at its
/// manifest size, rendered from the shared seed DB.
#[test]
fn populated_states_match_the_python_oracle() {
    let entries = manifest();
    assert!(
        !entries.is_empty(),
        "live-manifest.json lists no scenarios; export them with \
         `uv run python tests/export_golden.py --state live --out fixtures/golden_screens`"
    );
    for entry in &entries {
        assert!(
            entry.seed.exists(),
            "seed DB missing for {}: {}",
            entry.file,
            entry.seed.display()
        );
        let fixture_path = repo_root()
            .join("fixtures/golden_screens")
            .join(&entry.file);
        let fixture: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&fixture_path)
                .unwrap_or_else(|e| panic!("read {}: {e}", fixture_path.display())),
        )
        .expect("parse fixture JSON");
        let screen = render_live(&entry.state, &fixture, &entry.seed).unwrap_or_else(|| {
            panic!(
                "no live renderer registered for state {:?}; add one line to \
                 crates/delta-tui/src/screens/live.rs",
                entry.state
            )
        });
        assert_eq!(screen.w, entry.size.0, "{}: width", entry.file);
        assert_eq!(screen.h, entry.size.1, "{}: height", entry.file);
        let diffs = diff(entry, &screen);
        if !diffs.is_empty() {
            print_side_by_side(entry, &screen);
        }
        assert!(
            diffs.is_empty(),
            "{}: {} Tier A mismatches\n{}",
            entry.file,
            diffs.len(),
            diffs
                .iter()
                .take(30)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }
}

/// The harness stays generic: the shared seed DB every live scenario names
/// must open and hold data (guards the manifest's seed paths).
#[test]
fn live_scenarios_read_the_shared_seed_db() {
    let entries = manifest();
    let seed = &entries[0].seed;
    let db = Db::open(seed).expect("open the shared seed DB");
    let bars = db.bars("US:AAPL").expect("seed bars");
    assert_eq!(bars.len(), 80, "the shared seed's AAPL series");
    let bars = db.bars("US:MSFT").expect("seed bars");
    assert_eq!(bars.len(), 80, "the shared seed's MSFT series");
}
