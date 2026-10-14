# Delta

Delta is a personal investment research assistant. It gathers public evidence into SQLite, uses an LLM to write cited reports and answer questions, and lets you track long-term theses and decisions. It does not trade or recommend what to buy.

Every report claim and grounded answer is checked against gathered evidence IDs. The thesis layer proposes candidate evidence; you decide what to accept.

## Install

Clone the repository for its `config.toml` and `.env.example`, download the archive for your platform from [Releases](https://github.com/lukebrown14-code/Delta/releases), and run the extracted `delta` binary from the repository directory. To build the binary from source, install the Rust toolchain in `rust-toolchain.toml` and run:

```bash
cargo build --release -p delta-tui
./target/release/delta --version
```

Copy `.env.example` to `.env` and add your LLM provider key. `config.toml` holds targets, model routing, plugin settings, and the database path. Set `[plugins.sec_edgar].contact` to a real email before requesting SEC filings.

## Use

```bash
cargo run --release -p delta-tui
```

| Key | Screen |
|---|---|
| `1` | Home |
| `2` | Watchlist |
| `3` | Research: reports and evidence |
| `4` | Theses |
| `5` | Ask |
| `6` | Decisions |
| `c` | Settings |
| `?` / `q` | Help / quit |

The app also has headless commands:

```bash
delta gather [--target NAME]
delta report TARGET
delta review-due [--as-of YYYY-MM-DD]
```

Run `delta --help` for command usage. The app reads `config.toml` and `.env` from the current directory. `data/` and `reports/` are local output and should not be committed.

## Develop

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test --workspace
```

CI runs these Rust checks. Offline tests use fixed fixtures and mocked HTTP/LLM responses; they do not call live data or model services. The frozen Python reference data in `fixtures/` documents parity with the pre-cutover app.

The workspace has five crates: `delta-core` for config, models, and SQLite; `delta-plugins` for data sources and markets; `delta-llm` for model providers and prompts; `delta-services` for application operations; and `delta-tui` for the terminal app. New data sources belong in the plugin registry, and screens call services rather than providers or storage directly.

[PROJECT_SPEC.md](PROJECT_SPEC.md) is the original product and domain specification. [Rust rewrite docs](docs/rewrite/) record the migration and cutover evidence.

Free sources can be delayed or wrong. Check primary sources before relying on a report. Nothing here is financial advice.
