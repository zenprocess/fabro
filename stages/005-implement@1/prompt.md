Goal: # Issue #399: Add automation run endpoints

- URL: https://github.com/fabro-sh/fabro/issues/399
- State: OPEN
- Author: Bryan Helmkamp (@brynary)
- Created: 2026-05-25T15:06:27Z
- Updated: 2026-05-25T15:06:27Z
- Labels: None
- Assignees: None
- Milestone: None
- Comments: 0

---

## Goal

Expose API endpoints for listing runs associated with an automation and starting a run through an enabled API trigger.

## Scope

Implement these endpoints:

```http
GET  /automations/{id}/runs
POST /automations/{id}/runs
```

`GET /automations/{id}/runs` behavior:

- Require the automation definition to exist; return 404 when it does not.
- List cached runs from the existing run store.
- Filter by `run.automation.as_ref().is_some_and(|a| a.id == id)`.
- Sort newest first.
- Support `page[limit]` and `page[offset]` using existing pagination behavior.
- Return the existing paginated run list envelope:

```json
{
  "data": [],
  "meta": { "has_more": false, "total": 0 }
}
```

`POST /automations/{id}/runs` behavior:

- Use `RequiredRunToolActor`.
- Require the automation to exist and be enabled.
- Find an enabled trigger where `type = "api"`.
- Return 409 with API error code `automation_api_trigger_disabled` when the automation is disabled or no enabled API trigger is available.
- Materialize the run manifest using the configured `AutomationRunMaterializer`.
- Call the shared create-run helper with:

```rust
AutomationRef {
    id: automation.id.to_string(),
    name: Some(automation.name.clone()),
    trigger_id: Some(api_trigger.id.to_string()),
}
```

- Return 201 and the normal `Run` response shape with automation metadata populated.

Final integration expectations:

- Automation-created runs are visible through normal run APIs.
- Automation-created runs are visible through `GET /automations/{id}/runs`.
- Run history is derived from persisted/cached runs; no runtime automation state store is introduced.
- Schedule trigger expressions are stored and validated by earlier phases but are not scheduled by this endpoint work.

## Files

Modify:

- `lib/crates/fabro-server/src/server/handler/automations.rs`
- `lib/crates/fabro-server/src/server/handler/runs.rs`, only if additional helper exposure is needed from the previous phase
- `lib/crates/fabro-server/tests/it/api/automations.rs`
- `lib/crates/fabro-server/tests/it/api/mod.rs`

## Acceptance Criteria

- Disabled automations cannot start runs through the automation run endpoint.
- Automations without an enabled API trigger cannot start runs through the automation run endpoint.
- A successful API-triggered automation run returns a normal `Run` response with `automation.id`, `automation.name`, and `automation.trigger_id`.
- The automation run listing endpoint returns only runs linked to that automation.
- Automation run listings are newest-first and paginate correctly.
- No scheduler, web UI route/component, or CLI command is added.

## Verification

Add integration tests using the fake materializer for:

- Disabled automation returns 409.
- Disabled API trigger returns 409.
- Missing API trigger returns 409.
- Successful run creation returns 201.
- Created run persists `Run.automation`.
- Associated run listing includes the run.
- Run listing excludes runs from other automations.
- Run listing pagination and newest-first sorting.

Run:

```bash
cargo nextest run -p fabro-automation
cargo nextest run -p fabro-api
cargo nextest run -p fabro-server automations
cargo nextest run -p fabro-server openapi_conformance
cargo +nightly-2026-04-14 fmt --check --all
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
git diff -- apps/fabro-web lib/crates/fabro-cli
```

Expected: focused tests and checks pass; web UI and CLI command modules remain unchanged.


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