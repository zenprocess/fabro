Goal: ---
title: "feat: Give fabro_tools runs MCP tool parity"
type: feat
status: active
date: 2026-05-24
---

# feat: Give fabro_tools runs MCP tool parity

## Overview

When a workflow run opts in with `[run.agent] fabro_tools = true`, its agents
should see the same Fabro run-management tool catalog that a human MCP client
sees: create, search, get, interact, gather, events, and pair.

This is MCP tool parity, not full user API parity. The implementation should
simplify the current permission model by replacing the ad hoc "run tools"
extractor names with explicit run-management actor extractors. User/admin HTTP
surfaces that are not backed by Fabro MCP tools remain user-only.

One intentional exception to exact parity remains: workflow-agent
`fabro_run_create` must keep today's forced-child behavior. Runs created from a
workflow agent are always parented to the current run.

## Problem Frame

Today there are two similar but different tool catalogs:

- Human MCP clients get seven tools from `fabro-mcp-server`, including
  `fabro_run_pair`.
- Workflow agents with `fabro_tools = true` get six shared tool definitions
  from `fabro_tool::tool_definitions()`, excluding `fabro_run_pair`.

The auth model also leaks implementation detail into handler names:
`RequiredRunToolActor` and `RequireRunScopedOrRunTools` describe a historical
scope shape rather than the product capability. The behavior we want is simpler:
an authenticated human or an opted-in run-tools worker may perform
run-management actions exposed through the Fabro MCP tool surface.

## Requirements

- R1. Workflow agents with `fabro_tools = true` register `fabro_run_pair` in
  addition to the existing six Fabro run-management tools.
- R2. Workflow-agent `fabro_run_create` still forces the current run as parent
  and rejects conflicting explicit `parent_id` values.
- R3. The external Fabro MCP server tool list remains unchanged.
- R4. Pair HTTP routes accept run-management actors, not only users, so
  `fabro_run_pair` can work from workflow-agent tools.
- R5. User-only APIs remain user-only. Do not make `RequiredUser` accept worker
  principals.
- R6. Permission code uses names that match the product concept:
  run-management actor / target, not "run scoped or run tools".
- R7. Ask Fabro remains read-only and run-scoped with only `fabro_run_get` and
  `fabro_run_events`.

## Scope Boundaries

In scope:

- Shared Fabro tool catalog and workflow-agent tool registration.
- `fabro_run_pair` dispatcher integration in `fabro-workflow`.
- Server auth extractors for MCP-backed run-management endpoints.
- Pair route auth migration to the new run-management extractor.
- Docs updates for agent/MCP parity and the create-parent exception.

Out of scope:

- Treating worker tokens as generic user tokens.
- Granting workers access to secrets, server/system settings, billing, models,
  sandbox management, logs/files/artifacts, arbitrary event append, or other
  user/admin HTTP APIs.
- Changing Ask Fabro's read-only tool policy.
- Changing the worker JWT scope string or minting flow beyond names/tests needed
  for the run-management extractor cleanup.
- Removing the forced-child behavior for workflow-agent `fabro_run_create`.

## Technical Design

### Shared Tool Catalog

`lib/crates/fabro-tool/src/common.rs` should include
`FABRO_RUN_PAIR_TOOL_NAME` in `TOOL_DEFINITIONS`, using
`FabroRunPairParams` and the same description already used by
`fabro-mcp-server`.

This makes `register_fabro_run_tools()` in `fabro-workflow` register all seven
tools for workflow agents. `register_named_fabro_run_tools()` continues to
filter by name, so Ask Fabro remains restricted to its existing read-only list.

### Workflow Agent Execution

`lib/crates/fabro-workflow/src/handler/llm/api.rs` should add a
`FABRO_RUN_PAIR_TOOL_NAME` match arm in `execute_fabro_run_tool`:

- Parse `FabroRunPairParams`.
- Validate with `ValidatedPairRun`.
- Call `fabro_tool::pair_run`.
- Render the normal summary and structured result.

Do not change the `fabro_run_create` branch except for test updates caused by
the catalog growing. It must still call `ensure_current_run_parent` and pass
`CreateRunOptions { forced_parent_id: Some(current_run_id) }`.

### Run-Management Auth Model

In `lib/crates/fabro-server/src/principal_middleware.rs`, replace the current
run-tools-specific extractor names with product-level names:

- `RequiredRunManagementActor(pub Principal)`
- `RequireRunManagementTarget(pub RunId, pub Principal)`

Recommended semantics:

- `RequiredRunManagementActor` accepts a user principal or a worker principal
  whose token has `agent:run_tools`. It rejects base worker tokens.
- `RequireRunManagementTarget` accepts:
  - any user principal,
  - a same-run base worker principal,
  - any worker principal with `agent:run_tools`, including cross-run targets.
- Non-authenticated and invalid-token behavior should preserve the current
  auth rejection status/code behavior.

Use these names in route handlers that are directly backing the Fabro MCP
run-management tools. Remove or stop exporting the old
`RequiredRunToolActor` and `RequireRunScopedOrRunTools` names once callers are
migrated.

### Route Migrations

Migrate these route groups to the new run-management actor names without
changing behavior:

- Run collection/resolve/create endpoints used by `fabro_run_create` and
  `fabro_run_search`.
- Run parent link/unlink, run status, run state, questions, answer, start,
  cancel, archive, unarchive, steer/message, and event-list endpoints used by
  `fabro_run_get`, `fabro_run_interact`, and `fabro_run_events`.

Migrate pair routes in `lib/crates/fabro-server/src/server/handler/pair.rs`:

- `get_pair_status`, `get_pair`, and `get_transcript` use
  `RequireRunManagementTarget`.
- `start_pair`, `send_pair_message`, and `end_pair` also use
  `RequireRunManagementTarget` and pass the returned `Principal` through to the
  worker control transport.
- Do not construct `Principal::User(auth.0)` in pair handlers after migration.

Do not migrate endpoints whose behavior is not part of the Fabro MCP tool
surface. In particular, leave approve, deny, pause, unpause, retry, rewind,
fork, delete, batch actions, timeline, settings, logs, files, artifacts,
secrets, server/system, models, sandbox, billing, and graph rendering on their
existing user or run-scoped auth rules unless they are already needed by the
current tool backend.

### Documentation

Update public docs where `fabro_tools` is described:

- State that opted-in workflow agents get the same Fabro run-management MCP tool
  catalog as human MCP clients.
- Explicitly document the workflow-agent create exception: created runs are
  children of the current run.
- Keep the distinction from normal agent permissions and external MCP server
  configuration.

## Test Plan

### `fabro-tool`

- Update the shared tool-definition test coverage to expect seven tools,
  including `fabro_run_pair`.
- Assert the pair tool schema includes the expected action enum and stage/pair
  fields.

### `fabro-workflow`

- Update `agent_run_tools_register_exact_shared_definitions` to expect
  `fabro_run_pair`.
- Add executor coverage for `fabro_run_pair` proving it dispatches to the
  shared backend and renders the summary/result.
- Keep or add coverage proving workflow-agent create still injects the current
  run as parent and still rejects conflicting `parent_id`.
- Confirm `register_named_fabro_run_tools` still registers only requested names
  so Ask Fabro is unaffected.

### `fabro-server`

- Add/rename principal middleware tests:
  - run-management actor accepts users and `agent:run_tools` workers.
  - run-management actor rejects base worker tokens.
  - run-management target accepts same-run base workers.
  - run-management target accepts cross-run `agent:run_tools` workers.
  - run-management target rejects cross-run base workers.
- Extend existing run-tool worker API tests to cover the migrated extractor
  names without broadening non-tool surfaces.
- Add pair route auth tests:
  - a run-tools worker can call pair status/transcript endpoints for another
    run.
  - a run-tools worker reaches pair command domain logic, such as
    `worker_control_unavailable`, rather than failing auth.
  - a cross-run base worker remains forbidden.
- Add a negative test that a run-tools worker still cannot call at least one
  user-only non-MCP endpoint, such as approve/deny or timeline.

### `fabro-cli` / MCP Integration

- Existing `stdio_server_initializes_and_lists_run_tools` should remain green
  and continue to validate the external human MCP catalog.
- Add or update integration coverage only if the shared catalog change affects
  agent-visible tool listing snapshots or MCP schema parity tests.

### Commands

Targeted verification:

```bash
cargo nextest run -p fabro-tool -p fabro-workflow -p fabro-server -p fabro-cli
```

Full verification before merge if the route migration touches broad auth code:

```bash
cargo nextest run --workspace
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

## Implementation Notes

- Prefer renaming and consolidating auth extractors over adding another layer of
  compatibility aliases. The goal is to make handler signatures read like the
  product policy.
- Keep actor provenance as `Principal::Worker { run_id: <originating run> }`
  when a workflow agent acts through `fabro_tools`; do not forge a user
  principal.
- Pair route behavior may return domain errors when no live worker control
  channel exists. Tests should assert auth acceptance by expecting those domain
  errors, not by requiring a fully active pair session unless a fixture already
  supports it.
- The external MCP server already registers `fabro_run_pair` directly. Avoid
  duplicating tool catalogs there; use the shared `fabro-tool` definitions only
  where workflow-agent registration needs them.


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


Read the plan file referenced in the goal and implement every step. Make all the code changes described in the plan. Use red/green TDD.