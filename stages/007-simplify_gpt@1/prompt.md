Goal: # Move GitHub token permissions from `[server.integrations.github]` to `[run.integrations.github]`

## Context

Today `[server.integrations.github.permissions]` controls what scopes Fabro requests on the Installation Access Token it injects into the sandbox as `GITHUB_TOKEN`. This is conceptually wrong:

- Token permissions describe what *this run* is authorized to do, not the server's identity.
- The current key is server-only. `workflow.toml` and `project.toml` cannot override it (`builders.rs:394` strips `server.*` from per-workflow layers), so projects/workflows can't tighten or relax permissions.
- The public docs (`integrations/github.mdx:211-220`) already describe a per-run config, which never existed in code. The intent has always been run-level.

Greenfield app — no migration, no backwards compat. Moving permissions to `[run.integrations.github.permissions]` makes the natural layer-merge (workflow > project > user > defaults) Just Work: server admins set defaults in `~/.fabro/settings.toml`, projects and workflows override.

## Security model

Run config is trusted policy input for sandbox token scopes. The upper bound on what Fabro can mint is the GitHub App installation's granted permissions; Fabro does **not** impose a separate server-side cap. Operators must not run untrusted workflow/project/user TOML against a broadly-scoped App installation. Preflight prints requested permissions so reviewers can see them; no enforcement layer beyond GitHub's own.

## Design

New TOML path: `[run.integrations.github.permissions]`. Creates a fresh `[run.integrations]` namespace for future run-level integration knobs.

**Merge semantics** (presence-aware, hand-rolled). `ReplaceMap` does NOT work here: `maps.rs:76-80` is `if self.0.is_empty() { other } else { self }`, i.e. an empty higher layer falls back to the inherited map. We need empty-wins-as-clear, so we don't reuse `ReplaceMap` (and don't change global semantics for other users).

Layer field: `pub permissions: Option<HashMap<String, InterpString>>`.

| Higher layer | Lower layer | Result |
|---|---|---|
| `None` | anything | lower (inherit) |
| `Some(map)` | anything | `Some(map)` (full replace, including `Some({})` = clear) |

Hand-roll `Combine` on `RunIntegrationsGithubLayer`:
```rust
impl Combine for RunIntegrationsGithubLayer {
    fn combine(self, other: Self) -> Self {
        Self { permissions: self.permissions.or(other.permissions) }
    }
}
```
Don't derive — the blanket `Option<T: Combine>` impl recurses into the inner type and would reintroduce the empty-fallback bug.

Resolved type collapses the Option: `pub permissions: HashMap<String, InterpString>` where empty = no token requested. The presence distinction only matters during merge.

**Interpolation**: keep `InterpString` in the resolved type. Resolve to `String` at the start-services construction boundary, matching the existing pattern at `server.rs:2763-2772`. No early resolution in `resolve_run`.

## Changes

### 1. Config schema — `lib/crates/fabro-config/src/layers/run.rs`

Add new layer types. `RunIntegrationsLayer` derives `Combine` normally; `RunIntegrationsGithubLayer` does NOT — hand-roll `Combine` (see Design section) so `Some({})` is honored as a clear sentinel.

```rust
/// `[run.integrations]` — run-level integration knobs.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, fabro_macros::Combine)]
#[serde(deny_unknown_fields)]
pub struct RunIntegrationsLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github: Option<RunIntegrationsGithubLayer>,
}

/// `[run.integrations.github]` — runtime GitHub token shape.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunIntegrationsGithubLayer {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<HashMap<String, InterpString>>,
}

impl Combine for RunIntegrationsGithubLayer {
    fn combine(self, other: Self) -> Self {
        Self { permissions: self.permissions.or(other.permissions) }
    }
}
```

Add `pub integrations: Option<RunIntegrationsLayer>` to `RunLayer`.

### 2. Config schema — server side

`lib/crates/fabro-config/src/layers/server.rs:226`: remove `permissions: StickyMap<InterpString>` from `GithubIntegrationLayer`. Server struct keeps only identity/auth/webhook fields.

### 3. Resolved types — `lib/crates/fabro-types/src/settings/run.rs`

Resolved types collapse the layer-time `Option` (presence is only meaningful during merge):

```rust
pub struct RunIntegrationsSettings {
    pub github: RunIntegrationsGithubSettings,
}

pub struct RunIntegrationsGithubSettings {
    pub permissions: HashMap<String, InterpString>, // empty = no token
}
```

Add `integrations: RunIntegrationsSettings` to `RunNamespace`. Drop `permissions` from the resolved `GithubIntegrationSettings` in fabro-types.

### 4. Resolver — `lib/crates/fabro-config/src/resolve/run.rs`

Add `fn resolve_integrations(layer: Option<&RunIntegrationsLayer>) -> RunIntegrationsSettings`. Pass through `InterpString`s untouched — do NOT resolve env vars here. Collapse `Option<HashMap<...>>` → `HashMap<...>` (None and Some({}) both become empty).

`lib/crates/fabro-config/src/resolve/server.rs:465`: drop the line that copies `permissions` into the resolved github settings.

### 5. Server consumer — `lib/crates/fabro-server`

Four read sites swap from `server_settings.server.integrations.github.permissions` to `run_spec.settings.run.integrations.github.permissions` (a flat `HashMap<String, InterpString>`; empty = no token):

- `run_manifest.rs:488` — clone-credential gate inside `prepare_manifest`.
- `run_manifest.rs:1184-1235` — `run_github_token_check`. Change signature to take resolved run permissions; resolve `InterpString`s inside the function for both minting and the report.
- `server.rs:2727` — forced-credential gate in run launch path.
- `server.rs:2763-2772` — InterpString → String resolution for `StartServices.github_permissions`. Read from run settings instead of server settings; same resolution logic.

### 5b. Bundled-workflow TOML parsing — `lib/crates/fabro-server/src/run_manifest.rs:290-307`

`root_workflow_run_layer` currently parses the workflow TOML as a raw `toml::Table`, lifts out `run`, and silently discards every other top-level key. That means stale `[server.integrations.github.permissions]` is not caught by `deny_unknown_fields`.

Naive "reject any non-`run` key" would break valid workflow TOML (every workflow.toml in the repo has `_version = 1`; `hello/workflow.toml` has `[workflow]`). `SettingsLayer` (`layers/settings.rs:21-37`) is the schema for a settings file: `_version`, `project`, `workflow`, `run`, `cli`, `server`, `features`.

Fix: parse the source via `source.parse::<SettingsLayer>()` and take `layer.run.unwrap_or_default()`. That gives:
- valid top-level domains parse without error;
- stale `[server.integrations.github.permissions]` fails because `permissions` is no longer a known field on `GithubIntegrationLayer` (section 2) and the layer has `serde(deny_unknown_fields)`;
- shape-valid `[server.*]` in a workflow.toml is silently ignored downstream by `builders.rs:394`, preserving today's behavior (workflow.toml shouldn't set server config, but doesn't blow up either).

`resolve_manifest_dockerfile(&mut run, &config.path, &workflow.files)` (line 305) still runs on the extracted `RunLayer`.

### 6. CLI worker — `lib/crates/fabro-cli/src/commands/run/runner.rs` (P0, was missing)

Two reads on the CLI launch path that today still point at the server-side field:

- `runner.rs:120` — `github_permissions: HashMap::new()` is hardcoded. Source from `run_spec.settings.run.integrations.github.permissions`, applying the same `InterpString` → `String` resolution as the server side.
- `runner.rs:514-555` — `maybe_build_github_credentials`. Line 524 reads `settings.server.integrations.github.permissions.is_empty()` to decide whether credentials are required. Swap to read the run-level permissions. Identity fields (`strategy`, `app_id`, `slug`) at lines 527-538 stay on the server side.

To make this testable, factor two private helpers in `runner.rs`:
- `fn resolve_run_github_permissions(run: &RunNamespace) -> HashMap<String, String>` — InterpString resolution loop, same as the server side. Reuse server-side helper if one exists; otherwise extract a shared one in fabro-config or fabro-server.
- `fn requires_github_credentials(run: &RunNamespace, server: Option<&ServerNamespace>) -> bool` — folds the existing clone/PR/permissions gates.

Both are pure functions of resolved settings; unit-test them in `#[cfg(test)] mod tests` inside `runner.rs`. Don't try to reach private items from an integration test in `tests/it/`.

Without this fix, CLI-launched runs silently get no `GITHUB_TOKEN` regardless of TOML.

Downstream is unchanged: `StartServices.github_permissions` → `SandboxEnvSpec.github_permissions` (`fabro-workflow/src/operations/start.rs:99`, `pipeline/types.rs:225`) → `mint_github_token` + `GITHUB_TOKEN` injection at `pipeline/initialize.rs:240`.

### 7. Parse hints — `lib/crates/fabro-config/src/parse.rs:117-118`

Update the legacy-`[github]` migration hint to distinguish identity vs. permissions. Suggested wording:
- `"github"` → `"split into [server.integrations.github] (App identity/auth) and [run.integrations.github.permissions] (sandbox token scopes)"`.

Also: ensure `[server.integrations.github]` with a `permissions` subkey produces a `deny_unknown_fields` error pointing at the new path. If automatic, no extra code; if not, add a targeted parse-time check.

### 8. OpenAPI — `docs/public/api-reference/fabro-api.yaml`

Wire shape mirrors the resolved Rust types (no `Option`s, no nullability):

- **Remove** `permissions` from `GithubIntegrationSettings` (line 7212): drop the property and remove from `required`.
- **Add** new schemas:
  - `RunIntegrationsSettings`: `properties: { github: { $ref: "#/components/schemas/RunIntegrationsGithubSettings" } }`, `required: ["github"]`.
  - `RunIntegrationsGithubSettings`: `properties: { permissions: { type: object, additionalProperties: { type: string } } }`, `required: ["permissions"]`.
- **Add** `integrations: { $ref: "#/components/schemas/RunIntegrationsSettings" }` to `RunNamespace.properties` and to `RunNamespace.required`.

The collapsed-Option resolved types (section 3) make this clean: `github` is always present, `permissions` is always an object (possibly empty). No `nullable`, no `oneOf`, no `skip_serializing_if` to debate.

Type ownership: progenitor auto-generates new run DTOs (Explore confirmed no `with_replacement` for run types today). No new `with_replacement` entries; do not split into hand-rolled DTOs unless a real semantic divergence emerges. Add a fabro-api JSON parity test asserting OpenAPI's `RunIntegrationsGithubSettings` round-trips through the resolved Rust type, including the empty-permissions case.

Regenerate TS client: `cd lib/packages/fabro-api-client && bun run generate`.

### 9. Repo TOMLs — rewrite

- `.fabro/workflows/gh-triage/workflow.toml`
- `.fabro/workflows/implement-issue/workflow.toml`
- `.fabro/workflows/gh-list/workflow.toml`

Each: `[server.integrations.github.permissions]` → `[run.integrations.github.permissions]`.

### 10. User settings (advisory)

`~/.fabro/settings.toml` should be rewritten to put a default at `[run.integrations.github.permissions]`. Don't auto-edit; cover in verification.

### 11. Docs — `docs/public/integrations/github.mdx`

- Line 35 (overview table): rewrite the row to reference `[run.integrations.github.permissions]`.
- Lines 209-220 ("GITHUB_TOKEN injection"): rewrite. Show canonical run-level path. Note `~/.fabro/settings.toml` is the natural place for server defaults because it's the user layer of the same merge stack. Drop the `[github]` shorthand from line 211 (never existed in code).
- Add the security-model note (boundary = installation grants, no Fabro-side cap).

### 12. Tests

**Layer parsing + merge** (`lib/crates/fabro-config/src/tests/`, parse real TOML at each layer; not just resolver-level fixtures):
- Parse a workflow.toml with `[run.integrations.github.permissions]` — assert the layer round-trips.
- Merge user `{ contents = "read" }` + workflow `{ issues = "write" }` — assert workflow fully replaces: result is `{ issues = "write" }`.
- Merge user `{ contents = "read" }` + workflow absent (no `[run.integrations]` block) — assert inheritance: result is `{ contents = "read" }`.
- Merge user `{ contents = "read" }` + workflow `permissions = {}` — assert clear: resolved permissions is empty.
- Negative: `[server.integrations.github.permissions]` in user/project/workflow TOML must error via `deny_unknown_fields`.
- Negative (bundled workflow path, P2): targeted test exercising `root_workflow_run_layer` (`run_manifest.rs:290`) with a workflow.toml containing a stale `[server.integrations.github.permissions]` block — assert it errors via `deny_unknown_fields` after the rewrite to parse through `SettingsLayer`. Pair with a positive test asserting `_version = 1` and a `[workflow]` block still parse cleanly through this code path.

**Resolver** (`resolve/run.rs` tests): assert `InterpString` is preserved in resolved settings, not flattened to `String`. Assert resolved `permissions` is `HashMap<String, InterpString>` (Option collapsed).

**Preflight** (`run_manifest.rs:1822` and surrounds):
- Add a case where the run config sets `permissions = { issues = "read" }` and the GitHub App is configured — assert `run_github_token_check` reports `Pass`.
- Update `server_settings_fixture` (`run_manifest.rs:1390`) usage in any test that previously set permissions through it; rewrite to set via run layer.

**CLI worker path** (P0): unit-test the new private helpers in `runner.rs`'s `#[cfg(test)] mod tests`:
- `resolve_run_github_permissions` — given a `RunNamespace` with `InterpString` permissions referencing env vars, returns the resolved `HashMap<String, String>`.
- `requires_github_credentials` — exercises each truth-table case (clone needed, PR enabled, permissions non-empty, all-absent).
- Do not try to assert `StartServices.github_permissions` from an integration test — those internals are private. End-to-end behavior is covered by the smoke runs in Verification.

## Verification

1. `cargo build --workspace` clean.
2. `cargo nextest run -p fabro-config` — layer/resolve tests pass, including the new override + replace-semantics + deny-unknown tests.
3. `cargo nextest run -p fabro-server -p fabro-cli` — preflight + run-manifest + worker tests pass.
4. `cargo nextest run -p fabro-api` — JSON parity test for the new run integrations schema passes.
5. Smoke run via server (manual):
   - Update `~/.fabro/settings.toml` to put `[run.integrations.github.permissions]` defaults (`pull_requests = "read"`, `issues = "read"`).
   - Restart `fabro server`.
   - `fabro run gh-list --no-retro`; both stages exit 0 with PR/issue listings on stdout.
   - `fabro logs <id>` shows `gh pr list` returning data; no "populate the GH_TOKEN" error.
6. Smoke run via CLI worker path (manual): same as above but with a `fabro run` invocation that uses the local-CLI worker path (not HTTP). Confirms P0 fix.
7. Override test: in `gh-list/workflow.toml`, set `permissions = { issues = "write" }` over a server default of `read`. Confirm the minted-token preflight summary shows `issues: write` and not `read`.
8. Tightening test: in another workflow, set `permissions = {}`. Confirm preflight reports no token requested and the sandbox env has no `GITHUB_TOKEN`.
9. `cd apps/fabro-web && bun run typecheck && bun test` — generated TS client compiles against the new schema.

## Open questions

1. Anything else worth promoting to `[run.integrations.*]` now (Slack, Discord run-time config)? Recommendation: leave empty until a concrete need lands; don't speculate.
2. Should preflight surface the *resolved* permission strings in its report, or the raw `InterpString` source? Recommendation: resolved, so reviewers see what the App will actually be asked for; treat unresolved env-var fallbacks as a preflight warning.
3. `resolve_run_github_permissions` location — should the InterpString-resolution loop live in fabro-config (shared by server + CLI worker), or in each consumer? Recommendation: extract to fabro-config / fabro-types alongside the resolved type, so server (`server.rs:2763-2772`) and CLI (`runner.rs`) both call the same helper. Avoids two copies drifting.


## Completed stages
- **toolchain**: succeeded
  - Script: `command -v cargo >/dev/null || { curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y && sudo ln -sf $HOME/.cargo/bin/* /usr/local/bin/; }; cargo --version 2>&1`
  - Stdout:
    ```
    cargo 1.95.0 (f2d3ce0bd 2026-03-21)
    ```
  - Stderr: (empty)
- **preflight_compile**: succeeded
  - Script: `cargo check -q --workspace 2>&1`
  - Stdout: (empty)
  - Stderr: (empty)
- **preflight_lint**: succeeded
  - Script: `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings 2>&1`
  - Stdout: (empty)
  - Stderr: (empty)
- **implement**: succeeded
  - Model: claude-opus-4-7, 74.0k tokens in / 7.3k out
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 100.8k tokens in / 21.6k out
  - Files: /home/daytona/workspace/lib/crates/fabro-api/build.rs, /home/daytona/workspace/lib/crates/fabro-cli/src/commands/run/runner.rs, /home/daytona/workspace/lib/crates/fabro-config/src/layers/combine.rs, /home/daytona/workspace/lib/crates/fabro-config/src/layers/run.rs, /home/daytona/workspace/lib/crates/fabro-server/src/server.rs, /home/daytona/workspace/lib/crates/fabro-server/src/server/tests.rs


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