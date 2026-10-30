# Harness link: Delta MCP server in Rust

Research for #57 (map #56). Checked 2026-10-30. Claims marked **[unverified]** weren't confirmed in a primary source.

## Answer

- Build a **stdio** MCP server on **`rmcp`**, the official Rust SDK. Run it as a subcommand of the Delta binary, for example `delta mcp`. Both harnesses launch local stdio servers from a config entry, so you don't need a port, auth or a daemon.
- Register it once per harness. For Claude Code, use a project `.mcp.json` (shared) or `claude mcp add --scope user`. For Codex, use `[mcp_servers.delta]` in `~/.codex/config.toml`, or in `.codex/config.toml` for trusted projects.
- Don't let a tool block on a long fetch. Codex's default tool timeout is **60 s**. Expose long jobs as *start → job id → poll status/result* tools, and send MCP progress notifications while a call is open.
- Keep the headless CLI, but as a thin wrapper over the same service calls as the MCP tools, not as a second API. It's useful for scripts, debugging and cron-style background refresh, and as a fallback when a harness's MCP client misbehaves.

## MCP spec baseline

- The current revision is **2026-07-28**. It is stateless: protocol version and capabilities travel in each request's `_meta`, and there is no `initialize` handshake. `server/discover` returns versions and capabilities, and clients fall back to `initialize` for servers on `2025-11-25` or earlier. Source: [Versioning](https://modelcontextprotocol.io/specification/versioning), [Transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports).
- The standard transports are **stdio** (newline-delimited JSON-RPC over a client-launched subprocess) and **Streamable HTTP**. Source: [Transports](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports).
- stdio rules that affect Delta ([stdio](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/stdio)):
  - The server MUST NOT write anything to stdout except MCP messages. Logs go to stderr, so Delta's tracing must never print to stdout in MCP mode.
  - Servers SHOULD exit promptly when stdin closes.
  - Cancellation arrives as `notifications/cancelled`. Servers SHOULD stop work and MUST NOT reply afterwards.
  - Servers MUST NOT send JSON-RPC *requests*. Server-to-client input uses `InputRequiredResult` (multi round-trip).
- **Progress** ([Progress](https://modelcontextprotocol.io/specification/2026-07-28/basic/utilities/progress)):
  - The client puts `progressToken` in the request's `_meta`. The server MAY send `notifications/progress` with `progress`, an optional `total` and an optional `message`.
  - `progress` MUST increase with each notification, and notifications MUST stop once the request completes.
  - Progress is optional on both sides.

## Rust MCP crates

Download counts and release dates come from crates.io (queried 2026-10-30).

| Crate | Latest | Downloads | Licence / MSRV | Notes |
|---|---|---|---|---|
| [`rmcp`](https://github.com/modelcontextprotocol/rust-sdk) | 3.8.0 (2026-10-26) | ~42 M | Apache-2.0 / 1.88 | Official SDK (`modelcontextprotocol` org), ~4.1k stars. Implements 2026-07-28 and is compatible with 2025-11-25 and earlier. |
| [`rust-mcp-sdk`](https://github.com/rust-mcp-stack/rust-mcp-sdk) | 2.0.0 (2026-08-27) | ~0.33 M | MIT / 1.80 | Community, ~200 stars. 2.x implements 2026-07-28; the 1.x LTS line covers 2025-11-25. Progress and tasks support not documented. |
| `mcp-protocol-sdk`, `mcpr` | 0.5.1 / 0.2.3 (2025) | <15 k | MIT | Stale. Don't use. |

`rmcp` details (from [its README](https://github.com/modelcontextprotocol/rust-sdk)):

- **Transports:**
  - stdio (`transport-io`)
  - Streamable HTTP server (`transport-streamable-http-server`)
  - Client transports
  - Legacy 2024-11-05 HTTP+SSE is deliberately absent.
- **Macros:** `#[tool]`, `#[tool_router]` and `#[tool_handler]`. `#[tool_router(server_handler)]` covers tools-only servers.
- **Progress:** `context.peer.notify_progress(...)`.
- **Tasks:** implements the MCP Tasks extension (SEP-2663) with a `TaskManager`.

`rmcp` is also what Codex uses as its own MCP client: `openai/codex` `codex-rs/Cargo.toml` pins `rmcp = "=3.5.1"`, so the two should interoperate.

Fit with Delta: the workspace MSRV is 1.94 and it already uses tokio, so `rmcp` fits without a toolchain change. Adding it is a new dependency and needs approval (AGENTS.md).

## Claude Code

Source: [Claude Code MCP docs](https://code.claude.com/docs/en/mcp).

- **Transports:**
  - stdio
  - HTTP (recommended for remote servers)
  - SSE (deprecated)
  - WebSocket (`type: "ws"`, config file only)
- **Register:**
  - Command: `claude mcp add [--scope local|project|user] [--env K=V] delta -- /path/to/delta mcp`. Everything after `--` is passed through untouched.
  - Or: `claude mcp add-json delta '{"command":"delta","args":["mcp"]}'`.
- **Scopes and files:**
  - local (default): `~/.claude.json`, current project only
  - user: `~/.claude.json`, all projects
  - project: `.mcp.json` in the repo root as `{"mcpServers": {"delta": {...}}}`. It's checked in and needs a one-time approval.
  - Definitions aren't merged across scopes.
  - Config values support `${VAR}` and `${VAR:-default}` expansion.
  - `CLAUDE_PROJECT_DIR` is set in the environment of stdio servers.
- **Timeouts:**
  - `MCP_TIMEOUT` sets the startup timeout.
  - `MCP_TOOL_TIMEOUT` is the wall-clock limit. Its default is about 28 h.
  - A per-server `timeout` (ms) in `.mcp.json` overrides `MCP_TOOL_TIMEOUT`. Progress notifications do **not** extend it.
  - The idle timeout is **30 min for stdio** (5 min for HTTP). A call that sends no response and no progress within that window aborts, so progress notifications keep a call alive.
- **Long calls:** a call still running after **2 min** moves to a background task (`CLAUDE_CODE_MCP_AUTO_BACKGROUND_MS`, `0` turns it off).
  - The task shows the latest progress the server reported.
  - The result arrives as a task notification.
  - This doesn't happen for subagents or `-p` mode unless `CLAUDE_AUTO_BACKGROUND_TASKS=1`.
- **MCP Tasks:** public beta in CLI v2.1.304+. They require the server to support the tasks extension on revision 2026-07-28. Not available in the VS Code extension or desktop app.
- **Output limits:**
  - A warning shows above 10,000 tokens.
  - The default cap is 25,000 tokens (`MAX_MCP_OUTPUT_TOKENS`). Text results over 50,000 characters are saved to a file instead.
  - A tool can raise its own threshold with `_meta["anthropic/maxResultSizeChars"]` in its `tools/list` entry, up to 500,000.
  - Error text over about 11k characters is truncated.
- **Many tools:**
  - Tool search is on by default, so only tool names and server instructions load at start. There's no per-server tool cap.
  - Server instructions and descriptions of tools that load upfront are cut at 4,096 characters; descriptions of tools loaded through tool search at 16,384. Put the key words first.
- **Resources and prompts:**
  - Resources work through `@server:uri` mentions, with list and read tools added automatically.
  - Prompts become slash commands.
  - `list_changed` refreshes the tool list without a reconnect.
  - Form and URL elicitation are supported.

## Codex

Sources: [Codex MCP docs](https://learn.chatgpt.com/docs/extend/mcp?surface=cli) (redirected from developers.openai.com/codex/mcp) and the `openai/codex` source.

- **Transports:** stdio and Streamable HTTP.
- **Register:**
  - Command: `codex mcp add delta --env K=V -- /path/to/delta mcp`.
  - Or edit `~/.codex/config.toml`. A project `.codex/config.toml` is read for trusted projects only.
  - The CLI, IDE extension and ChatGPT desktop app share this config.

  ```toml
  [mcp_servers.delta]
  command = "delta"
  args = ["mcp"]
  cwd = "/path/to/Delta"
  tool_timeout_sec = 600    # default 60
  startup_timeout_sec = 10  # default 10
  ```

  Other keys:
  - `env` and `env_vars`
  - `enabled` and `required`
  - `enabled_tools` and `disabled_tools`
  - `default_tools_approval_mode`
  - `tools.<tool>.approval_mode`
  - `tools.<tool>.output_token_limit`
- **Limits:**
  - `tool_timeout_sec` defaults to **60 s**, the binding constraint for data fetches.
  - Output is capped per tool with `output_token_limit`. **[unverified]** The global default output cap isn't documented.
  - The first 512 characters of the server's `instructions` should stand on their own.
- **Resources:** supported. Codex exposes built-in `list_mcp_resources` and `list_mcp_resource_templates` tools (`codex-rs/core/src/tools/handlers/mcp_resource*`). It also has BM25 `tool_search` for deferred tools.
- **Progress:**
  - **[unverified]** The stdio client's `on_progress` (`codex-rs/rmcp-client/src/logging_client_handler.rs`) only logs the notification.
  - The app-server protocol has an `item/mcpToolCall/progress` event (message only) that the TUI handles. I couldn't confirm that server progress reaches it.
  - **[unverified]** Whether progress extends `tool_timeout_sec`; assume it doesn't.
  - **[unverified]** Support for MCP Tasks; nothing in the docs.

## Design implications for Delta

1. **Transport:** stdio only. HTTP adds a port, auth and a daemon lifecycle for no gain on one machine. Add Streamable HTTP later only if a GUI or remote harness needs a shared long-lived server.
2. **Long-running work:** don't rely on a single blocking call, given Codex's 60 s default and Tasks being beta and Claude-only.
   - Use `start_fetch` → `{job_id}`, then `job_status(job_id)` and `job_result(job_id)`, with jobs persisted in SQLite.
   - Send `notifications/progress` during any call that can take more than a few seconds. It shows in Claude Code's `/tasks` and keeps its idle timer alive.
   - Optionally adopt MCP Tasks once both harnesses support it.
3. **Concurrency:** each harness session spawns its own `delta mcp` process, and the TUI may be open at the same time. SQLite needs WAL mode and busy timeouts, and job execution should be idempotent so a restarted process can resume it.
4. **Output size:**
   - Paginate list tools and keep results under about 25k tokens or 50k characters.
   - Return evidence IDs and summaries, with full text available through resources (`delta://source/<id>`). Both harnesses can read resources.
5. **Tool surface:** keep it small and well described, with clear `instructions` whose first 512 characters stand alone (Codex) and that fit within 4,096 characters (Claude Code). Write tools validate citations server-side, per #56.
6. **CLI fallback:** keep `delta <subcommand>` sharing the service layer with the MCP tools. It covers scripts, cron refresh while no harness runs (an open #56 question), and debugging when an MCP client fails. It shouldn't grow features the MCP surface lacks.

## Open questions

- Will background refresh (#56) be a separate long-lived `delta daemon`, or jobs that run only while some Delta process is alive?
- Ship a project `.mcp.json` and `.codex/config.toml` in the repo, or document user-scope registration only? The binary path differs per machine.
