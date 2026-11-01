# Harness guides ship inside the Delta binary

The workflow guides the harness follows (the new-Space interview, how to write each report type, refreshing a Space) are compiled into Delta. Each is served as a `delta://guide/<name>` MCP resource and as an MCP prompt. There are no separate Claude Code skills or `AGENTS.md` snippets. Registration is per user (`delta setup harness`), not per project.

The guides then version with the tools they describe, so they can't drift. Resources work in both harnesses even if Codex ignores MCP prompts, and user-scope registration lets the harness run from any research folder. See [Harness packaging](https://github.com/lukebrown14-code/Delta/issues/70).
