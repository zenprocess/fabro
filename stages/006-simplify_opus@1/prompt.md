Goal: # Provider-Backed Sandbox API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Expose provider-backed sandbox inventory APIs that list and retrieve Fabro-managed sandboxes directly from sandbox providers, outside the context of runs.

**Architecture:** Split sandbox responsibilities into a provider-level `SandboxProvider` trait and an instantiated `Sandbox` handle. `SandboxProvider` owns provider inventory and lifecycle escape hatches (`list`, `get`, `create`, `delete`); `Sandbox` keeps in-sandbox capabilities such as file IO, exec, terminal, preview URLs, and git operations. The API fans out across configured providers and never uses run projection state as the source of truth for sandbox list/get.

**Tech Stack:** Rust, Axum, OpenAPI/progenitor, generated TypeScript Axios client, Bollard for Docker, Daytona Rust SDK, existing Fabro managed labels.

---

## Decisions

- Use greenfield naming: `SandboxProvider` is the trait, `SandboxProviderKind` is the enum discriminator (`local`, `docker`, `daytona`).
- `GET /api/v1/sandboxes/{id}` treats `{id}` as a provider-native id and searches all configured providers.
- Return `409 Conflict` when more than one provider returns a sandbox for the same id.
- List and get return only Fabro-managed sandboxes; provider implementations must filter or verify `sh.fabro.managed=true`.
- `SandboxProvider::delete` exists for cleanup of provider resources that cannot be instantiated as `Sandbox`; it is not exposed through public API in this plan.
- `local` has no provider-managed inventory and should return an empty list plus `None` for get.

## API And Types

**Files:**
- Modify: `docs/public/api-reference/fabro-api.yaml`
- Create: `lib/crates/fabro-types/src/sandbox_inventory.rs`
- Modify: `lib/crates/fabro-types/src/lib.rs`
- Test: `lib/crates/fabro-api/tests/sandbox_inventory_round_trip.rs`

- [ ] **Add OpenAPI paths**

  Add:

  ```yaml
  /api/v1/sandboxes:
    get:
      operationId: listSandboxes
      tags: [Sandboxes]
      summary: List Sandboxes
      description: Lists Fabro-managed sandboxes directly from configured sandbox providers.
      responses:
        "200":
          description: Provider-backed sandbox inventory
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/SandboxListResponse"

  /api/v1/sandboxes/{id}:
    get:
      operationId: retrieveSandbox
      tags: [Sandboxes]
      summary: Retrieve Sandbox
      description: Retrieves a Fabro-managed sandbox by provider-native id by searching all configured sandbox providers.
      parameters:
        - in: path
          name: id
          required: true
          schema:
            type: string
      responses:
        "200":
          description: Sandbox found
          content:
            application/json:
              schema:
                $ref: "#/components/schemas/SandboxInfo"
        "404":
          description: No provider found a Fabro-managed sandbox with this id
        "409":
          description: More than one provider matched this sandbox id
        "502":
          description: Provider lookup failed before a definitive result could be determined
  ```

- [ ] **Add API schemas**

  Add schemas for:

  ```yaml
  SandboxProviderKind:
    type: string
    enum: [local, docker, daytona]

  SandboxInfo:
    required: [provider, id, state, resources, network, labels, timestamps]
    properties:
      provider:
        $ref: "#/components/schemas/SandboxProviderKind"
      id:
        type: string
      display_name:
        type: string
        nullable: true
      state:
        $ref: "#/components/schemas/SandboxState"
      native_state:
        type: string
        nullable: true
      image:
        type: string
        nullable: true
      snapshot:
        type: string
        nullable: true
      region:
        type: string
        nullable: true
      web_url:
        type: string
        nullable: true
      working_directory:
        type: string
        nullable: true
      resources:
        $ref: "#/components/schemas/SandboxResources"
      network:
        $ref: "#/components/schemas/SandboxNetwork"
      labels:
        type: object
        additionalProperties:
          type: string
      timestamps:
        $ref: "#/components/schemas/SandboxTimestamps"

  SandboxProviderLookupError:
    required: [provider, message]
    properties:
      provider:
        $ref: "#/components/schemas/SandboxProviderKind"
      message:
        type: string

  SandboxListMeta:
    required: [provider_errors]
    properties:
      provider_errors:
        type: array
        items:
          $ref: "#/components/schemas/SandboxProviderLookupError"

  SandboxListResponse:
    required: [data, meta]
    properties:
      data:
        type: array
        items:
          $ref: "#/components/schemas/SandboxInfo"
      meta:
        $ref: "#/components/schemas/SandboxListMeta"
  ```

- [ ] **Add canonical Rust DTOs**

  Create `lib/crates/fabro-types/src/sandbox_inventory.rs` with Rust structs mirroring the OpenAPI schemas. Reuse existing `SandboxState`, `SandboxResources`, `SandboxNetwork`, and `SandboxTimestamps`.

- [ ] **Wire generated API replacement tests**

  Add `with_replacement` mappings in `lib/crates/fabro-api/build.rs` for the new canonical DTOs and prove JSON parity in `lib/crates/fabro-api/tests/sandbox_inventory_round_trip.rs`.

## Provider Runtime Layer

**Files:**
- Create: `lib/crates/fabro-sandbox/src/provider.rs`
- Create: `lib/crates/fabro-sandbox/src/provider/docker.rs`
- Create: `lib/crates/fabro-sandbox/src/provider/daytona.rs`
- Modify: `lib/crates/fabro-sandbox/src/lib.rs`
- Test: `lib/crates/fabro-sandbox/src/provider.rs`

- [ ] **Define `SandboxProvider` and registry**

  Add:

  ```rust
  #[async_trait::async_trait]
  pub trait SandboxProvider: Send + Sync {
      fn kind(&self) -> SandboxProviderKind;

      async fn list(&self, filter: SandboxListFilter) -> crate::Result<Vec<SandboxInfo>>;
      async fn get(&self, id: &str) -> crate::Result<Option<SandboxInfo>>;
      async fn create(&self, spec: SandboxCreateSpec) -> crate::Result<SandboxInfo>;
      async fn delete(&self, id: &str) -> crate::Result<()>;
  }
  ```

  Add `SandboxProviderRegistry` with:

  ```rust
  pub async fn list_managed(&self) -> SandboxListResponse;
  pub async fn get_managed_by_native_id(&self, id: &str) -> Result<SandboxInfo, SandboxLookupError>;
  ```

- [ ] **Define registry lookup semantics**

  Implement:

  - list: call every configured provider, concatenate successful results, and put failed providers in `meta.provider_errors`.
  - get: call every configured provider. Return the single match, return not-found if all providers succeed with no match, return conflict if two or more providers match, and return provider-unavailable if no provider matches and at least one provider failed.

- [ ] **Keep `Sandbox` instance behavior unchanged**

  Do not move file IO, exec, terminal, preview, or git methods onto `SandboxProvider`. Those remain on the instantiated `Sandbox` trait.

## Provider Implementations

**Files:**
- Modify: `lib/crates/fabro-sandbox/src/docker.rs`
- Modify: `lib/crates/fabro-sandbox/src/daytona/mod.rs`
- Modify: `lib/crates/fabro-sandbox/src/details.rs`
- Test: `lib/crates/fabro-sandbox/src/details.rs`

- [ ] **Docker provider inventory**

  Implement Docker inventory without constructing a `DockerSandbox` instance:

  - `list`: use Bollard to list containers filtered by `label=sh.fabro.managed=true`.
  - `get`: inspect by provider-native id or name; return `None` when missing or unmanaged.
  - `delete`: inspect first and verify managed labels before removing.
  - `create`: create a managed Docker sandbox and return `SandboxInfo`.

- [ ] **Daytona provider inventory**

  Implement Daytona inventory through the SDK:

  - `list`: call SDK list with label filter `{ "sh.fabro.managed": "true" }`.
  - `get`: call SDK get and return `None` if the sandbox does not carry the managed label.
  - `delete`: fetch first and verify managed label before deleting.
  - `create`: create a managed Daytona sandbox and return `SandboxInfo`.

- [ ] **Share detail mapping**

  Refactor existing Docker/Daytona detail mapping so run-scoped `SandboxDetails` and provider-backed `SandboxInfo` use the same normalization for state, resources, network, labels, timestamps, image/snapshot, region, and provider URL.

## Server API

**Files:**
- Create: `lib/crates/fabro-server/src/server/handler/sandboxes.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/mod.rs`
- Modify: `lib/crates/fabro-server/src/server.rs`
- Test: `lib/crates/fabro-server/src/server/handler/sandboxes.rs`

- [ ] **Register routes**

  Add a new top-level handler module:

  ```rust
  Router::new()
      .route("/sandboxes", get(list_sandboxes))
      .route("/sandboxes/{id}", get(retrieve_sandbox))
  ```

  Merge it into the `/api/v1` router beside existing run, model, system, and human-in-the-loop routes.

- [ ] **Use provider registry from app state**

  Add the provider registry to server state initialization. The registry should include enabled providers only, built from server/environment settings and secrets.

- [ ] **Use run-management authorization**

  Require the same broad run-management authorization used by run listing, because the sandbox inventory crosses individual run boundaries and may expose provider resource metadata.

- [ ] **Map registry errors to HTTP**

  Map:

  - single match: `200`
  - no matches and no provider failures: `404`
  - duplicate provider matches: `409`
  - no match plus at least one provider failure: `502`

## Verification

**Files:**
- Test: `lib/crates/fabro-types/tests/sandbox_inventory_serde.rs`
- Test: `lib/crates/fabro-api/tests/sandbox_inventory_round_trip.rs`
- Test: `lib/crates/fabro-sandbox/src/provider.rs`
- Test: `lib/crates/fabro-server/src/server/handler/sandboxes.rs`

- [ ] **Type and API tests**

  Run:

  ```bash
  cargo test -p fabro-types sandbox_inventory
  cargo test -p fabro-api sandbox_inventory_round_trip
  ```

- [ ] **Provider registry tests**

  Cover these cases with fake providers:

  - list returns aggregate data from two successful providers.
  - list includes provider error metadata when one provider fails.
  - get returns one matching sandbox.
  - get returns not-found when all providers succeed with no match.
  - get returns conflict when two providers match the same native id.
  - get returns provider-unavailable when no provider matches and one provider fails.

- [ ] **Server route tests**

  Cover:

  - `GET /api/v1/sandboxes` returns provider-backed data without reading run projections.
  - `GET /api/v1/sandboxes/{id}` searches all configured providers.
  - duplicate ids return `409`.
  - provider lookup uncertainty returns `502`.

- [ ] **Workspace checks**

  Run:

  ```bash
  cargo +nightly-2026-04-14 fmt --check --all
  cargo nextest run -p fabro-sandbox -p fabro-server -p fabro-api -p fabro-types
  cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
  ```

## Assumptions

- This plan uses greenfield naming and allows `SandboxProvider` to mean the provider control-plane trait.
- The enum discriminator is `SandboxProviderKind`, not `SandboxProvider`.
- Public API v1 exposes only list and get; create and delete are trait capabilities for provider internals and cleanup.
- Provider-native id search across all providers is intentional; duplicate matches are reported as ambiguity rather than guessed.
- Provider-backed inventory intentionally does not depend on run records, run projections, or preserved-run handoff state.


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
  - Model: gpt-5.5, 1.8m tokens in / 10.4k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/sandboxes.rs


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