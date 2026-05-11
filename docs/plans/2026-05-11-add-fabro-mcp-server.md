# Fabro MCP Server Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use trycycle-executing to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a stdio MCP server to the `fabro` CLI, exposed as `fabro mcp start`, with `fabro mcp config` and `fabro mcp init <agent>` support and first-class tools for managing Fabro runs.

**Architecture:** Implement the Fabro MCP server inside `fabro-cli`, not in `fabro-mcp`, because the server must reuse CLI authentication, server target resolution, settings loading, and run manifest construction without creating a crate dependency cycle. Use `rmcp` server macros and stdio transport for protocol correctness, connect lazily to the Fabro API through `CommandContext`, and return structured MCP tool results plus text fallbacks. Keep the existing `fabro-mcp` crate as the external MCP client used by Fabro agents.

**Tech Stack:** Rust, clap, tokio, rmcp 1.3 stdio server transport, serde/schemars JSON schemas, fabro-client, fabro-api generated types, existing Fabro CLI integration test harness with insta snapshots.

---

## File Structure

- Modify `lib/crates/fabro-cli/Cargo.toml`
  - Add a direct `rmcp` dependency with server, macros, schemars, and stdio transport features. `fabro-cli` currently reaches `rmcp` only indirectly through `fabro-mcp`; direct usage is needed because the new server implementation lives in this crate.

- Modify `lib/crates/fabro-cli/src/args.rs`
  - Add `McpNamespace`, `McpCommand`, `McpStartArgs`, `McpConfigArgs`, `McpInitArgs`, and `McpAgent`.
  - Add `Commands::Mcp(McpNamespace)` and `Commands::name()` branch returning `mcp start`, `mcp config`, or `mcp init`.

- Modify `lib/crates/fabro-cli/src/main.rs`
  - Add `mod commands::mcp` dispatch.
  - Keep `fabro mcp start` on the normal CLI logging path, which writes logs to stderr, and never write human output to stdout during stdio serving.

- Modify `lib/crates/fabro-cli/src/commands/mod.rs`
  - Export the new `mcp` command module.

- Create `lib/crates/fabro-cli/src/commands/mcp/mod.rs`
  - Own CLI dispatch for `start`, `config`, and `init`.

- Create `lib/crates/fabro-cli/src/commands/mcp/server.rs`
  - Own the stdio MCP service, tool registration, tool parameter structs, output structs, API client acquisition, and tool error shaping.

- Create `lib/crates/fabro-cli/src/commands/mcp/run_tools.rs`
  - Own run-management behavior behind the MCP tools: create/start, search, interact, gather, and events.
  - This split keeps protocol boilerplate out of run semantics.

- Create `lib/crates/fabro-cli/src/commands/mcp/config.rs`
  - Own generic MCP config rendering and agent-specific config path/merge/write logic.

- Modify `lib/crates/fabro-cli/src/commands/run/overrides.rs`
  - Expose small helper functions needed by MCP run creation, or add equivalent crate-visible wrappers:
    - label parsing
    - goal layer construction
    - execution/model/sandbox override construction
  - Do not duplicate manifest override semantics in the MCP module.

- Modify `lib/crates/fabro-cli/tests/it/cmd/mod.rs`
  - Add `mod mcp;`.

- Create `lib/crates/fabro-cli/tests/it/cmd/mcp.rs`
  - Add CLI help/config/init snapshots and stdio MCP integration tests.

- Optionally modify `lib/crates/fabro-cli/tests/it/support/mod.rs`
  - Add only narrow helpers for spawning `fabro mcp start` or extracting MCP text/structured output if duplication appears in `cmd/mcp.rs`.

## User-Visible Contract

The CLI contract is:

```text
fabro mcp start [--server <SERVER>] [--storage-dir <DIR>]
fabro mcp config [--server <SERVER>] [--storage-dir <DIR>]
fabro mcp init <agent> [--server <SERVER>] [--storage-dir <DIR>]
```

Supported agents for the first implementation:

```text
claude
cursor
windsurf
```

`fabro mcp config` emits generic MCP client JSON to stdout:

```json
{
  "mcpServers": {
    "fabro": {
      "command": "fabro",
      "args": ["mcp", "start"]
    }
  }
}
```

When `--server` or `--storage-dir` is passed to `config` or `init`, preserve those choices in the emitted or written `args`, for example:

```json
{
  "mcpServers": {
    "fabro": {
      "command": "fabro",
      "args": ["mcp", "start", "--server", "https://example.test/api/v1"]
    }
  }
}
```

`fabro mcp init <agent>` writes the same entry into the agent config file under `mcpServers.fabro`, preserving every unrelated existing key. Re-running it is idempotent. If the existing file is invalid JSON or its root is not an object, fail clearly and do not overwrite it.

Agent config paths:

- `claude`
  - macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
  - Linux: `~/.config/Claude/claude_desktop_config.json`
  - Windows: `%APPDATA%\Claude\claude_desktop_config.json`
- `cursor`
  - all platforms: `~/.cursor/mcp.json`
- `windsurf`
  - all platforms: `~/.codeium/windsurf/mcp_config.json`

The MCP server exposes exactly these tools in this first slice:

```text
fabro_run_create
fabro_run_search
fabro_run_interact
fabro_run_gather
fabro_run_events
```

### Tool Semantics

`fabro_run_create`

- Input:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
struct FabroRunCreateParams {
    runs: Vec<CreateRunSpec>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct CreateRunSpec {
    workflow: String,
    cwd: Option<PathBuf>,
    run_id: Option<String>,
    goal: Option<String>,
    inputs: HashMap<String, serde_json::Value>,
    labels: HashMap<String, String>,
    dry_run: Option<bool>,
    auto_approve: Option<bool>,
    model: Option<String>,
    provider: Option<String>,
    sandbox: Option<String>,
    preserve_sandbox: Option<bool>,
    start: Option<bool>,
}
```

- `runs` is required and must contain 1 to 50 entries.
- `workflow` is a workflow path or project workflow selector resolved from `cwd` when provided, otherwise from the MCP process cwd.
- `start` defaults to `true` because this is analogous to Devin session creation: creating a run for an agent should normally launch it. Passing `start: false` creates a submitted run without starting it.
- `inputs` object values are converted to `toml::Value` with JSON-compatible semantics: string, bool, integer, float, arrays, and objects are accepted; null is rejected with a tool error naming the key.
- Output is structured:

```rust
#[derive(Debug, Serialize, JsonSchema)]
struct CreateRunsResult {
    runs: Vec<CreatedRunResult>,
}

#[derive(Debug, Serialize, JsonSchema)]
struct CreatedRunResult {
    run_id: String,
    workflow: String,
    started: bool,
    status: String,
}
```

`fabro_run_search`

- Input:

```rust
struct FabroRunSearchParams {
    run_ids: Option<Vec<String>>,
    workflow: Option<String>,
    labels: Option<HashMap<String, String>>,
    status: Option<Vec<String>>,
    archived: Option<bool>,
    created_after: Option<String>,
    created_before: Option<String>,
    first: Option<usize>,
    after: Option<String>,
}
```

- Search starts from `Client::list_store_runs()`, which already includes archived runs.
- `status` uses existing `run_status_kind(...)` strings.
- `created_after` and `created_before` parse RFC3339 timestamps or `YYYY-MM-DD` dates.
- `first` defaults to 20 and has max 100.
- `after` is an opaque cursor containing the last run id from the previous page. For the first implementation, encode it as the run id string and document it as opaque in the tool description.
- Output contains normalized run summaries:

```rust
struct RunSummaryResult {
    run_id: String,
    workflow_name: String,
    workflow_slug: Option<String>,
    status: String,
    archived: bool,
    created_at: String,
    started_at: Option<String>,
    completed_at: Option<String>,
    labels: HashMap<String, String>,
    source_directory: Option<String>,
    repo_origin_url: Option<String>,
    goal: String,
}
```

`fabro_run_interact`

- Input:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RunInteractAction {
    Get,
    Start,
    Message,
    Cancel,
    Archive,
    Unarchive,
    GetQuestions,
    Answer,
}

struct FabroRunInteractParams {
    action: RunInteractAction,
    run_id: String,
    message: Option<String>,
    interrupt: Option<bool>,
    question_id: Option<String>,
    answer: Option<serde_json::Value>,
}
```

- `run_id` accepts the same selector semantics as CLI commands by calling `Client::resolve_run(...)`.
- `get` returns summary plus projection from `retrieve_run` and `get_run_state`.
- `start` calls `start_run(resume = false)`.
- `message` calls `steer_run`; `message` is required and trimmed; `interrupt` defaults false.
- `cancel` calls `cancel_run`.
- `archive` and `unarchive` call existing API methods.
- `get_questions` calls `list_run_questions`.
- `answer` requires `question_id` and maps answer JSON into `SubmitAnswerRequest`:
  - boolean true -> yes
  - boolean false -> no
  - string -> freeform
  - `{ "option": "key" }` -> single choice
  - `{ "options": ["a", "b"] }` -> multi choice
  - `{ "text": "..." }` -> freeform
- Return a structured object with `run_id`, `action`, and action-specific `result`.

`fabro_run_gather`

- Input:

```rust
struct FabroRunGatherParams {
    run_ids: Vec<String>,
    timeout_seconds: Option<u64>,
    poll_interval_seconds: Option<u64>,
}
```

- `run_ids` is required, max 50.
- `timeout_seconds` defaults to 300 and maxes at 600.
- `poll_interval_seconds` defaults to 15 and mins at 5.
- Resolve selectors once at the start.
- Poll `retrieve_run` until every run is terminal or timeout expires.
- Output contains each final or current run summary plus `timed_out: bool`.

`fabro_run_events`

- Input:

```rust
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum RunEventsAction {
    List,
    Details,
    Search,
}

struct FabroRunEventsParams {
    action: RunEventsAction,
    run_id: String,
    event_types: Option<Vec<String>>,
    categories: Option<Vec<String>>,
    direction: Option<String>,
    created_after: Option<String>,
    created_before: Option<String>,
    first: Option<usize>,
    after: Option<u32>,
    event_ids: Option<Vec<String>>,
    offset: Option<usize>,
    limit: Option<usize>,
    max_content_length: Option<usize>,
    query: Option<String>,
}
```

- Use `Client::list_run_events(...)` rather than SSE for deterministic request/response behavior.
- `list` returns paginated envelopes sorted ascending by default; `direction: "desc"` reverses after fetching.
- `details` filters by `event_ids`.
- `search` filters events whose serialized event JSON contains `query`.
- `event_types` match `event.event_name()`.
- `categories` are best-effort derived from the prefix before the first `.` in `event_name`, for example `run.completed` has category `run`.
- `first` defaults to 50 and maxes at 200. `limit` is accepted as an alias for compatibility with the Devin-shaped input. `after` maps to `since_seq`.
- `max_content_length` defaults to 20_000 and truncates only large serialized event payload strings, with a `truncated: true` marker in the returned event item.

### Contracts And Invariants

- `fabro mcp start` stdout is reserved for MCP JSON-RPC only. All logs, warnings, errors, tracing, and diagnostics must go to stderr.
- MCP initialize and tools/list must not require a live Fabro server. API connection is lazy and happens when a tool needs it.
- There is no separate MCP authentication. Tool calls use the same `CommandContext` and `fabro-client` auth store behavior as existing CLI commands. Auth failures returned from tools must include the existing user guidance: `Run \`fabro auth login\` to authenticate.`
- Tool failures are MCP tool errors, not process exits. The stdio server should stay alive after invalid arguments, not-found selectors, conflicts, auth failures, and API errors.
- Every successful tool returns structured content and a concise text fallback. The text fallback is for clients that do not yet show MCP structured output.
- Run selectors must go through `Client::resolve_run(...)` to preserve existing Fabro prefix/workflow-name behavior.
- Run creation must reuse `build_run_manifest(...)` and server manifest validation. Do not fabricate run specs or bypass the same source-of-truth path as `fabro create`.
- Agent config writes must be idempotent and preserve unrelated user config.
- Do not add live LLM/provider tests for this first slice. Use dry-run workflows and local/test servers.

## Strategy Decisions

- **Keep implementation in `fabro-cli`:** The existing `fabro-mcp` crate is a client abstraction for agents consuming third-party MCP servers. A Fabro MCP server needs CLI config, auth, and manifest-building internals. Moving those internals into `fabro-mcp` would either create a cycle or force a broad public API extraction. The clean steady-state for this feature is a CLI-owned MCP server module with a future extraction point if other binaries need it.
- **Use `rmcp` instead of hand-rolled JSON-RPC:** The project already depends on `rmcp` and uses it for MCP client behavior. The server should use the same SDK to get initialize/tools/list/tools/call semantics, JSON schema generation, and stdio framing right.
- **Default create to start:** Devin's session creation starts usable sessions. For Fabro, a run that stays submitted unless the caller remembers a second tool call is a surprising first-use experience. `start: false` keeps the lower-level control available without making it the default.
- **Use five Devin-shaped tools instead of many tiny tools:** The user explicitly asked to adapt Devin sessions to Fabro runs. The five-tool shape is easier for MCP clients to discover and keeps later additions compatible. Internally, the Rust implementation should still split actions into small functions.
- **Lazy API connection:** MCP clients often list tools during startup. Requiring auth/server connectivity during initialize would make even configuration validation brittle. Lazy connection gives users useful tool discovery and clear per-tool auth errors.

## Task 1: Add CLI Surface And Help Snapshots

**Files:**
- Modify: `lib/crates/fabro-cli/Cargo.toml`
- Modify: `lib/crates/fabro-cli/src/args.rs`
- Modify: `lib/crates/fabro-cli/src/main.rs`
- Modify: `lib/crates/fabro-cli/src/commands/mod.rs`
- Create: `lib/crates/fabro-cli/src/commands/mcp/mod.rs`
- Create: `lib/crates/fabro-cli/tests/it/cmd/mcp.rs`
- Modify: `lib/crates/fabro-cli/tests/it/cmd/mod.rs`

- [ ] **Step 1: Write failing CLI help tests**

Add `mod mcp;` to `lib/crates/fabro-cli/tests/it/cmd/mod.rs`.

Create `lib/crates/fabro-cli/tests/it/cmd/mcp.rs` with snapshots for:

```rust
use fabro_test::{fabro_snapshot, test_context};

#[test]
fn help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"");
}

#[test]
fn start_help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "start", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"");
}

#[test]
fn config_help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "config", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"");
}

#[test]
fn init_help() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "init", "--help"]);
    fabro_snapshot!(context.filters(), cmd, @"");
}
```

- [ ] **Step 2: Run the help tests and verify they fail**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::help cmd::mcp::start_help cmd::mcp::config_help cmd::mcp::init_help
```

Expected: FAIL because `fabro mcp` does not exist.

- [ ] **Step 3: Add clap arguments and no-op dispatch**

In `lib/crates/fabro-cli/Cargo.toml`, add direct `rmcp` dependency:

```toml
rmcp = { workspace = true, features = ["server", "macros", "schemars", "transport-io"] }
```

In `lib/crates/fabro-cli/src/args.rs`, add:

```rust
#[derive(Args)]
pub(crate) struct McpNamespace {
    #[command(subcommand)]
    pub(crate) command: McpCommand,
}

#[derive(Subcommand)]
pub(crate) enum McpCommand {
    /// Start the Fabro MCP server over stdio
    Start(McpStartArgs),
    /// Print MCP client configuration JSON
    Config(McpConfigArgs),
    /// Configure an MCP client to launch Fabro
    Init(McpInitArgs),
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct McpStartArgs {
    #[command(flatten)]
    pub(crate) connection: ServerConnectionArgs,
}

#[derive(Args, Debug, Clone, Default)]
pub(crate) struct McpConfigArgs {
    #[command(flatten)]
    pub(crate) connection: ServerConnectionArgs,
}

#[derive(Args, Debug, Clone)]
pub(crate) struct McpInitArgs {
    pub(crate) agent: McpAgent,

    #[command(flatten)]
    pub(crate) connection: ServerConnectionArgs,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub(crate) enum McpAgent {
    Claude,
    Cursor,
    Windsurf,
}
```

Add `Commands::Mcp(McpNamespace)` with help text `Model Context Protocol server`.

In `Commands::name()`:

```rust
Self::Mcp(ns) => match &ns.command {
    McpCommand::Start(_) => "mcp start",
    McpCommand::Config(_) => "mcp config",
    McpCommand::Init(_) => "mcp init",
},
```

In `commands/mod.rs`, add `pub(crate) mod mcp;`.

Create `commands/mcp/mod.rs`:

```rust
use anyhow::Result;

use crate::args::{McpCommand, McpNamespace};
use crate::command_context::CommandContext;

mod config;
mod run_tools;
mod server;

pub(crate) async fn dispatch(ns: McpNamespace, base_ctx: &CommandContext) -> Result<()> {
    match ns.command {
        McpCommand::Start(args) => server::start(args, base_ctx).await,
        McpCommand::Config(args) => config::print_config(args),
        McpCommand::Init(args) => config::init_agent(args),
    }
}
```

Use stub implementations that return `Ok(())` for now, except `start` can `anyhow::bail!("fabro mcp start is not implemented yet")` until Task 3.

In `main.rs`, dispatch:

```rust
Commands::Mcp(ns) => {
    commands::mcp::dispatch(ns, &base_ctx).await?;
}
```

- [ ] **Step 4: Run help tests and accept expected snapshots**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::help cmd::mcp::start_help cmd::mcp::config_help cmd::mcp::init_help
cargo insta pending-snapshots
cargo insta accept
cargo nextest run -p fabro-cli --test it cmd::mcp::help cmd::mcp::start_help cmd::mcp::config_help cmd::mcp::init_help
```

Expected: first run produces snapshots to inspect, final run PASS.

- [ ] **Step 5: Refactor and verify**

Run:

```bash
cargo +nightly-2026-04-14 fmt --all
cargo +nightly-2026-04-14 clippy -p fabro-cli --test it -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add lib/crates/fabro-cli/Cargo.toml lib/crates/fabro-cli/src/args.rs lib/crates/fabro-cli/src/main.rs lib/crates/fabro-cli/src/commands/mod.rs lib/crates/fabro-cli/src/commands/mcp/mod.rs lib/crates/fabro-cli/tests/it/cmd/mod.rs lib/crates/fabro-cli/tests/it/cmd/mcp.rs
git commit -m "feat(cli): add mcp command surface"
```

## Task 2: Implement `fabro mcp config` And `fabro mcp init`

**Files:**
- Modify: `lib/crates/fabro-cli/src/commands/mcp/config.rs`
- Modify: `lib/crates/fabro-cli/tests/it/cmd/mcp.rs`

- [ ] **Step 1: Write failing config/init tests**

Add tests:

```rust
#[test]
fn config_prints_generic_mcp_json() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args(["mcp", "config"]);
    fabro_snapshot!(context.filters(), cmd, @"");
}

#[test]
fn config_preserves_connection_flags() {
    let context = test_context!();
    let mut cmd = context.command();
    cmd.args([
        "mcp",
        "config",
        "--server",
        "https://example.test/api/v1",
        "--storage-dir",
        "/tmp/fabro-mcp-storage",
    ]);
    fabro_snapshot!(context.filters(), cmd, @"");
}

#[test]
fn init_cursor_writes_idempotent_config() {
    let context = test_context!();
    context
        .command()
        .args(["mcp", "init", "cursor"])
        .assert()
        .success();
    context
        .command()
        .args(["mcp", "init", "cursor"])
        .assert()
        .success();

    let config_path = context.home_dir.join(".cursor").join("mcp.json");
    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    fabro_json_snapshot!(context, config, @"");
}

#[test]
fn init_preserves_existing_servers() {
    let context = test_context!();
    let config_path = context.home_dir.join(".cursor").join("mcp.json");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(
        &config_path,
        r#"{"mcpServers":{"other":{"command":"other","args":["serve"]}},"theme":"dark"}"#,
    )
    .unwrap();

    context
        .command()
        .args(["mcp", "init", "cursor", "--server", "https://example.test/api/v1"])
        .assert()
        .success();

    let config: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(config_path).unwrap()).unwrap();
    fabro_json_snapshot!(context, config, @"");
}

#[test]
fn init_invalid_json_fails_without_overwrite() {
    let context = test_context!();
    let config_path = context.home_dir.join(".cursor").join("mcp.json");
    std::fs::create_dir_all(config_path.parent().unwrap()).unwrap();
    std::fs::write(&config_path, "{not json").unwrap();

    let mut cmd = context.command();
    cmd.args(["mcp", "init", "cursor"]);
    fabro_snapshot!(context.filters(), cmd, @"");
    assert_eq!(std::fs::read_to_string(config_path).unwrap(), "{not json");
}
```

Use `fabro_json_snapshot` where the parsed config is the contract. Add the required import:

```rust
use fabro_test::{fabro_json_snapshot, fabro_snapshot, test_context};
```

- [ ] **Step 2: Run tests and verify they fail**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::config_prints_generic_mcp_json cmd::mcp::config_preserves_connection_flags cmd::mcp::init_cursor_writes_idempotent_config cmd::mcp::init_preserves_existing_servers cmd::mcp::init_invalid_json_fails_without_overwrite
```

Expected: FAIL because config/init are stubs.

- [ ] **Step 3: Implement config rendering**

In `commands/mcp/config.rs`, implement:

```rust
use std::path::PathBuf;

use anyhow::{Context as _, Result, anyhow, bail};
use serde_json::{Map, Value, json};

use crate::args::{McpAgent, McpConfigArgs, McpInitArgs, ServerConnectionArgs};
use crate::shared::print_json_pretty;

const SERVER_NAME: &str = "fabro";

pub(crate) fn print_config(args: McpConfigArgs) -> Result<()> {
    print_json_pretty(&generic_config(&args.connection))
}

pub(crate) fn init_agent(args: McpInitArgs) -> Result<()> {
    let path = agent_config_path(args.agent)?;
    let entry = server_entry(&args.connection);
    merge_server_entry(&path, entry)?;
    eprintln!("Configured Fabro MCP server for {} at {}", agent_name(args.agent), path.display());
    Ok(())
}
```

`server_entry(...)` must emit command `fabro` and args built by:

```rust
fn start_args(connection: &ServerConnectionArgs) -> Vec<String> {
    let mut args = vec!["mcp".to_string(), "start".to_string()];
    if let Some(server) = connection.target.server.as_ref() {
        args.push("--server".to_string());
        args.push(server.clone());
    }
    if let Some(storage_dir) = connection.storage_dir.as_deref() {
        args.push("--storage-dir".to_string());
        args.push(storage_dir.display().to_string());
    }
    args
}
```

Implement `merge_server_entry(path, entry)` so it:

- creates the parent directory
- reads existing JSON if the file exists
- rejects invalid JSON with context including the path
- rejects non-object roots and non-object `mcpServers`
- inserts/replaces only `mcpServers.fabro`
- writes pretty JSON plus trailing newline

- [ ] **Step 4: Run config/init tests and accept snapshots**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::config_prints_generic_mcp_json cmd::mcp::config_preserves_connection_flags cmd::mcp::init_cursor_writes_idempotent_config cmd::mcp::init_preserves_existing_servers cmd::mcp::init_invalid_json_fails_without_overwrite
cargo insta pending-snapshots
cargo insta accept
cargo nextest run -p fabro-cli --test it cmd::mcp::config_prints_generic_mcp_json cmd::mcp::config_preserves_connection_flags cmd::mcp::init_cursor_writes_idempotent_config cmd::mcp::init_preserves_existing_servers cmd::mcp::init_invalid_json_fails_without_overwrite
```

Expected: PASS.

- [ ] **Step 5: Refactor and verify**

Run:

```bash
cargo +nightly-2026-04-14 fmt --all
cargo +nightly-2026-04-14 clippy -p fabro-cli --test it -- -D warnings
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add lib/crates/fabro-cli/src/commands/mcp/config.rs lib/crates/fabro-cli/tests/it/cmd/mcp.rs
git commit -m "feat(cli): configure fabro mcp clients"
```

## Task 3: Add MCP Server Skeleton With Protocol Tests

**Files:**
- Modify: `lib/crates/fabro-cli/src/commands/mcp/server.rs`
- Modify: `lib/crates/fabro-cli/src/commands/mcp/run_tools.rs`
- Modify: `lib/crates/fabro-cli/tests/it/cmd/mcp.rs`

- [ ] **Step 1: Write failing stdio protocol test**

Add a test that uses the existing `fabro_mcp::client::McpClient` to spawn the compiled CLI:

```rust
#[tokio::test]
async fn stdio_server_initializes_and_lists_run_tools() {
    let context = test_context!();
    let config = fabro_mcp::config::McpServerSettings {
        name: "fabro-under-test".to_string(),
        transport: fabro_mcp::config::McpTransport::Stdio {
            command: vec![
                env!("CARGO_BIN_EXE_fabro").to_string(),
                "mcp".to_string(),
                "start".to_string(),
            ],
            env: context.command_env(),
        },
        startup_timeout_secs: 10,
        tool_timeout_secs: 30,
    };
    let client = fabro_mcp::client::McpClient::new(&config).unwrap();
    client.initialize(config.startup_timeout()).await.unwrap();

    let tools = client.list_tools().await.unwrap();
    let names: Vec<_> = tools.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec![
            "fabro_run_create",
            "fabro_run_search",
            "fabro_run_interact",
            "fabro_run_gather",
            "fabro_run_events",
        ]
    );
}
```

If `TestContext` does not expose a command env map, add a narrow helper in the test module that mirrors `context.command()` environment by spawning through `Command` only for raw tests, or extend support with a focused helper. Do not use ambient user HOME.

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::stdio_server_initializes_and_lists_run_tools
```

Expected: FAIL because `fabro mcp start` is not implemented.

- [ ] **Step 3: Implement rmcp server skeleton**

In `server.rs`, implement:

```rust
use std::sync::Arc;

use anyhow::Result;
use rmcp::{
    ServerHandler, serve_server,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router,
    transport::stdio,
};
use tokio::sync::OnceCell;

use crate::args::McpStartArgs;
use crate::command_context::CommandContext;
use crate::server_client::Client;

use super::run_tools;

#[derive(Clone)]
pub(crate) struct FabroMcpServer {
    ctx: Arc<CommandContext>,
    client: Arc<OnceCell<Arc<Client>>>,
    tool_router: ToolRouter<Self>,
}

pub(crate) async fn start(args: McpStartArgs, base_ctx: &CommandContext) -> Result<()> {
    let ctx = Arc::new(base_ctx.with_connection(&args.connection)?);
    let server = FabroMcpServer::new(ctx);
    serve_server(server, stdio()).await?;
    Ok(())
}
```

Implement `ServerHandler`:

```rust
impl ServerHandler for FabroMcpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Use these tools to create, inspect, control, wait for, and read events from Fabro workflow runs.")
    }
}
```

Add tool functions with temporary placeholder results:

```rust
#[tool_router]
impl FabroMcpServer {
    pub(crate) fn new(ctx: Arc<CommandContext>) -> Self { ... }

    #[tool(name = "fabro_run_create", description = "...")]
    async fn fabro_run_create(
        &self,
        params: Parameters<run_tools::FabroRunCreateParams>,
    ) -> Result<Json<run_tools::CreateRunsResult>, String> {
        run_tools::create_runs(self.client().await?, params.0).await
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for FabroMcpServer { ... }
```

Each placeholder in `run_tools.rs` should return `Err("not implemented".to_string())` until later tasks, except it must compile and be listed.

Implement `client(&self)` with lazy connection:

```rust
async fn client(&self) -> Result<Arc<Client>, String> {
    self.client
        .get_or_try_init(|| async { self.ctx.server().await.map_err(format_tool_error) })
        .await
        .map(Arc::clone)
}
```

Make `format_tool_error` append auth guidance when `fabro_util::exit::exit_class_for(&err) == Some(ExitClass::AuthRequired)`.

- [ ] **Step 4: Run protocol test**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::stdio_server_initializes_and_lists_run_tools
```

Expected: PASS listing all five tools.

- [ ] **Step 5: Add stdout-purity regression**

Add a raw subprocess test that:

- spawns `fabro mcp start`
- sends a JSON-RPC initialize request on stdin
- reads the first stdout line
- asserts it parses as JSON and has `jsonrpc: "2.0"`
- asserts stderr may contain logs but stdout contains no leading human text

Use a child timeout and kill-on-drop cleanup. The raw JSON should be:

```json
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"fabro-test","version":"0.0.0"}}}
```

- [ ] **Step 6: Run protocol checks**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::stdio_server_initializes_and_lists_run_tools cmd::mcp::stdio_start_writes_only_json_rpc_to_stdout
```

Expected: PASS.

- [ ] **Step 7: Refactor and verify**

Run:

```bash
cargo +nightly-2026-04-14 fmt --all
cargo +nightly-2026-04-14 clippy -p fabro-cli --test it -- -D warnings
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add lib/crates/fabro-cli/src/commands/mcp/server.rs lib/crates/fabro-cli/src/commands/mcp/run_tools.rs lib/crates/fabro-cli/tests/it/cmd/mcp.rs
git commit -m "feat(cli): start fabro mcp stdio server"
```

## Task 4: Implement Run Create/Search Tools

**Files:**
- Modify: `lib/crates/fabro-cli/src/commands/mcp/run_tools.rs`
- Modify: `lib/crates/fabro-cli/src/commands/run/overrides.rs`
- Modify: `lib/crates/fabro-cli/tests/it/cmd/mcp.rs`

- [ ] **Step 1: Write failing create/search integration test**

Add a test backed by an authenticated real Fabro server:

```rust
#[tokio::test]
async fn mcp_create_and_search_manage_real_runs_with_cli_auth() {
    let context = test_context!();
    let harness = RealAuthHarness::start_with_dev_token(fabro_test::GitHubAppState::default()).await;
    let target: fabro_client::ServerTarget = harness.api_base_url.parse().unwrap();
    seed_dev_token_auth(&context.home_dir, &target, TEST_DEV_TOKEN);
    let workflow = context.install_fixture("simple.fabro");

    let client = spawn_mcp_client(&context, &[
        "--server",
        &harness.api_base_url,
    ]).await;

    let create = call_tool_json(&client, "fabro_run_create", serde_json::json!({
        "runs": [{
            "workflow": workflow,
            "dry_run": true,
            "auto_approve": true,
            "labels": { "source": "mcp-test" }
        }]
    })).await;
    let run_id = create["runs"][0]["run_id"].as_str().unwrap().to_string();
    assert_eq!(create["runs"][0]["started"], true);

    let search = call_tool_json(&client, "fabro_run_search", serde_json::json!({
        "run_ids": [run_id],
        "labels": { "source": "mcp-test" },
        "first": 10
    })).await;
    fabro_json_snapshot!(context, normalize_run_search(search), @"");

    harness.shutdown().await;
}
```

If `RealAuthHarness::start_with_dev_token` cannot create runs due missing server settings for local execution, use `TestContext` managed server plus explicit `seed_dev_token_auth` against its server target, but keep the test proving persisted CLI auth is used.

- [ ] **Step 2: Run test and verify it fails**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::mcp_create_and_search_manage_real_runs_with_cli_auth
```

Expected: FAIL because tools return not implemented.

- [ ] **Step 3: Implement shared parameter and result types**

In `run_tools.rs`, define public crate-visible structs for every tool input/output with:

```rust
#[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
pub(crate) struct ...

#[derive(Debug, serde::Serialize, rmcp::schemars::JsonSchema)]
pub(crate) struct ...
```

Use `#[serde(default)]` on optional map fields so omitted maps become empty maps where helpful.

- [ ] **Step 4: Expose manifest override helpers**

In `commands/run/overrides.rs`, make these helpers `pub(crate)` if needed:

- `parse_labels`
- `model_from_args`
- `sandbox_layer`
- `execution_layer`
- `goal_layer_from_args`

If changing visibility creates awkward API, instead add one new crate-visible function:

```rust
pub(crate) fn manifest_overrides_from_parts(input: ManifestOverrideParts<'_>) -> Result<ManifestSettingsOverrides>
```

Prefer the single helper if more than three helpers would need visibility changes.

- [ ] **Step 5: Implement `fabro_run_create`**

Implementation outline:

```rust
pub(crate) async fn create_runs(
    client: Arc<Client>,
    base_cwd: &Path,
    params: FabroRunCreateParams,
) -> Result<CreateRunsResult, String> {
    validate_len("runs", params.runs.len(), 1, 50)?;
    let mut created = Vec::with_capacity(params.runs.len());
    for spec in params.runs {
        let cwd = spec.cwd.clone().unwrap_or_else(|| base_cwd.to_path_buf());
        let run_id = spec.run_id.as_deref().map(str::parse).transpose().map_err(tool_err)?;
        let overrides = build_mcp_manifest_overrides(&spec, &cwd)?;
        let manifest_args = build_manifest_args(&spec);
        let built = build_run_manifest(ManifestBuildInput {
            workflow: PathBuf::from(&spec.workflow),
            cwd,
            run_overrides: overrides.run,
            cli_overrides: overrides.cli,
            input_overrides: overrides.input_overrides,
            args: manifest_args,
            run_id,
            user_settings_path: Some(active_settings_path(None)),
        })?;
        let validation = manifest_validation::validate_manifest(&RunLayer::default(), &built.manifest)?;
        reject_validation_errors(validation)?;
        let run_id = client.create_run_from_manifest(built.manifest).await?;
        let started = spec.start.unwrap_or(true);
        if started {
            client.start_run(&run_id, false).await?;
        }
        let summary = client.retrieve_run(&run_id).await?;
        created.push(CreatedRunResult::from_summary(summary, started));
    }
    Ok(CreateRunsResult { runs: created })
}
```

Important: the function signature in `server.rs` should pass both the lazy API client and the MCP process cwd from the context, not call `std::env::current_dir()` deep in the tool.

- [ ] **Step 6: Implement JSON-to-TOML input conversion**

Add unit tests in `run_tools.rs` for:

- strings
- bools
- integers
- floats
- arrays
- objects
- null rejected with key name

Run:

```bash
cargo nextest run -p fabro-cli mcp::run_tools
```

Expected: PASS after implementation.

- [ ] **Step 7: Implement `fabro_run_search`**

Use existing `server_runs::ServerRunSummaryInfo` where useful, but avoid adding public API only for tests. Search should:

- fetch `client.list_store_runs().await`
- sort newest first using created/start timestamp and run id as tie-breaker
- apply filters
- page with `first` and `after`
- return `SearchRunsResult { runs, next_cursor }`

Do not drop archived runs by default. `archived: Some(false)` should exclude them.

- [ ] **Step 8: Run create/search tests**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::mcp_create_and_search_manage_real_runs_with_cli_auth
```

Expected: PASS.

- [ ] **Step 9: Refactor and verify**

Run:

```bash
cargo +nightly-2026-04-14 fmt --all
cargo +nightly-2026-04-14 clippy -p fabro-cli --test it -- -D warnings
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add lib/crates/fabro-cli/src/commands/mcp/run_tools.rs lib/crates/fabro-cli/src/commands/run/overrides.rs lib/crates/fabro-cli/tests/it/cmd/mcp.rs
git commit -m "feat(cli): add mcp run create and search tools"
```

## Task 5: Implement Interact/Gather/Events Tools

**Files:**
- Modify: `lib/crates/fabro-cli/src/commands/mcp/run_tools.rs`
- Modify: `lib/crates/fabro-cli/tests/it/cmd/mcp.rs`

- [ ] **Step 1: Write failing lifecycle interaction test**

Add a test that:

- creates a dry-run auto-approved run with `fabro_run_create`
- calls `fabro_run_gather` with the run id
- calls `fabro_run_interact` action `get`
- calls `fabro_run_events` action `list`
- calls `fabro_run_interact` action `archive`
- calls `fabro_run_interact` action `unarchive`
- verifies server-visible state through API or a follow-up `fabro_run_search`

Snapshot a normalized object:

```rust
fabro_json_snapshot!(
    context,
    serde_json::json!({
        "gather": normalize_gather(gather),
        "get_status": get["result"]["summary"]["status"],
        "events_nonempty": events["events"].as_array().unwrap().is_empty() == false,
        "archive_action": archive["action"],
        "unarchive_action": unarchive["action"],
    }),
    @""
);
```

- [ ] **Step 2: Write failing validation/error tests**

Add tests for:

- `fabro_run_gather` rejects more than 50 run ids.
- `fabro_run_interact` action `message` without `message` returns an MCP tool error and the server remains alive for a subsequent `fabro_run_search`.
- Missing auth against a protected remote target returns a tool error containing `Run \`fabro auth login\` to authenticate.`

- [ ] **Step 3: Run tests and verify they fail**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::mcp_lifecycle_tools_manage_real_run cmd::mcp::mcp_gather_rejects_too_many_runs cmd::mcp::mcp_interact_error_does_not_stop_server cmd::mcp::mcp_tool_auth_error_mentions_login
```

Expected: FAIL because tools are incomplete.

- [ ] **Step 4: Implement `fabro_run_interact`**

Implement one small function per action:

```rust
async fn interact_get(client: &Client, run_id: &RunId) -> Result<Value>
async fn interact_start(client: &Client, run_id: &RunId) -> Result<Value>
async fn interact_message(client: &Client, run_id: &RunId, message: Option<String>, interrupt: bool) -> Result<Value>
async fn interact_cancel(client: &Client, run_id: &RunId) -> Result<Value>
async fn interact_archive(client: &Client, run_id: &RunId) -> Result<Value>
async fn interact_unarchive(client: &Client, run_id: &RunId) -> Result<Value>
async fn interact_get_questions(client: &Client, run_id: &RunId) -> Result<Value>
async fn interact_answer(client: &Client, run_id: &RunId, question_id: Option<String>, answer: Option<Value>) -> Result<Value>
```

Resolve selectors once:

```rust
let run_id = client.resolve_run(&params.run_id).await?.id;
```

Use `serde_json::to_value(...)` for API objects rather than manually copying complex projection/question structures.

- [ ] **Step 5: Implement answer mapping tests and helper**

Add unit tests for:

```rust
assert_answer(true, SubmitAnswerRequestKind::Yes);
assert_answer(false, SubmitAnswerRequestKind::No);
assert_answer("hello", SubmitAnswerRequestKind::Freeform);
assert_answer(json!({"option":"a"}), SubmitAnswerRequestKind::SingleChoice);
assert_answer(json!({"options":["a","b"]}), SubmitAnswerRequestKind::MultiChoice);
assert_answer(json!({"text":"hello"}), SubmitAnswerRequestKind::Freeform);
```

Use actual generated `fabro_api::types::SubmitAnswerRequest` constructors/builders available in the crate. If generated variants are awkward, inspect the generated type and map exactly to the existing API shape used by `Client::submit_run_answer`.

- [ ] **Step 6: Implement `fabro_run_gather`**

Validation:

```rust
validate_len("run_ids", params.run_ids.len(), 1, 50)?;
let timeout = params.timeout_seconds.unwrap_or(300).min(600);
let poll = params.poll_interval_seconds.unwrap_or(15).max(5);
```

Implementation:

- resolve all selectors at the start
- poll summaries until every `summary.lifecycle.status.is_terminal()` or deadline
- return `timed_out`, `runs`, and `elapsed_seconds`

- [ ] **Step 7: Implement `fabro_run_events`**

Fetch events using:

```rust
let events = client
    .list_run_events(&run_id, params.after, effective_limit_for_fetch(params))
    .await?;
```

Then apply filters in memory:

- event ids
- event types
- categories
- created_after/before
- query substring on serialized event JSON
- offset
- limit/first
- direction
- max_content_length truncation

Output:

```rust
struct RunEventsResult {
    run_id: String,
    action: RunEventsAction,
    events: Vec<RunEventResult>,
    next_cursor: Option<u32>,
}
```

`next_cursor` is last returned sequence plus one when at least one event was returned.

- [ ] **Step 8: Run lifecycle and error tests**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp::mcp_lifecycle_tools_manage_real_run cmd::mcp::mcp_gather_rejects_too_many_runs cmd::mcp::mcp_interact_error_does_not_stop_server cmd::mcp::mcp_tool_auth_error_mentions_login
```

Expected: PASS.

- [ ] **Step 9: Refactor and verify**

Run:

```bash
cargo +nightly-2026-04-14 fmt --all
cargo +nightly-2026-04-14 clippy -p fabro-cli --test it -- -D warnings
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add lib/crates/fabro-cli/src/commands/mcp/run_tools.rs lib/crates/fabro-cli/tests/it/cmd/mcp.rs
git commit -m "feat(cli): add mcp run control tools"
```

## Task 6: Final Contract Coverage And Workspace Verification

**Files:**
- Modify as needed from prior tasks only.

- [ ] **Step 1: Run all MCP command tests**

Run:

```bash
cargo nextest run -p fabro-cli --test it cmd::mcp
```

Expected: PASS.

- [ ] **Step 2: Run existing relevant MCP client tests**

Run:

```bash
cargo nextest run -p fabro-mcp
```

Expected: PASS. This confirms the existing external MCP client crate was not regressed by dependency feature unification.

- [ ] **Step 3: Run relevant existing CLI run/auth tests**

Run:

```bash
cargo nextest run -p fabro-cli --test it scenario::auth::auth_login_refresh_logout_flow scenario::lifecycle::dry_run_create_start_attach_works_with_default_run_lookup cmd::ps::ps_explicit_local_tcp_target_uses_auth_store
```

Expected: PASS. If exact test names drift, use `cargo nextest list -p fabro-cli --test it | rg 'auth_login_refresh_logout_flow|dry_run_create_start_attach|explicit_local_tcp'` and run the matching tests.

- [ ] **Step 4: Run formatting and linting**

Run:

```bash
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 5: Run broader regression suite**

Run:

```bash
ulimit -n 4096 && cargo nextest run --workspace
```

Expected: PASS.

- [ ] **Step 6: Inspect snapshots before accepting any remaining changes**

Run:

```bash
cargo insta pending-snapshots
```

Expected: no pending snapshots. If pending snapshots exist, inspect them before accepting. Only accept snapshots caused by this feature.

- [ ] **Step 7: Final code review pass**

Check manually:

- `fabro mcp start` has no `printout!`, `println!`, `eprintln!` is only for stderr and not in server steady-state startup.
- all MCP tool argument validation returns tool errors, not process exits.
- no tests write run internals directly.
- no live provider credentials are required.
- agent config merge preserves unrelated keys.
- auth failures include login guidance.

- [ ] **Step 8: Commit final fixes if any**

```bash
git status --short
git add <changed-files>
git commit -m "test(cli): cover fabro mcp server contract"
```

Only make this commit if Task 6 produced additional fixes or tests not already committed.
