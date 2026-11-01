# Stdio MCP server, polled jobs and one tool registry

The harness reaches Delta through a **stdio** MCP server (`delta mcp`, built on `rmcp`). Long fetches are started as jobs and polled with `job_status` rather than held open in one call. The MCP tools, the CLI (`delta tool <name> --json`) and any future GUI all call one tool registry over the service layer.

Stdio needs no port, auth or daemon on a single machine. Codex's 60-second tool timeout rules out blocking fetches. A single registry stops the CLI, MCP and GUI from becoming three APIs that drift apart.

**Considered options:** Streamable HTTP, rejected for its daemon and auth overhead; it can be added later if a remote or shared server is needed. MCP Tasks, rejected for now because only Claude Code supports them. See [Harness link](https://github.com/lukebrown14-code/Delta/issues/57) and [MCP tool surface](https://github.com/lukebrown14-code/Delta/issues/66).
