Goal: # Run Agent Fabro Tools Opt-In Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `[run.agent] fabro_tools = true/false`, defaulting to `false`, so workflow agents only get Fabro run tools and the `agent:run_tools` worker JWT scope when a run opts in.

**Architecture:** Treat `run.agent.fabro_tools` as the source of truth in resolved run settings. The server reads the effective run setting before spawning `__run-worker`, issues the worker token with or without `agent:run_tools`, and passes a private worker env flag so the CLI worker registers Fabro run tools only for opted-in runs. Server-side JWT scope checks remain the authorization backstop.

**Tech Stack:** Rust, Serde TOML config layers, Fabro worker JWT scopes, Tokio subprocess spawning, `cargo nextest`.

---

## File Map

- Modify `lib/crates/fabro-types/src/settings/run.rs`: add the resolved `RunAgentSettings::fabro_tools` boolean.
- Modify `lib/crates/fabro-config/src/layers/run.rs`: add the optional layered `[run.agent] fabro_tools` field and options metadata.
- Modify `lib/crates/fabro-config/src/resolve/run.rs`: resolve missing config to `false`.
- Modify `lib/crates/fabro-config/src/tests/resolve_run.rs`: cover default, true, false, and layer override behavior.
- Modify `lib/crates/fabro-static/src/env_vars.rs`: add a typed internal worker env var name.
- Modify `lib/crates/fabro-server/src/worker_token.rs`: make `WorkerScopeSet::run_worker()` available to production code.
- Modify `lib/crates/fabro-server/src/server.rs`: compute the opt-in flag from the run spec, choose worker JWT scopes, and pass the worker env flag.
- Modify `lib/crates/fabro-server/src/server/tests.rs`: update worker command tests for default and opted-in scope/env behavior.
- Modify `lib/crates/fabro-cli/src/commands/run/runner.rs`: gate `FabroRunToolServices` construction on the worker env flag and add unit coverage for the env parser.
- Modify docs generator/reference docs: `lib/crates/fabro-dev/src/commands/docs_options_reference.rs`, `docs/public/reference/user-configuration.mdx`, and `docs/public/execution/run-configuration.mdx`.

---

### Task 1: Add Resolved Run Config

**Files:**
- Modify: `lib/crates/fabro-types/src/settings/run.rs`
- Modify: `lib/crates/fabro-config/src/layers/run.rs`
- Modify: `lib/crates/fabro-config/src/resolve/run.rs`
- Test: `lib/crates/fabro-config/src/tests/resolve_run.rs`

- [ ] **Step 1: Write config resolver tests first**

Add a `run_agent_fabro_tools` test module to `lib/crates/fabro-config/src/tests/resolve_run.rs` near the existing run settings tests.

```rust
mod run_agent_fabro_tools {
    use crate::layers::Combine;
    use crate::{SettingsLayer, WorkflowSettingsBuilder};

    fn parse_settings(source: &str) -> SettingsLayer {
        source
            .parse::<SettingsLayer>()
            .expect("fixture should parse via SettingsLayer")
    }

    #[test]
    fn defaults_to_false_when_run_agent_is_absent() {
        let settings = WorkflowSettingsBuilder::from_layer(&SettingsLayer::default())
            .expect("empty settings should resolve")
            .run;

        assert!(!settings.agent.fabro_tools);
    }

    #[test]
    fn resolves_true_from_run_agent_table() {
        let settings = WorkflowSettingsBuilder::from_toml(
            r#"
_version = 1

[run.agent]
fabro_tools = true
"#,
        )
        .expect("run.agent.fabro_tools should resolve");

        assert!(settings.run.agent.fabro_tools);
    }

    #[test]
    fn resolves_explicit_false_from_run_agent_table() {
        let settings = WorkflowSettingsBuilder::from_toml(
            r#"
_version = 1

[run.agent]
fabro_tools = false
"#,
        )
        .expect("run.agent.fabro_tools false should resolve");

        assert!(!settings.run.agent.fabro_tools);
    }

    #[test]
    fn higher_layer_false_overrides_lower_true() {
        let workflow = parse_settings(
            r#"
_version = 1

[run.agent]
fabro_tools = false
"#,
        );
        let user = parse_settings(
            r#"
_version = 1

[run.agent]
fabro_tools = true
"#,
        );
        let merged = workflow.combine(user);

        let settings = WorkflowSettingsBuilder::from_layer(&merged)
            .expect("merged settings should resolve")
            .run;

        assert!(!settings.agent.fabro_tools);
    }
}
```

- [ ] **Step 2: Run the new tests and confirm they fail**

Run:

```bash
cargo nextest run -p fabro-config run_agent_fabro_tools
```

Expected: compile failure mentioning `fabro_tools` is not a field, or parse failure saying `fabro_tools` is unknown.

- [ ] **Step 3: Add the resolved setting**

Update `RunAgentSettings` in `lib/crates/fabro-types/src/settings/run.rs`:

```rust
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RunAgentSettings {
    pub fabro_tools: bool,
    pub permissions: Option<AgentPermissions>,
    pub mcps:        HashMap<String, McpServerSettings>,
}
```

- [ ] **Step 4: Add the layered TOML field**

Update `RunAgentLayer` in `lib/crates/fabro-config/src/layers/run.rs`:

```rust
/// `[run.agent]` — agent knobs only (Fabro tools, permissions, MCPs).
#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Serialize,
    Deserialize,
    fabro_macros::Combine,
    fabro_macros::OptionsMetadata,
)]
#[serde(deny_unknown_fields)]
pub struct RunAgentLayer {
    /// Allow workflow agents to use Fabro run-management tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[option(default = "false", value_type = "boolean")]
    pub fabro_tools: Option<bool>,

    /// Default tool permission level for workflow agents.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[option(
        default = "\"read-write\"",
        value_type = "\"read-only\" | \"read-write\" | \"full\""
    )]
    pub permissions: Option<AgentPermissions>,

    /// Agent-scoped MCP server entries, keyed by name.
    #[serde(default, skip_serializing_if = "StickyMap::is_empty")]
    #[option(value_type = "table")]
    pub mcps:        StickyMap<McpEntryLayer>,
}
```

- [ ] **Step 5: Resolve the setting**

Update `resolve_agent` in `lib/crates/fabro-config/src/resolve/run.rs`:

```rust
fn resolve_agent(agent: Option<&RunAgentLayer>) -> RunAgentSettings {
    let Some(agent) = agent else {
        return RunAgentSettings::default();
    };

    RunAgentSettings {
        fabro_tools: agent.fabro_tools.unwrap_or(false),
        permissions: agent.permissions,
        mcps:        agent
            .mcps
            .iter()
            .map(|(name, entry)| (name.clone(), resolve_mcp_entry(name, entry)))
            .collect(),
    }
}
```

- [ ] **Step 6: Run config tests**

Run:

```bash
cargo nextest run -p fabro-config run_agent_fabro_tools
```

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add lib/crates/fabro-types/src/settings/run.rs lib/crates/fabro-config/src/layers/run.rs lib/crates/fabro-config/src/resolve/run.rs lib/crates/fabro-config/src/tests/resolve_run.rs
git commit -m "feat: add run agent fabro tools setting"
```

---

### Task 2: Gate Worker JWT Scope and Worker Env

**Files:**
- Modify: `lib/crates/fabro-static/src/env_vars.rs`
- Modify: `lib/crates/fabro-server/src/worker_token.rs`
- Modify: `lib/crates/fabro-server/src/server.rs`
- Test: `lib/crates/fabro-server/src/server/tests.rs`

- [ ] **Step 1: Write server tests for default and opted-in runs**

Update `worker_command_always_sets_worker_token_env` in `lib/crates/fabro-server/src/server/tests.rs` so the default case expects only `run:worker`. Add a second test that passes `agent_fabro_tools_enabled = true` and expects both scopes plus the worker env flag.

```rust
#[cfg(unix)]
#[test]
fn worker_command_default_token_omits_agent_run_tools_scope() {
    let storage_dir = tempfile::tempdir().unwrap();
    let state = worker_command_test_state(storage_dir.path(), &["dev-token"], Some(TEST_DEV_TOKEN));
    let run_id = RunId::new();

    let cmd = worker_command(
        state.as_ref(),
        run_id,
        RunExecutionMode::Start,
        storage_dir.path(),
        false,
    )
    .unwrap();

    let EnvOverride::Set(token) = command_env_value(&cmd, EnvVars::FABRO_WORKER_TOKEN) else {
        panic!("worker token env should be set");
    };
    let claims = jsonwebtoken::decode::<crate::worker_token::WorkerTokenClaims>(
        &token,
        state.worker_token_keys().decoding_key(),
        state.worker_token_keys().validation(),
    )
    .expect("worker token should decode")
    .claims;

    assert_eq!(claims.scope.split_whitespace().collect::<Vec<_>>(), vec!["run:worker"]);
    assert_eq!(
        command_env_value(&cmd, EnvVars::FABRO_WORKER_AGENT_RUN_TOOLS),
        EnvOverride::Removed
    );
}

#[cfg(unix)]
#[test]
fn worker_command_opt_in_token_includes_agent_run_tools_scope() {
    let storage_dir = tempfile::tempdir().unwrap();
    let state = worker_command_test_state(storage_dir.path(), &["dev-token"], Some(TEST_DEV_TOKEN));
    let run_id = RunId::new();

    let cmd = worker_command(
        state.as_ref(),
        run_id,
        RunExecutionMode::Start,
        storage_dir.path(),
        true,
    )
    .unwrap();

    let EnvOverride::Set(token) = command_env_value(&cmd, EnvVars::FABRO_WORKER_TOKEN) else {
        panic!("worker token env should be set");
    };
    let claims = jsonwebtoken::decode::<crate::worker_token::WorkerTokenClaims>(
        &token,
        state.worker_token_keys().decoding_key(),
        state.worker_token_keys().validation(),
    )
    .expect("worker token should decode")
    .claims;

    assert_eq!(
        claims.scope.split_whitespace().collect::<Vec<_>>(),
        vec!["run:worker", "agent:run_tools"]
    );
    assert_eq!(
        command_env_value(&cmd, EnvVars::FABRO_WORKER_AGENT_RUN_TOOLS),
        EnvOverride::Set("true".to_string())
    );
}
```

- [ ] **Step 2: Run the server tests and confirm they fail**

Run:

```bash
cargo nextest run -p fabro-server worker_command_
```

Expected: compile failure because `FABRO_WORKER_AGENT_RUN_TOOLS` and the new `worker_command` argument do not exist.

- [ ] **Step 3: Add the internal env var constant**

Update `lib/crates/fabro-static/src/env_vars.rs` near `FABRO_WORKER_TOKEN`:

```rust
pub const FABRO_WORKER_AGENT_RUN_TOOLS: &'static str = "FABRO_WORKER_AGENT_RUN_TOOLS";
pub const FABRO_WORKER_TOKEN: &'static str = "FABRO_WORKER_TOKEN";
```

Update the EnvVars tests in the same file so the new constant is included in the alphabetized/core variable expectations.

- [ ] **Step 4: Make the base worker scope constructor available**

Update `lib/crates/fabro-server/src/worker_token.rs`:

```rust
impl WorkerScopeSet {
    #[must_use]
    pub(crate) const fn run_worker() -> Self {
        Self {
            agent_run_tools: false,
        }
    }

    #[must_use]
    pub(crate) const fn run_worker_with_agent_run_tools() -> Self {
        Self {
            agent_run_tools: true,
        }
    }
}
```

Remove only the `#[cfg(test)]` attribute from `run_worker`; leave the existing tests intact.

- [ ] **Step 5: Add the worker command parameter and choose scopes**

Update `worker_command` in `lib/crates/fabro-server/src/server.rs`:

```rust
fn worker_command(
    state: &AppState,
    run_id: RunId,
    mode: RunExecutionMode,
    run_dir: &std::path::Path,
    agent_fabro_tools_enabled: bool,
) -> anyhow::Result<Command> {
    // existing setup...
    let scopes = if agent_fabro_tools_enabled {
        WorkerScopeSet::run_worker_with_agent_run_tools()
    } else {
        WorkerScopeSet::run_worker()
    };
    let worker_token = issue_worker_token_with_scopes(state.worker_token_keys(), &run_id, scopes)
        .map_err(|_| anyhow::anyhow!("failed to sign worker token"))?;

    // existing Command construction...
    cmd.env_remove(EnvVars::FABRO_WORKER_TOKEN);
    cmd.env(EnvVars::FABRO_WORKER_TOKEN, worker_token);
    cmd.env_remove(EnvVars::FABRO_WORKER_AGENT_RUN_TOOLS);
    if agent_fabro_tools_enabled {
        cmd.env(EnvVars::FABRO_WORKER_AGENT_RUN_TOOLS, "true");
    }
    // existing GitHub key forwarding...
}
```

Update all `worker_command(...)` test call sites to pass `false` unless the test is explicitly about Fabro tool opt-in.

- [ ] **Step 6: Load the effective run setting before spawning the worker**

Update `execute_run_subprocess` in `lib/crates/fabro-server/src/server.rs` after `open_run` succeeds and before `spawn_blocking`:

```rust
let run_state = match run_store.state().await {
    Ok(run_state) => run_state,
    Err(err) => {
        tracing::error!(run_id = %run_id, error = %err, "Failed to load run state");
        fail_managed_run(
            &state,
            run_id,
            FailureReason::WorkflowError,
            format!("Failed to load run state: {err}"),
        );
        state.scheduler_notify.notify_one();
        return;
    }
};
let agent_fabro_tools_enabled = run_state.spec.settings.run.agent.fabro_tools;
```

Pass the boolean into `worker_command` inside the existing `spawn_blocking` closure:

```rust
worker_command(
    state_for_build.as_ref(),
    run_id,
    execution_mode,
    &run_dir_for_build,
    agent_fabro_tools_enabled,
)
```

- [ ] **Step 7: Run server tests**

Run:

```bash
cargo nextest run -p fabro-server worker_command
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add lib/crates/fabro-static/src/env_vars.rs lib/crates/fabro-server/src/worker_token.rs lib/crates/fabro-server/src/server.rs lib/crates/fabro-server/src/server/tests.rs
git commit -m "feat: gate worker run tool scope by run setting"
```

---

### Task 3: Gate CLI Worker Tool Registration

**Files:**
- Modify: `lib/crates/fabro-cli/src/commands/run/runner.rs`
- Test: `lib/crates/fabro-cli/src/commands/run/runner.rs`

- [ ] **Step 1: Add a focused test for the env gate**

Add a `#[cfg(test)]` module at the bottom of `lib/crates/fabro-cli/src/commands/run/runner.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::fabro_run_tools_enabled_from_env;

    #[test]
    fn fabro_run_tools_enabled_env_requires_true() {
        assert!(!fabro_run_tools_enabled_from_env(None));
        assert!(!fabro_run_tools_enabled_from_env(Some("")));
        assert!(!fabro_run_tools_enabled_from_env(Some("false")));
        assert!(!fabro_run_tools_enabled_from_env(Some("1")));
        assert!(fabro_run_tools_enabled_from_env(Some("true")));
    }
}
```

- [ ] **Step 2: Run the new test and confirm it fails**

Run:

```bash
cargo nextest run -p fabro-cli fabro_run_tools_enabled_env_requires_true
```

Expected: compile failure because `fabro_run_tools_enabled_from_env` does not exist.

- [ ] **Step 3: Add the env parsing helper**

Add this helper near `build_fabro_run_tool_services` in `lib/crates/fabro-cli/src/commands/run/runner.rs`:

```rust
fn fabro_run_tools_enabled_from_env(value: Option<&str>) -> bool {
    value == Some("true")
}
```

- [ ] **Step 4: Gate service construction in worker startup**

Replace the unconditional `build_fabro_run_tool_services(...)` call in `execute` with:

```rust
let fabro_run_tools = if fabro_run_tools_enabled_from_env(process_env_var(
    EnvVars::FABRO_WORKER_AGENT_RUN_TOOLS,
).as_deref()) {
    build_fabro_run_tool_services(
        worker_token,
        client.clone_for_reuse(),
        run_id,
        run_spec.source_directory.as_deref(),
        &run_dir,
        Arc::clone(&catalog),
    )
} else {
    None
};
```

Keep `build_fabro_run_tool_services` returning `None` for an empty token. That keeps token presence as a second local guard.

- [ ] **Step 5: Run CLI tests**

Run:

```bash
cargo nextest run -p fabro-cli fabro_run_tools_enabled_env_requires_true
cargo nextest run -p fabro-cli --test it runner
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add lib/crates/fabro-cli/src/commands/run/runner.rs
git commit -m "feat: register fabro run tools only when opted in"
```

---

### Task 4: Update Docs and Generated Reference Text

**Files:**
- Modify: `lib/crates/fabro-dev/src/commands/docs_options_reference.rs`
- Modify: `docs/public/reference/user-configuration.mdx`
- Modify: `docs/public/execution/run-configuration.mdx`

- [ ] **Step 1: Update the docs generator sample**

Update the `[run.agent]` sample in `docs_options_reference.rs`:

```rust
Section::of::<fabro_config::RunAgentLayer>(
    "[run.agent]",
    r#"[run.agent]
fabro_tools = true
permissions = "read-write""#,
),
```

- [ ] **Step 2: Update generated/reference docs**

In `docs/public/reference/user-configuration.mdx`, update the `[run.agent]` description, example, and options table:

```mdx
## `[run.agent]`

`[run.agent]` — agent knobs only (Fabro tools, permissions, MCPs)

```toml
[run.agent]
fabro_tools = true
permissions = "read-write"
```

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `fabro_tools` | boolean | false | Allow workflow agents to use Fabro run-management tools. |
| `mcps` | table | None | Agent-scoped MCP server entries, keyed by name. |
| `permissions` | "read-only" \| "read-write" \| "full" | "read-write" | Default tool permission level for workflow agents. |
```

- [ ] **Step 3: Add user-facing run configuration docs**

In `docs/public/execution/run-configuration.mdx`, add a short section before the existing `[run.agent.mcps]` section:

```mdx
### `[run.agent]`

Configure workflow agent behavior that is not tied to a single stage.

```toml
[run.agent]
fabro_tools = true
```

`fabro_tools` defaults to `false`. Set it to `true` only for runs whose agents should be able to create, search, inspect, and interact with Fabro runs through the built-in Fabro run tools. This setting is separate from normal agent `permissions` and from MCP server configuration.
```

- [ ] **Step 4: Run docs/reference checks**

Run:

```bash
cargo dev docs check
```

Expected before regenerating docs: FAIL with `docs/public/reference/user-configuration.mdx is stale; run cargo dev docs refresh`.

Then run:

```bash
cargo dev docs refresh
cargo dev docs check
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add lib/crates/fabro-dev/src/commands/docs_options_reference.rs docs/public/reference/user-configuration.mdx docs/public/execution/run-configuration.mdx
git commit -m "docs: document run agent fabro tools opt in"
```

---

### Task 5: Full Verification

**Files:**
- No source edits unless verification finds a defect.

- [ ] **Step 1: Run targeted package tests**

Run:

```bash
cargo nextest run -p fabro-config
cargo nextest run -p fabro-server
cargo nextest run -p fabro-cli
```

Expected: all PASS.

- [ ] **Step 2: Run formatting check**

Run:

```bash
cargo +nightly-2026-04-14 fmt --check --all
```

Expected: PASS.

- [ ] **Step 3: Run clippy for touched Rust crates**

Run:

```bash
cargo +nightly-2026-04-14 clippy -p fabro-types -p fabro-config -p fabro-static -p fabro-server -p fabro-cli -p fabro-dev --all-targets -- -D warnings
```

Expected: PASS.

- [ ] **Step 4: Manually review behavior**

Confirm these invariants in the final diff:

```text
Default run:
- resolved run.agent.fabro_tools == false
- worker JWT scope == "run:worker"
- FABRO_WORKER_AGENT_RUN_TOOLS is absent from worker env
- StartServices.fabro_run_tools == None

Opted-in run:
- resolved run.agent.fabro_tools == true
- worker JWT scope == "run:worker agent:run_tools"
- FABRO_WORKER_AGENT_RUN_TOOLS == "true"
- StartServices.fabro_run_tools is Some(...)
```

- [ ] **Step 5: Commit verification fixes**

When verification changes files, inspect the exact paths and commit them:

```bash
git status --short
git add -u
git commit -m "test: cover run agent fabro tools opt in"
```

---

## Assumptions and Defaults

- `fabro_tools` is a per-run opt-in setting only; this plan does not add a separate server-wide allow/deny policy.
- Defaulting to `false` intentionally changes existing behavior: runs that need Fabro run tools must set `[run.agent] fabro_tools = true`.
- `run.agent.permissions` remains about ordinary agent tool permissions and does not imply Fabro API access.
- `[run.agent.mcps]` remains independent; MCP tools are not enabled or disabled by `fabro_tools`.
- `fabro mcp start` and standalone MCP exposure of Fabro tools are out of scope.
- The private worker env var uses the exact string `"true"` as the only enabling value, so accidental values such as `"1"` or `"yes"` do not grant tools.
- The hidden `__run-worker` CLI argument contract should not grow; use the env var rather than a new hidden CLI flag.


## Completed stages
- **toolchain**: succeeded
  - Script: `command -v cargo >/dev/null || { curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && sudo ln -sf $HOME/.cargo/bin/* /usr/local/bin/; }; cargo --version 2>&1`
  - Output:
    ```
    cargo 1.95.0 (f2d3ce0bd 2026-03-21)
    ```
- **preflight_compile**: succeeded
  - Script: `cargo check -q --workspace 2>&1`
  - Output: (empty)
- **preflight_lint**: succeeded
  - Script: `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings 2>&1`
  - Output: (empty)
- **implement**: succeeded
  - Model: claude-opus-4-7, 114.4k tokens in / 19.7k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-cli/src/commands/run/runner.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/tests.rs, /home/daytona/workspace/fabro/lib/crates/fabro-static/src/env_vars.rs
- **simplify_opus**: failed

## Context
- failure_class: transient_infra
- failure_signature: simplify_opus|transient_infra|api_transient|unknown|stream


# Simplify: Code Review and Cleanup

Review changes vs. origin for reuse, quality, and efficiency. Fix any issues found.

## Phase 1: Identify Changes

Run git diff (or git diff HEAD if there are staged changes) to see what changed. If there are no git changes, review the most recently modified files that the user mentioned or that you edited earlier in this conversation.

## Phase 2: Launch Three Review Agents in Parallel

Use the Agent tool to launch all three agents concurrently in a single message. Pass each agent the full diff so it has the complete context.

### Agent 1: Code Reuse Review

For each change:

1. Search for existing utilities and helpers that could replace newly written code. Use Grep to find similar patterns elsewhere in the codebase — common locations are utility directories, shared modules, and files adjacent to the changed ones.
2. Flag any new function that duplicates existing functionality. Suggest the existing function to use instead.
3. Flag any inline logic that could use an existing utility — hand-rolled string manipulation, manual path handling, custom environment checks, ad-hoc type guards, and similar patterns are common candidates.

Note: This is a greenfield app, so focus on maximizing simplicity and don't worry about changing things to achieve it.

### Agent 2: Code Quality Review

Review the same changes for hacky patterns:

1. Redundant state: state that duplicates existing state, cached values that could be derived, observers/effects that could be direct calls
2. Parameter sprawl: adding new parameters to a function instead of generalizing or restructuring existing ones
3. Copy-paste with slight variation: near-duplicate code blocks that should be unified with a shared abstraction
4. Leaky abstractions: exposing internal details that should be encapsulated, or breaking existing abstraction boundaries
5. Stringly-typed code: using raw strings where constants, enums (string unions), or branded types already exist in the codebase

Note: This is a greenfield app, so be aggressive in optimizing quality.

### Agent 3: Efficiency Review

Review the same changes for efficiency:

1. Unnecessary work: redundant computations, repeated file reads, duplicate network/API calls, N+1 patterns
2. Missed concurrency: independent operations run sequentially when they could run in parallel
3. Hot-path bloat: new blocking work added to startup or per-request/per-render hot paths
4. Unnecessary existence checks: pre-checking file/resource existence before operating (TOCTOU anti-pattern) — operate directly and handle the error
5. Memory: unbounded data structures, missing cleanup, event listener leaks
6. Overly broad operations: reading entire files when only a portion is needed, loading all items when filtering for one

## Phase 3: Fix Issues

Wait for all three agents to complete. Aggregate their findings and fix each issue directly. If a finding is a false positive or not worth addressing, note it and move on — do not argue with the finding, just skip it.

When done, briefly summarize what was fixed (or confirm the code was already clean).