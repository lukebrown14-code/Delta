# Harness-first: Delta stops generating text

All open-ended AI work moves to an external harness (Claude Code or Codex): research, interviews, reports and Q&A. Delta keeps data sources, price refresh, validation, storage and the Jev client. It drops its own report and chat generation, and the `delta-llm` crate shrinks to the Jev client, which moves into another crate. The harness never writes SQLite directly; every write goes through Delta's validated interface.

Harnesses already do research and long-form writing better than a prompt pipeline inside Delta, and they improve without Delta changing. Delta's value is the parts a harness can't keep: a durable, cited, per-idea store, deterministic checks, and a screen to read it on. The cost is that Delta can't produce a report on its own. See [Audit](https://github.com/lukebrown14-code/Delta/issues/59) and [Harness link](https://github.com/lukebrown14-code/Delta/issues/57).
