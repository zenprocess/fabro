Goal: # Remove Automation Master Enabled Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove the top-level automation `enabled` field so trigger-level `enabled` is the only activation control.

**Architecture:** Automations keep their existing file-backed TOML store and REST API, but the top-level master gate disappears from the Rust domain model, persisted TOML, OpenAPI schemas, generated clients, and web UI. API/manual run creation checks only for an enabled `api` trigger. No migration or legacy parser path is added because automations are brand new; TOML that still contains top-level `enabled` is obsolete input.

**Tech Stack:** Rust, serde/TOML, Axum, OpenAPI/progenitor, TypeScript Axios client generation, React 19, SWR, Tailwind CSS.

---

## File Structure

- Modify `lib/crates/fabro-automation/src/model.rs` for the core type and TOML shape.
- Modify `lib/crates/fabro-automation/src/store.rs` for unit fixtures that create automation drafts/replacements.
- Modify `lib/crates/fabro-server/src/server/handler/automations.rs` for API-trigger conflict wording.
- Modify `lib/crates/fabro-server/tests/it/api/automations.rs` for HTTP fixtures and behavior tests.
- Modify `docs/public/api-reference/fabro-api.yaml` and regenerate `lib/packages/fabro-api-client/src/**`.
- Modify `lib/crates/fabro-api/tests/automation_round_trip.rs` for Rust/OpenAPI type parity.
- Modify `apps/fabro-web/app/components/automation-form.tsx`, `apps/fabro-web/app/routes/automations-new.tsx`, `apps/fabro-web/app/routes/automations-edit.tsx`, `apps/fabro-web/app/routes/automation-detail.tsx`, and `apps/fabro-web/app/routes/automations.tsx` for UI state and trigger-derived run availability.

## Task 1: Remove The Domain Master Gate

**Files:**
- Modify: `lib/crates/fabro-automation/src/model.rs`
- Modify: `lib/crates/fabro-automation/src/store.rs`

- [ ] Remove `pub enabled: bool` from `Automation`, `AutomationDraft`, `AutomationReplace`, and `PersistedAutomation`.
- [ ] Remove `enabled` from every conversion between `AutomationDraft`, `AutomationReplace`, `PersistedAutomation`, and `Automation`.
- [ ] Update `Automation::enabled_api_trigger()` to return an enabled API trigger without checking a top-level automation flag:

```rust
/// Returns the enabled API trigger if the automation has one.
/// Returns `None` when the automation has no enabled API trigger.
#[must_use]
pub fn enabled_api_trigger(&self) -> Option<&ApiTrigger> {
    self.triggers.iter().find_map(|trigger| match trigger {
        AutomationTrigger::Api(trigger) if trigger.enabled => Some(trigger),
        _ => None,
    })
}
```

- [ ] Remove the now-unused `default_true()` helper if no other code in the file still uses it.
- [ ] Update `persisted_toml_applies_defaults_and_canonicalizes_without_id_or_revision` so the fixture has no top-level `enabled = true`, does not assert `automation.enabled`, and asserts the canonical TOML has no top-level `enabled` line:

```rust
assert!(!top_level_lines(&toml).any(|line| line.starts_with("enabled = ")));
```

- [ ] Add a focused no-compatibility test in `lib/crates/fabro-automation/src/model.rs`:

```rust
#[test]
fn persisted_toml_rejects_legacy_top_level_enabled() {
    let bytes = br#"
name = "Legacy"
enabled = false

[target]
repository = "fabro-sh/fabro"
ref = "main"
workflow = "release"

[[triggers]]
type = "api"
id = "manual"
enabled = true
"#;

    let result = Automation::from_toml_bytes(AutomationId::new("legacy").unwrap(), bytes);

    assert!(result.is_err());
}
```

- [ ] Update `lib/crates/fabro-automation/src/store.rs` test helpers so `draft()` and `replacement()` no longer set top-level `enabled`.
- [ ] Run:

```bash
cargo nextest run -p fabro-automation
```

Expected: all `fabro-automation` tests pass.

## Task 2: Update Server Behavior And Tests

**Files:**
- Modify: `lib/crates/fabro-server/src/server/handler/automations.rs`
- Modify: `lib/crates/fabro-server/tests/it/api/automations.rs`

- [ ] Change the `create_automation_run` conflict detail from:

```rust
"automation is disabled or has no enabled API trigger"
```

to:

```rust
"automation has no enabled API trigger"
```

Keep the existing code `"automation_api_trigger_disabled"` for compatibility with current clients and tests.

- [ ] Remove top-level `"enabled": true` from `automation_body()`.
- [ ] Remove top-level `"enabled": false` from `replacement_body()`.
- [ ] Delete `disabled_automation_run_endpoint_returns_conflict_code`; the master gate no longer exists.
- [ ] Keep `disabled_api_trigger_run_endpoint_returns_conflict_code` and `missing_api_trigger_run_endpoint_returns_conflict_code` as the authoritative inactive-run tests.
- [ ] Update any test that mutates `body["enabled"]` or expects top-level enabled in automation JSON/TOML.
- [ ] Run:

```bash
cargo nextest run -p fabro-server automations
```

Expected: automation integration tests pass.

## Task 3: Update OpenAPI And Generated API Types

**Files:**
- Modify: `docs/public/api-reference/fabro-api.yaml`
- Modify: `lib/crates/fabro-api/tests/automation_round_trip.rs`
- Regenerate: `lib/packages/fabro-api-client/src/**`

- [ ] In the `Automation` schema, remove top-level `enabled` from `required` and `properties`.
- [ ] In `CreateAutomationRequest`, remove top-level `enabled` from `properties`.
- [ ] In `ReplaceAutomationRequest`, remove top-level `enabled` from `required` and `properties`.
- [ ] Keep `enabled` on `AutomationApiTrigger` and `AutomationScheduleTrigger`.
- [ ] Update the `POST /api/v1/automations/{id}/runs` `409` description from:

```yaml
description: Automation is disabled or has no enabled API trigger
```

to:

```yaml
description: Automation has no enabled API trigger
```

- [ ] Update `lib/crates/fabro-api/tests/automation_round_trip.rs` so the `Automation`, `CreateAutomationRequest`, and `ReplaceAutomationRequest` JSON fixtures no longer include top-level `"enabled"`.
- [ ] Run:

```bash
cargo build -p fabro-api
```

Expected: progenitor type generation succeeds.

- [ ] Run:

```bash
cargo nextest run -p fabro-api automation_round_trip
```

Expected: automation type identity and JSON parity tests pass.

- [ ] Regenerate the TypeScript client:

```bash
cd lib/packages/fabro-api-client && bun run generate
```

Expected: generated model files remove top-level `enabled` from `Automation`, `CreateAutomationRequest`, and `ReplaceAutomationRequest`.

## Task 4: Remove The Web UI Master Toggle

**Files:**
- Modify: `apps/fabro-web/app/components/automation-form.tsx`
- Modify: `apps/fabro-web/app/routes/automations-new.tsx`
- Modify: `apps/fabro-web/app/routes/automations-edit.tsx`
- Modify: `apps/fabro-web/app/routes/automation-detail.tsx`
- Modify: `apps/fabro-web/app/routes/automations.tsx`

- [ ] Remove `enabled` from `AutomationFormValues` and `EMPTY_AUTOMATION_FORM`.
- [ ] Remove `enabled: automation.enabled` from `automationToFormValues`.
- [ ] Delete the `Row title="Enabled"` block from `AutomationFormFields`.
- [ ] Remove `enabled: values.enabled` from the create payload in `automations-new.tsx`.
- [ ] Remove `enabled: values.enabled` from the replace payload in `automations-edit.tsx`.
- [ ] In `isFormValid`, remove the requirement that at least one trigger is enabled. The final return should only require non-empty ID, name, repository, ref, and workflow:

```ts
return (
  values.id.trim() !== "" &&
  values.name.trim() !== "" &&
  values.repository.trim() !== "" &&
  values.ref.trim() !== "" &&
  values.workflow.trim() !== ""
);
```

- [ ] In `automation-detail.tsx`, change run availability to:

```ts
const canRun = apiTrigger?.enabled === true;
```

- [ ] In `automation-detail.tsx`, remove `StatusChip`, remove its use, and simplify the Run button `title` so only a missing/disabled API trigger explains the disabled state:

```ts
title={!apiTrigger?.enabled ? "Enable the API trigger to run it" : undefined}
```

- [ ] In `automations.tsx`, extend `AutomationRow` with `apiEnabled: boolean`, set it from the enabled API trigger in `mapAutomations`, and pass `disabled={deleting || !automation.apiEnabled || (runningId !== null && runningId !== automation.id)}` to the run button path.
- [ ] In `AutomationCard`, make the run button title reflect trigger-disabled state:

```tsx
title={
  running
    ? "Starting run..."
    : automation.apiEnabled
      ? "Run automation"
      : "Enable the API trigger to run it"
}
```

Use exactly this title text for the disabled/run states; do not change visible button copy.

- [ ] Run:

```bash
cd apps/fabro-web && bun run typecheck
```

Expected: TypeScript passes with no `automation.enabled` references.

## Task 5: Final Verification

**Files:**
- No additional source edits expected.

- [ ] Run the focused backend checks:

```bash
cargo nextest run -p fabro-automation
cargo nextest run -p fabro-api automation_round_trip
cargo nextest run -p fabro-server automations
```

Expected: all focused Rust checks pass.

- [ ] Run the focused frontend checks:

```bash
cd lib/packages/fabro-api-client && bun run typecheck
cd apps/fabro-web && bun run typecheck
```

Expected: generated client and web app typecheck.

- [ ] Search for leftover master-gate references:

```bash
rg -n "automation\\.enabled|enabled_api_trigger\\(\\).*automation|Automation is disabled|automation is disabled|\\\"enabled\\\": true" \
  lib/crates/fabro-automation \
  lib/crates/fabro-server/tests/it/api/automations.rs \
  lib/crates/fabro-server/src/server/handler/automations.rs \
  lib/crates/fabro-api/tests/automation_round_trip.rs \
  apps/fabro-web/app/components/automation-form.tsx \
  apps/fabro-web/app/routes/automations-new.tsx \
  apps/fabro-web/app/routes/automations-edit.tsx \
  apps/fabro-web/app/routes/automation-detail.tsx \
  apps/fabro-web/app/routes/automations.tsx
```

Expected: no hits for the removed top-level automation enabled gate. Hits for trigger-level `enabled` are acceptable when they are clearly on `AutomationTrigger`, `ApiTrigger`, or `ScheduleTrigger`.

- [ ] Optional formatting check:

```bash
cargo +nightly-2026-04-14 fmt --check --all
```

Expected: Rust formatting passes. If it fails on touched Rust files, run `cargo +nightly-2026-04-14 fmt --all` and re-run the focused Rust checks.

## Explicit Assumptions

- No migration or compatibility parser is added for top-level automation `enabled`.
- Existing automation TOML files that still contain top-level `enabled` are obsolete and may fail startup until manually edited.
- Trigger-level `enabled` remains public API and persisted TOML.
- API/manual run creation remains controlled by the enabled `api` trigger only.
- Schedule execution, cron semantics, and schedule-trigger preservation are outside this cleanup.


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