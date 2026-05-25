Goal: # Automations Backend API Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the backend data model and REST API for creating, editing, deleting, starting, and listing runs for Automations.

**Architecture:** Automations are server-owned runnable bindings stored as one canonical TOML file per automation in `dirname(active_config_path)/automations/<id>.toml`. The server loads those files into an in-memory store at startup, persists API mutations atomically, and attaches an automation reference to runs created through the automation API. Schedule triggers are stored and validated, but no cron scheduler or background trigger loop is added in this plan.

**Tech Stack:** Rust, serde, toml, toml_edit, sha2, hex, croner for schedule validation only, Axum, OpenAPI/progenitor, existing Fabro run manifest and run creation pipeline.

---

## Locked Decisions

- Backend only: do not add web UI routes/components and do not add CLI commands.
- Storage root: `dirname(active_config_path)/automations`.
- File layout: one automation per file, `automations/<id>.toml`.
- Canonical ID: the filename stem. The TOML file does not repeat `id`.
- Automation ID format: `[a-z0-9][a-z0-9-]{0,62}`.
- Trigger ID format: `[a-z0-9][a-z0-9_-]{0,62}`.
- Trigger IDs are required, user-visible, editable, and unique within one automation.
- Triggers are an array from v1.
- The API trigger type is `api`, not `manual_api`. Trigger IDs remain user-visible and editable; examples use `id = "api"` but startability is based on `type = "api"`.
- At most one trigger with `type = "api"` is allowed per automation.
- Multiple `schedule` triggers are allowed.
- Unknown trigger types, including future `event` shapes, return `422` in v1. Handlers must not let unknown trigger discriminators fail as JSON parse errors.
- If an automation is disabled, or it has no enabled trigger with `type = "api"`, `POST /automations/{id}/runs` returns `409` and does not create a run.
- API writes canonicalize TOML and may discard comments in automation files.
- No runtime automation state store or derived automation status API is added in V1. Run history is available through `GET /automations/{id}/runs`; schedule expressions are validated but not evaluated for scheduling.

## File Structure

Create:

- `lib/crates/fabro-automation/Cargo.toml` - domain crate manifest.
- `lib/crates/fabro-automation/src/lib.rs` - public exports.
- `lib/crates/fabro-automation/src/error.rs` - validation and persistence errors.
- `lib/crates/fabro-automation/src/id.rs` - `AutomationId` and `AutomationTriggerId`.
- `lib/crates/fabro-automation/src/model.rs` - automation domain and serde/TOML model.
- `lib/crates/fabro-automation/src/store.rs` - in-memory file-backed automation store.
- `lib/crates/fabro-server/src/automation_materializer.rs` - GitHub target materialization and manifest building for automation runs.
- `lib/crates/fabro-server/src/server/handler/automations.rs` - REST handlers and router.
- `lib/crates/fabro-server/tests/it/api/automations.rs` - server API integration tests.
- `lib/crates/fabro-server/tests/it/api/mod.rs` - wire the automations integration test module.

Modify:

- `lib/crates/fabro-server/Cargo.toml` - add `fabro-automation`.
- `lib/crates/fabro-api/Cargo.toml` - add `fabro-automation` so OpenAPI can reuse matching automation domain types.
- `lib/crates/fabro-types/src/run_summary.rs` - extend `AutomationRef` with `trigger_id`.
- `lib/crates/fabro-types/src/run.rs` - add `automation: Option<AutomationRef>` to `RunSpec`.
- `lib/crates/fabro-types/src/run_event/run.rs` - add `automation: Option<AutomationRef>` to `RunCreatedProps`.
- `lib/crates/fabro-workflow/src/operations/create.rs` - carry automation metadata through `CreateRunInput`, persistence options, `RunSpec`, and `run.created`.
- `lib/crates/fabro-workflow/src/event/convert.rs` - preserve automation metadata in any legacy-to-current event conversion path that constructs `RunCreatedProps`.
- `lib/crates/fabro-store/src/run_state.rs` - project `RunSpec.automation` into `Run.automation`.
- `lib/crates/fabro-server/src/server.rs` - load the automation store into `AppState` and expose crate-private accessors.
- `lib/crates/fabro-server/src/server/handler/mod.rs` - merge real automation routes.
- `lib/crates/fabro-server/src/test_support.rs` - create temp automation storage by active config path and allow test-only materializer injection.
- `docs/public/api-reference/fabro-api.yaml` - add automation paths and schemas.
- `lib/crates/fabro-api/build.rs` - add replacement mappings only for domain types with identical wire shape.
- `lib/crates/fabro-api/tests/*` - add JSON parity tests for reused automation types.
- `lib/packages/fabro-api-client` - regenerate generated TypeScript client files only; do not import them from the web UI.

Do not modify:

- `apps/fabro-web/**`, except generated API package consumers are not touched.
- CLI command modules.
- Scheduler services or background run loops.

## Public API Shape

Add these OpenAPI paths under `/api/v1`:

```http
GET    /automations
POST   /automations
GET    /automations/{id}
PUT    /automations/{id}
PATCH  /automations/{id}
DELETE /automations/{id}
GET    /automations/{id}/runs
POST   /automations/{id}/runs
```

Use this response model:

```ts
type Automation = {
  id: string;
  revision: string;
  name: string;
  description: string | null;
  enabled: boolean;
  target: AutomationTarget;
  triggers: AutomationTrigger[];
};

type AutomationTarget = {
  repository: string; // GitHub owner/repo
  ref: string;
  workflow: string;
};

type AutomationTrigger =
  | { id: string; type: "api"; enabled: boolean }
  | { id: string; type: "schedule"; enabled: boolean; expression: string };

```

Request models:

```ts
type CreateAutomationRequest = {
  id: string;
  name: string;
  description?: string | null;
  enabled?: boolean;
  target: AutomationTarget;
  triggers: AutomationTrigger[];
};

type ReplaceAutomationRequest = {
  name: string;
  description?: string | null;
  enabled: boolean;
  target: AutomationTarget;
  triggers: AutomationTrigger[];
};

type PatchAutomationRequest = {
  name?: string;
  description?: string | null;
  enabled?: boolean;
  target?: AutomationTarget;
  triggers?: AutomationTrigger[];
};
```

`GET /automations/{id}/runs` returns the existing paginated run list envelope:

```json
{
  "data": [],
  "meta": { "has_more": false, "total": 0 }
}
```

It accepts `page[limit]` and `page[offset]`, sorts newest first, filters by `Run.automation.id`, and returns `404` if the automation definition no longer exists.

`POST /automations/{id}/runs` returns the existing `Run` response shape with `automation` populated:

```json
{
  "automation": {
    "id": "nightly-deps",
    "name": "Nightly dependency update",
    "trigger_id": "api"
  }
}
```

## TOML Shape

Persist this canonical TOML:

```toml
name = "Nightly dependency update"
description = "Open a PR for dependency updates."
enabled = true

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "dependency-update"

[[triggers]]
id = "api"
type = "api"
enabled = false

[[triggers]]
id = "nightly"
type = "schedule"
enabled = true
expression = "0 3 * * *"
```

Defaults:

- `enabled` defaults to `true` when omitted in TOML or create requests.
- `description` defaults to `null`.
- Trigger `enabled` defaults to `true` when omitted in TOML or create requests.
- `schedule.expression` must be a non-empty five-field cron expression accepted by `croner`.
- `target.repository` must be a GitHub `owner/repo` slug using the existing server slug validation rules: owner max 39 chars, repo max 100 chars, no path traversal or separators inside either segment.
- `target.ref` must be a non-empty branch, tag, or SHA selector and must not start with `-`, contain ASCII control characters, or contain shell/path traversal metacharacters that would make git argv ambiguous.
- `target.workflow` is a Fabro workflow selector resolved inside the cloned repository with `WorkflowLocation::resolve`; it may be a workflow slug such as `dependency-update` or a relative workflow path, but absolute paths and `..` path traversal are invalid.

## Task 1: Add Domain Crate And Model Tests

**Files:**

- Create: `lib/crates/fabro-automation/Cargo.toml`
- Create: `lib/crates/fabro-automation/src/lib.rs`
- Create: `lib/crates/fabro-automation/src/error.rs`
- Create: `lib/crates/fabro-automation/src/id.rs`
- Create: `lib/crates/fabro-automation/src/model.rs`

- [ ] Read `docs/internal/testing-strategy.md` and `docs/internal/error-handling-strategy.md` before adding tests and error types.
- [ ] Create the crate. Because the workspace uses `members = ["lib/crates/*"]`, no root workspace member edit is required.
- [ ] Add dependencies in `lib/crates/fabro-automation/Cargo.toml`: `chrono`, `croner`, `hex`, `serde`, `sha2`, `thiserror`, `tokio`, `toml`, and `toml_edit`. Add dev-dependencies: `tempfile`.
- [ ] Define `AutomationId` and `AutomationTriggerId` newtypes with `TryFrom<String>`, `AsRef<str>`, `Display`, `Serialize`, and `Deserialize`.
- [ ] Define the domain model with this public shape:

```rust
pub struct AutomationRevision(String);

pub struct RepositorySlug(String);

pub struct GitRefSelector(String);

pub struct WorkflowSlug(String);

pub struct Automation {
    pub id: AutomationId,
    pub revision: AutomationRevision,
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub target: AutomationTarget,
    pub triggers: Vec<AutomationTrigger>,
}

pub struct AutomationTarget {
    pub repository: RepositorySlug,
    pub ref_: GitRefSelector,
    pub workflow: WorkflowSlug,
}

#[serde(tag = "type", rename_all = "snake_case")]
pub enum AutomationTrigger {
    Api(ApiTrigger),
    Schedule(ScheduleTrigger),
}

pub struct ApiTrigger {
    pub id: AutomationTriggerId,
    pub enabled: bool,
}

pub struct ScheduleTrigger {
    pub id: AutomationTriggerId,
    pub enabled: bool,
    pub expression: String,
}

pub struct AutomationDraft {
    pub id: AutomationId,
    pub name: String,
    pub description: Option<String>,
    pub enabled: Option<bool>,
    pub target: AutomationTarget,
    pub triggers: Vec<AutomationTrigger>,
}

pub struct AutomationReplace {
    pub name: String,
    pub description: Option<String>,
    pub enabled: bool,
    pub target: AutomationTarget,
    pub triggers: Vec<AutomationTrigger>,
}

pub struct AutomationPatch {
    pub name: Option<String>,
    pub description: Option<Option<String>>,
    pub enabled: Option<bool>,
    pub target: Option<AutomationTarget>,
    pub triggers: Option<Vec<AutomationTrigger>>,
}
```

- [ ] Use `#[serde(rename = "ref")]` for the Rust field `ref_`.
- [ ] Keep `revision` out of the persisted TOML model; compute it from raw file bytes.
- [ ] Reject empty names, invalid GitHub repository slugs, invalid refs, invalid workflow selectors, duplicate trigger IDs, and more than one trigger with `type = "api"`.
- [ ] Add unit tests for valid TOML, defaults, invalid automation IDs, invalid trigger IDs, duplicate trigger IDs, two `api` triggers, invalid repository slug, and invalid schedule expression.
- [ ] Run `cargo nextest run -p fabro-automation`.
- [ ] Commit:

```bash
git add lib/crates/fabro-automation
git commit -m "feat: add automation domain model"
```

## Task 2: Implement File-Backed Automation Store

**Files:**

- Create: `lib/crates/fabro-automation/src/store.rs`
- Modify: `lib/crates/fabro-automation/src/lib.rs`

- [ ] Implement `AutomationStore` as an in-memory map guarded by `tokio::sync::RwLock`.
- [ ] Load files from a configured directory with this behavior:
  - Missing directory means an empty store.
  - Non-`.toml` files are ignored.
  - Invalid filenames fail load.
  - Invalid TOML or invalid automation data fails load.
- [ ] Compute `AutomationRevision` as lowercase hex SHA-256 of the exact TOML bytes read from disk.
- [ ] Expose these async methods:

```rust
pub async fn load(dir: impl Into<PathBuf>) -> Result<Self, AutomationStoreError>;
pub async fn list(&self) -> Vec<Automation>;
pub async fn get(&self, id: &AutomationId) -> Option<Automation>;
pub async fn create(&self, draft: AutomationDraft) -> Result<Automation, AutomationStoreError>;
pub async fn replace(
    &self,
    id: &AutomationId,
    expected: &AutomationRevision,
    draft: AutomationReplace,
) -> Result<Automation, AutomationStoreError>;
pub async fn patch(
    &self,
    id: &AutomationId,
    expected: &AutomationRevision,
    patch: AutomationPatch,
) -> Result<Automation, AutomationStoreError>;
pub async fn delete(
    &self,
    id: &AutomationId,
    expected: &AutomationRevision,
) -> Result<(), AutomationStoreError>;
```

- [ ] Make create/update writes atomic by serializing to canonical TOML, writing a temp file in the automation directory, flushing it, and renaming it over the final path.
- [ ] Create the automation directory on first write.
- [ ] Map store errors into precise variants: not found, already exists, missing revision, revision mismatch, validation, parse, and I/O.
- [ ] Add tests using `tempfile` for empty load, create writes file, replace changes revision, patch keeps unchanged fields, stale revision fails, delete removes file, and startup fails on malformed TOML.
- [ ] Run `cargo nextest run -p fabro-automation`.
- [ ] Commit:

```bash
git add lib/crates/fabro-automation
git commit -m "feat: persist automations as TOML files"
```

## Task 3: Carry Automation Metadata Through Runs

**Files:**

- Modify: `lib/crates/fabro-types/src/run_summary.rs`
- Modify: `lib/crates/fabro-types/src/run.rs`
- Modify: `lib/crates/fabro-types/src/run_event/run.rs`
- Modify: `lib/crates/fabro-workflow/src/operations/create.rs`
- Modify: `lib/crates/fabro-workflow/src/event/convert.rs`
- Modify: `lib/crates/fabro-store/src/run_state.rs`
- Modify tests that construct `RunSpec` or `RunCreatedProps`

- [ ] Extend `AutomationRef`:

```rust
pub struct AutomationRef {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trigger_id: Option<String>,
}
```

- [ ] Add `automation: Option<AutomationRef>` to `RunSpec` with `#[serde(default, skip_serializing_if = "Option::is_none")]`.
- [ ] Add `automation: Option<AutomationRef>` to `RunCreatedProps` with the same serde behavior.
- [ ] Add `automation: Option<AutomationRef>` to `fabro_workflow::operations::CreateRunInput`.
- [ ] Thread the field through `PersistCreateOptions`, the `RunSpec` built in `persist_validated`, and the `Event::RunCreated` emitted in `persist_created_run`.
- [ ] In `fabro-store/src/run_state.rs`, set `Run.automation` from `state.spec.automation.clone()` instead of always using `None`.
- [ ] Preserve backward compatibility: old run specs and old `run.created` events without `automation` deserialize as `None`.
- [ ] Update all test fixture constructors by setting `automation: None` unless the test specifically checks automation linkage.
- [ ] Add a focused projection test proving `RunCreatedProps.automation` appears in cached `Run.automation`.
- [ ] Run:

```bash
cargo nextest run -p fabro-types
cargo nextest run -p fabro-workflow operations::create
cargo nextest run -p fabro-store run_state
```

- [ ] Commit:

```bash
git add lib/crates/fabro-types lib/crates/fabro-workflow lib/crates/fabro-store
git commit -m "feat: associate runs with automations"
```

## Task 4: Add OpenAPI Contract And Type Reuse

**Files:**

- Modify: `docs/public/api-reference/fabro-api.yaml`
- Modify: `lib/crates/fabro-api/Cargo.toml`
- Modify: `lib/crates/fabro-api/build.rs`
- Create: `lib/crates/fabro-api/tests/automation_round_trip.rs`

- [ ] Add an `Automations` tag.
- [ ] Add schemas for `Automation`, `AutomationTarget`, `AutomationTrigger`, `AutomationApiTrigger`, `AutomationScheduleTrigger`, `CreateAutomationRequest`, `ReplaceAutomationRequest`, `PatchAutomationRequest`, and `AutomationListResponse`.
- [ ] Use OpenAPI discriminator `propertyName: type` for trigger variants.
- [ ] Implement request-body parsing so unknown trigger discriminator values are reported as domain validation errors (`422`), not JSON parse errors (`400`). Use raw DTOs or custom deserialization before converting into `fabro-automation` domain types.
- [ ] Reuse existing `Run` and paginated run envelope schemas for `POST /automations/{id}/runs` and `GET /automations/{id}/runs`.
- [ ] Add response codes:
  - `200` for reads and replace/patch.
  - `201` for create automation and create run.
  - `204` for delete.
  - `400` for malformed JSON or invalid path syntax.
  - `404` for missing automation.
  - `409` for duplicate create, stale revision, disabled automation, or disabled/missing `api` trigger.
  - `422` for domain validation errors.
  - `428` for missing `If-Match` on `PUT`, `PATCH`, or `DELETE`.
- [ ] Add `If-Match` header parameters for mutating path operations except `POST /automations`.
- [ ] Add `ETag` response header on `GET /automations/{id}`, `PUT`, and `PATCH`.
- [ ] Before adding generated duplicate Rust types, search for matching domain types. If `fabro-automation` serde shape matches a schema exactly, add a `with_replacement(...)` entry in `lib/crates/fabro-api/build.rs`.
- [ ] Add JSON parity tests for every automation replacement type used by `fabro-api`.
- [ ] Run `cargo build -p fabro-api`.
- [ ] Commit:

```bash
git add docs/public/api-reference/fabro-api.yaml lib/crates/fabro-api
git commit -m "feat: define automations API contract"
```

## Task 5: Wire Automation Store Into Server State

**Files:**

- Modify: `lib/crates/fabro-server/Cargo.toml`
- Modify: `lib/crates/fabro-server/src/server.rs`
- Modify: `lib/crates/fabro-server/src/test_support.rs`

- [ ] Add `fabro-automation = { path = "../fabro-automation" }` to server dependencies.
- [ ] Add `automation_store: Arc<AutomationStore>` to `AppState`.
- [ ] In `build_app_state`, compute the automation directory as:

```rust
let automation_dir = active_config_path
    .parent()
    .unwrap_or_else(|| std::path::Path::new("."))
    .join("automations");
```

- [ ] Load `AutomationStore::load(automation_dir)` before constructing `AppState`.
- [ ] Fail server startup if an existing automation file is malformed.
- [ ] Add `pub(crate) fn automation_store(&self) -> Arc<AutomationStore>`.
- [ ] In test support, keep the existing temp `active_config_path` behavior so each test gets its own sibling `automations` directory.
- [ ] Add a server unit test for empty automation store creation when no automation directory exists.
- [ ] Run `cargo nextest run -p fabro-server automation_store`.
- [ ] Commit:

```bash
git add lib/crates/fabro-server
git commit -m "feat: load automation store in server state"
```

## Task 6: Add Automation CRUD Routes

**Files:**

- Create: `lib/crates/fabro-server/src/server/handler/automations.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/mod.rs`
- Create: `lib/crates/fabro-server/tests/it/api/automations.rs`
- Modify: `lib/crates/fabro-server/tests/it/api/mod.rs`

- [ ] Read `docs/internal/logging-strategy.md` and `docs/internal/error-handling-strategy.md` before adding request errors or logs.
- [ ] Implement `automations::routes()` and merge it into `handler::real_routes()`.
- [ ] Use `RequiredUser` for CRUD routes.
- [ ] Implement `GET /automations` by listing store entries, sorting by ID ascending, and returning `{ data, meta: { total } }`.
- [ ] Implement `POST /automations` with `CreateAutomationRequest`; duplicate ID returns `409`.
- [ ] Implement `GET /automations/{id}` with `ETag: "<revision>"`.
- [ ] Implement `PUT /automations/{id}` with `ReplaceAutomationRequest` and required `If-Match`.
- [ ] Implement `PATCH /automations/{id}` with `PatchAutomationRequest`, shallow patch semantics, and required `If-Match`.
- [ ] Implement `DELETE /automations/{id}` with required `If-Match`.
- [ ] Add a helper that parses a quoted or unquoted `If-Match` revision and rejects missing headers with `428`.
- [ ] Map `AutomationStoreError` to `ApiError`:
  - not found to `404`
  - already exists to `409`
  - missing revision to `428`
  - revision mismatch to `409`
  - validation to `422`
  - parse/I/O to `500` except malformed request bodies, which stay `400`
- [ ] Add route tests for empty list, create, duplicate create, get with ETag, replace, stale replace, missing `If-Match`, patch clearing description, delete, invalid trigger IDs, duplicate trigger IDs, second trigger with `type = "api"`, and invalid schedule expression.
- [ ] Run `cargo nextest run -p fabro-server automations`.
- [ ] Commit:

```bash
git add lib/crates/fabro-server
git commit -m "feat: add automation CRUD API"
```

## Task 7: Add Automation Run Listing And API-Triggered Runs

**Files:**

- Create: `lib/crates/fabro-server/src/automation_materializer.rs`
- Modify: `lib/crates/fabro-server/src/server.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/runs.rs`
- Modify: `lib/crates/fabro-server/src/server/handler/automations.rs`
- Modify: `lib/crates/fabro-server/src/test_support.rs`
- Create: `lib/crates/fabro-server/tests/it/api/automations.rs`
- Modify: `lib/crates/fabro-server/tests/it/api/mod.rs`

- [ ] Extract the common run creation body from `handler/runs.rs::create_run` into a crate-private helper that accepts:

```rust
struct CreateRunFromManifestRequest {
    manifest: fabro_api::types::RunManifest,
    submitted_manifest_bytes: Vec<u8>,
    explicit_run_id: Option<fabro_types::RunId>,
    explicit_title_supplied: bool,
    actor: fabro_types::Principal,
    headers: axum::http::HeaderMap,
    automation: Option<fabro_types::AutomationRef>,
}
```

- [ ] Keep `POST /runs` behavior unchanged by calling the helper with `automation: None`.
- [ ] Define a crate-private materializer trait:

```rust
pub(crate) struct AutomationRunMaterializeInput {
    pub automation_id: fabro_automation::AutomationId,
    pub target: fabro_automation::AutomationTarget,
    pub run_id: fabro_types::RunId,
    pub user_settings_path: std::path::PathBuf,
    pub temp_root: std::path::PathBuf,
}

pub(crate) struct AutomationRunMaterialized {
    pub manifest: fabro_api::types::RunManifest,
    pub submitted_manifest_bytes: Vec<u8>,
}

#[derive(thiserror::Error, Debug)]
pub(crate) enum AutomationRunMaterializeError {
    #[error("invalid automation target: {0}")]
    InvalidTarget(String),
    #[error("failed to clone automation repository: {0}")]
    CloneFailed(String),
    #[error("failed to resolve automation workflow: {0}")]
    WorkflowNotFound(String),
    #[error("failed to build run manifest: {0}")]
    Manifest(String),
}

#[async_trait::async_trait]
pub(crate) trait AutomationRunMaterializer: Send + Sync {
    async fn materialize(
        &self,
        input: AutomationRunMaterializeInput,
    ) -> Result<AutomationRunMaterialized, AutomationRunMaterializeError>;
}
```

- [ ] Use a production implementation that:
  - validates target repository as GitHub `owner/repo`
  - is constructed with the server GitHub credentials, GitHub API base URL, HTTP client, and cleanup policy needed for clone materialization
  - creates a per-run temp directory under `AutomationRunMaterializeInput.temp_root`
  - clones `https://github.com/{owner}/{repo}.git`
  - uses existing GitHub clone credential helpers when configured
  - checks out the configured `ref`
  - resolves the workflow selector using `fabro_config::project::WorkflowLocation::resolve`
  - builds a `RunManifest` with `fabro_manifest::build_run_manifest`
  - passes `user_settings_path: Some(state.active_config_path().to_path_buf())`
- [ ] Use `tokio::process::Command` with argv values for git commands. Do not construct shell command strings. Set `GIT_TERMINAL_PROMPT=0` and explicit timeouts so private-repo credential failures cannot hang request handling.
- [ ] Store only sanitized repository URLs in run metadata. Do not persist credentialed clone URLs.
- [ ] Add test support injection for a fake `AutomationRunMaterializer` behind tests or the existing `test-support` feature.
- [ ] Implement `GET /automations/{id}/runs`:
  - require the automation to exist
  - list cached runs from the store
  - filter by `run.automation.as_ref().is_some_and(|a| a.id == id)`
  - sort newest first
  - paginate with `page[limit]` and `page[offset]`
  - return the existing `{ data, meta }` list shape
- [ ] Implement `POST /automations/{id}/runs`:
  - use `RequiredRunToolActor`
  - require automation `enabled == true`
  - find the enabled trigger with `type = "api"`
  - return `409` with API error code `automation_api_trigger_disabled` if not startable
  - materialize the run manifest
  - call the shared create-run helper with `AutomationRef { id, name, trigger_id: Some(api_trigger_id) }`
  - return `201` and the created `Run`
- [ ] Add route tests using the fake materializer for disabled automation, disabled API trigger, successful run creation, persisted `Run.automation`, and associated run listing.
- [ ] Add lower-level materializer tests for target URL construction, credential redaction, ref checkout command planning, and workflow path resolution using temp directories. Do not add a live GitHub test.
- [ ] Run `cargo nextest run -p fabro-server automations`.
- [ ] Commit:

```bash
git add lib/crates/fabro-server
git commit -m "feat: start runs from automations"
```

## Task 8: Generate Clients And Final Verification

**Files:**

- Modify generated files under `lib/packages/fabro-api-client`
- Modify generated Rust files under `lib/crates/fabro-api/src` if `cargo build -p fabro-api` updates them

- [ ] Regenerate Rust API code:

```bash
cargo build -p fabro-api
```

- [ ] Regenerate the TypeScript API client:

```bash
cd lib/packages/fabro-api-client && bun run generate
```

- [ ] Confirm no web UI imports or CLI command modules changed:

```bash
git diff -- apps/fabro-web lib/crates/fabro-cli
```

Expected: no application or CLI command changes caused by this plan.

- [ ] Run focused tests:

```bash
cargo nextest run -p fabro-automation
cargo nextest run -p fabro-api
cargo nextest run -p fabro-server automations
cargo nextest run -p fabro-server openapi_conformance
```

- [ ] Run broader checks:

```bash
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

- [ ] If clippy or tests expose unrelated existing failures, record the exact failing command and failure summary in the implementation handoff.
- [ ] Commit generated and verification fixes:

```bash
git add docs/public/api-reference/fabro-api.yaml lib/crates lib/packages/fabro-api-client
git commit -m "chore: regenerate automation API clients"
```

## Acceptance Criteria

- A server with no `automations/` directory starts and returns an empty automation list.
- Creating an automation writes `dirname(active_config_path)/automations/<id>.toml`.
- Updating or deleting an automation requires `If-Match`.
- Stale revisions are rejected.
- Invalid automation and trigger shapes are rejected with `422`.
- Disabling the `api` trigger makes the automation not startable through `POST /automations/{id}/runs`.
- A successful API-triggered automation run returns a normal `Run` response with `automation.id`, `automation.name`, and `automation.trigger_id`.
- `GET /automations/{id}/runs` returns runs linked to that automation.
- No cron scheduler, web UI exposure, or CLI exposure is added.


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
- **fix_lints**: succeeded
  - Model: claude-opus-4-7, 15.2k tokens in / 1.3k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-store/src/run_state.rs
- **preflight_lint**: succeeded
  - Script: `cargo +nightly-2026-04-14 clippy -q --workspace --all-targets -- -D warnings 2>&1`
  - Output: (empty)
- **implement**: succeeded
  - Model: gpt-5.5, 8.8m tokens in / 26.1k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/automation_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/automation_materializer.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/automations.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/tests/it/api/automations.rs
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 165.4k tokens in / 58.2k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-api/tests/automation_round_trip.rs, /home/daytona/workspace/fabro/lib/crates/fabro-automation/Cargo.toml, /home/daytona/workspace/fabro/lib/crates/fabro-automation/src/error.rs, /home/daytona/workspace/fabro/lib/crates/fabro-automation/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-automation/src/model.rs, /home/daytona/workspace/fabro/lib/crates/fabro-automation/src/store.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/automation_materializer.rs, /home/daytona/workspace/fabro/lib/crates/fabro-server/src/server/handler/automations.rs
- **simplify_gpt**: succeeded
  - Model: gpt-5.5, 1.1m tokens in / 5.0k out
- **verify**: failed
  - Script: `git fetch origin main 2>&1 && git merge --no-edit --no-stat origin/main 2>&1 && cargo +nightly-2026-04-14 fmt --all 2>&1 && cargo dev docs refresh 2>&1 && cargo +nightly-2026-04-14 fmt --check --all 2>&1 && { command -v rg >/dev/null 2>&1 || { echo 'rg is required for verify'; exit 127; }; } && ! rg -n 'AuthMode::Disabled|RunAuthMethod|RunSubjectProvenance|\bActorRef\b|\bActorKind\b|AuthenticatedSubject|AuthenticatedService|AuthorizeRunScoped|AuthorizeRunBlob|AuthorizeStageArtifact|AuthorizeCommandLog|auth_method\s*==\s*"disabled"' lib/crates apps lib/packages docs/public/api-reference/fabro-api.yaml 2>&1 && cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings 2>&1 && cargo nextest run --workspace --status-level slow --profile ci 2>&1 && cargo dev docs check 2>&1 && bun install --frozen-lockfile 2>&1 && (cd apps/fabro-web && bun run typecheck) 2>&1 && (cd apps/fabro-web && bun run test) 2>&1 && (cd lib/packages/fabro-api-client && bun run typecheck) 2>&1 && cargo dev build -- -p fabro-cli --release 2>&1`
  - Output:
    ```
    (319 lines omitted)
        Stopped on the first failure. Run `cargo insta test` to run all snapshots.
        test cmd::model::list_query_aliases ... FAILED
    
        failures:
    
        failures:
            cmd::model::list_query_aliases
    
        test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 507 filtered out; finished in 0.10s
    
      stderr ───
        MODEL                PROVIDER  ALIASES      CONTEXT          COST       SPEED 
         gpt-5.3-codex        openai    codex           272k  $1.8 / $14.0   100 tok/s 
         gpt-5.3-codex-spark  openai    codex-spark     131k         - / -  1000 tok/s
    
        thread 'cmd::model::list_query_aliases' (111054) panicked at /root/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/insta-1.46.3/src/runtime.rs:719:13:
        snapshot assertion for 'list_query_aliases' failed in line 118
        note: run with `RUST_BACKTRACE=1` environment variable to display a backtrace
    
      Cancelling due to test failure: 7 tests still running
    ────────────
         Summary [   5.803s] 953/6421 tests run: 952 passed, 1 failed, 181 skipped
            FAIL [   0.112s] ( 946/6421) fabro-cli::it cmd::model::list_query_aliases
    warning: 5468/6421 tests were not run due to test failure (run with --no-fail-fast to run all tests, or run with --max-fail)
    error: test run failed
    ```

## Context
- failure_class: transient_infra
- failure_signature: verify|transient_infra|script failed with exit code: <n> ## output ly.<n> (/home/daytona/repos/fabro-sh/fabro/lib/crates/fabro-tool) compiling fabro-workflow v0.<n>.<n>-nightly.<n> (/home/daytona/repos/fabro-sh/fabro/lib/crates/fabro-workflow) compiling fabro-sla


The verify step failed. Read the build output from context and fix all format, clippy, Rust test, docs, TypeScript typecheck/test, and build failures.