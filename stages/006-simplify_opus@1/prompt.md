Goal: # Named Environments Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Replace run-scoped sandbox configuration with named, provider-explicit environments that runs can select by slug.

**Architecture:** Add a shared top-level environment catalog, resolve a selected environment into the run's dense settings, validate provider capabilities, and convert the resolved environment into the existing sandbox runtime specs. Keep "environment" as reusable desired configuration and "sandbox" as the concrete runtime instance created for a run.

**Tech Stack:** Rust config/types crates, TOML settings layers, Fabro workflow sandbox providers, OpenAPI-generated clients, public docs.

---

## Summary

Replace run-scoped sandbox configuration with named, provider-explicit environments. A run selects an environment by slug via `[run.environment] id = "..."`; Fabro resolves the environment catalog through normal config precedence, applies run-level environment overrides, validates provider capabilities, freezes the resolved environment into the run settings, and creates a concrete sandbox instance from it.

This is a greenfield break: no `[run.sandbox]` compatibility layer, no server policy layer, and no required/optional volume semantics.

## Key Interface Changes

- Add top-level `[environments.<slug>]` to the shared settings schema. It is valid in `settings.toml`, `.fabro/project.toml`, and `workflow.toml`.
- Replace sandbox selection with:

```toml
[run.environment]
id = "fabro-dev"
```

- Allow sparse run-level overrides under the same table:

```toml
[run.environment.resources]
memory = "32GB"

[run.environment.lifecycle]
preserve = true
```

- Environment shape:

```toml
[environments.fabro-dev]
provider = "daytona" # local | docker | daytona

[environments.fabro-dev.image]
ref = "fabro-v11"              # Docker image or Daytona snapshot name
dockerfile = { path = "Dockerfile" }

[environments.fabro-dev.resources]
cpu = 8
memory = "16GB"
disk = "20GB"

[environments.fabro-dev.network]
mode = "block"                # allow_all | block | cidr_allow_list
allow = ["10.0.0.0/8"]

[environments.fabro-dev.lifecycle]
preserve = false
stop_on_terminal = true
auto_stop = "30m"

[environments.fabro-dev.labels]
repo = "fabro-sh/fabro"

[[environments.fabro-dev.volumes]]
id = "vol-agent-state"
mount_path = "/home/daytona/agent-state"
subpath = "auth"

[environments.fabro-dev.env]
NODE_ENV = "development"
```

- Built-in default becomes:

```toml
[run.environment]
id = "default"

[environments.default]
provider = "docker"

[environments.default.image]
ref = "buildpack-deps:noble"

[environments.default.resources]
cpu = 2
memory = "4GB"

[environments.default.lifecycle]
preserve = false
stop_on_terminal = true
```

## Implementation Changes

- Add environment sparse and dense types:
  - Sparse layer in `fabro-config` for `EnvironmentLayer`, `RunEnvironmentLayer`, image/resources/network/lifecycle/volume sublayers, and `[environments]` as a `MergeMap`.
  - Dense types in `fabro-types` for `EnvironmentSettings`, `RunEnvironmentSettings`, `EnvironmentProvider`, `EnvironmentNetworkMode`, and related subsettings.
  - Add `environments` to the top-level `SettingsLayer` and resolved `WorkflowSettings`; add selected `environment` to `RunNamespace`.
- Resolve environments before run consumers use sandbox data:
  - Merge environment definitions by slug.
  - Resolve `[run.environment].id`; error if the slug is missing.
  - Overlay sparse `[run.environment.*]` fields onto the selected environment.
  - Validate provider is `local`, `docker`, or `daytona`.
  - Validate CIDRs with existing `ipnet`.
  - Store the selected resolved environment in `RunNamespace.environment`.
- Replace sandbox runtime mapping:
  - Convert `RunNamespace.environment` to `SandboxSpec` in workflow start and server preflight paths.
  - Daytona: `image.ref` maps to snapshot name, `dockerfile` to snapshot Dockerfile, resources to snapshot sizing, network to Daytona policy, labels/volumes/env/lifecycle to existing provider fields.
  - Docker: `image.ref` maps to Docker image, `cpu` maps to `cpu_quota = cpu * 100000`, memory maps to memory limit, `network.mode = block` maps to `network_mode = none`, `allow_all` maps to default/bridge.
  - Local: use resolved working directory; env overlays process env as today.
- Capability diagnostics:
  - Hard error for explicit security/isolation properties a provider cannot enforce:
    - local with `network.mode = block` or `cidr_allow_list`
    - docker with `network.mode = cidr_allow_list`
  - Warnings only for unsupported resource limits, volumes, labels, `auto_stop`, and Docker `image.dockerfile`.
  - If Daytona has `image.dockerfile` without `image.ref`, error because snapshot creation needs a name.
- Remove old sandbox config surface:
  - Delete `[run.sandbox]` parsing/resolution/types from user-facing config.
  - Replace CLI/API/tool manifest args named `sandbox` with `environment` where they select execution profile.
  - Keep runtime/public "sandbox" terminology only for concrete instances, e.g. `fabro sandbox ssh`, `RunSandbox`, sandbox details.
- Update docs and generated clients:
  - Update run configuration, environments, Daytona, server configuration, CLI reference, and OpenAPI spec.
  - Regenerate Rust API types/client and TypeScript API client after OpenAPI changes.

## Test Plan

- Config tests:
  - default resolves to `run.environment.id = "default"` and Docker environment settings.
  - project/workflow/run layers merge environment catalog by slug.
  - `[run.environment]` overrides selected environment fields.
  - `env` and `labels` merge by key; `volumes` replace wholesale.
  - missing environment slug errors.
  - old `[run.sandbox]` is rejected as an unknown field.
- Provider mapping tests:
  - Daytona environment maps to snapshot/resources/network/labels/volumes/env.
  - Docker environment maps image, CPU, memory, network block, and env.
  - Local environment ignores non-security unsupported fields with warnings.
- Validation tests:
  - docker plus CIDR allow-list errors.
  - local plus blocked network errors.
  - resource limits unsupported by provider produce warnings, not errors.
  - volumes unsupported by provider produce warnings, not errors.
  - Daytona dockerfile without image ref errors.
- Integration/API tests:
  - run manifest with `[environments.<slug>]` and `[run.environment]` starts with the selected provider.
  - Dockerfile path bundling works from environment image config.
  - preflight reports capability warnings and security errors.
  - CLI/API `environment` override wins over config selection.

## Assumptions

- No compatibility behavior is required for `[run.sandbox]` or `--sandbox`.
- No server-side environment policy or quota enforcement is in scope.
- Volumes are simple provider hints; unsupported volume config warns and continues.
- Resource limits are best-effort hints; unsupported resource fields warn and continue.
- Provider names remain explicit for now: `local`, `docker`, and `daytona`.


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
- **implement**: failed

## Context
- failure_class: transient_infra
- failure_signature: implement|transient_infra|api_transient|unknown|stream


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