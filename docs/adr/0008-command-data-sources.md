# Command data sources run local scripts

Besides built-in Rust adapters, a data source can be a **command**: a local script named in `config.toml`. It receives JSON on stdin and prints rows in Delta's schema on stdout, and Delta validates every row. Only the user adds or enables a source. The harness may write a script and propose its config entry but can't apply either.

This makes Delta work for any market or provider without a Rust release, which a market- and asset-agnostic tool needs. The cost is that Delta runs local code, so trust sits with the user's own config.

**Considered options:** a generic HTTP/JSON connector with field mappings, rejected as brittle to configure; adapters only, rejected because every new market would need Rust work. See [Data sources in config](https://github.com/lukebrown14-code/Delta/issues/63).
