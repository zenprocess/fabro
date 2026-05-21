Let me give a final summary:

## Summary

Implemented the full **Wall and active time metrics** plan with red/green testing throughout.

### Changes by crate

**`fabro-types`** (new types + field renames)
- Added `timing` module with `StageTiming` and `RunTiming` value objects (fields: `wall_time_ms`, `inference_time_ms`, `tool_time_ms`, `active_time_ms` — all `u64` ms; `active_time_ms` derived from inference + tool).
- Replaced `Outcome.duration_ms` with `Outcome.timing: Option<StageTiming>`.
- Replaced `NodeResult.duration: Duration` with `wall_time` + `inference_time` + `tool_time` Durations.
- Replaced `StageProjection.duration_ms` with `timing: Option<StageTiming>`; renamed `runtime_secs()` to `live_wall_time_ms()`.
- Replaced `Conclusion.duration_ms` with `timing: RunTiming`.
- Replaced `StageSummary.duration_ms` with `timing: StageTiming`.
- Removed `RunTimestamps.{duration_ms, elapsed_secs}`; added `Run.timing: Option<RunTiming>` as a sibling of `timestamps`.
- Replaced `duration_ms` with `timing` on `RunCompletedProps`, `RunFailedProps`, `StageCompletedProps`, `StageFailedProps`.

**`fabro-core`** (NodeResult constructor + executor helper)
- 6-arg `NodeResult::new(outcome, wall_time, inference_time, tool_time, attempts, max_attempts)`.
- Executor now harvests inference/tool from `outcome.timing` when handlers populate it.

**`fabro-workflow`** (rename + thread timing)
- Renamed event/duration helpers to `extract_stage_timings_by_stage_id` / `total_stage_timing_by_node` / `latest_stage_timing_by_node`.
- `ProjectionBillingStage.duration_ms` → `timing: StageTiming`; `ProjectionBillingRollup.runtime_ms` → `timing: StageTiming` (sums work across visits).
- `Event::WorkflowRunCompleted/Failed` + `Event::StageCompleted/Failed` now carry `timing` instead of `duration_ms`.
- `Executed.duration_ms` → `wall_time_ms`.
- `build_terminal_event` and `workflow_run_failed_from_error` take `RunTiming`.
- PR body and lifecycle emitters thread timing through.

**`fabro-store`**
- Projection populates `stage.timing`, `Run.timing`, and `Conclusion.timing` from new event props.
- Removed `elapsed_secs(duration_ms)` helper.

**`fabro-server`**
- Billing handler exposes `RunBillingStage.timing` and `RunBillingTotals.timing` (was `runtime_secs`).
- `AggregateBillingTotals.timing` replaces `runtime_secs`.
- `RunStage.wall_time_ms` (was `duration_secs`).
- `BillingAccumulator.total_runtime_secs` → `total_timing: StageTiming`.

**OpenAPI** (`docs/public/api-reference/fabro-api.yaml`)
- New `StageTiming` and `RunTiming` schemas.
- `RunBillingStage.runtime_secs`, `RunBillingTotals.runtime_secs`, `AggregateBillingTotals.runtime_secs` → `timing`.
- `RunTimestamps.{duration_ms, elapsed_secs}` removed; `Run.timing` added.
- `StageProjection.duration_ms`, `StageSummary.duration_ms`, `Conclusion.duration_ms` → `timing`.
- `RunStage.duration_secs`, `CheckRun.duration_secs` → `wall_time_ms`.
- `RunTimings.elapsed_secs` → `wall_time_ms`.

**Generated clients** (Rust + TypeScript regenerated)
- `fabro-api` build.rs now reuses `fabro_types::{StageTiming, RunTiming}` via `with_replacement`.
- TS client regenerated under `lib/packages/fabro-api-client/src/models/`.

**`apps/fabro-web`**
- `runs.ts` reads `item.timing.wall_time_ms` (was `item.timestamps.elapsed_secs`).
- `stage-sidebar.ts` reads `stage.wall_time_ms` (was `stage.duration_secs`).
- `run-billing.tsx` operates entirely on `timing.wall_time_ms` (renamed `runtimeSecs` → `wallTimeMs`, `liveRuntimeSecs` → `liveWallTimeMs`, etc).
- All tests updated and pass.

**Subsystem `duration_ms` preserved** for sandbox setup/snapshot/ready/stop, devcontainer lifecycle, metadata snapshot, hook/interview/parallel branch/parallel completed/ACP completed-cancelled-timeout/command completed events (per plan).

### Test results

- **Workspace tests**: 5932/5932 pass, 180 skipped.
- **Web tests**: 408/408 pass, typecheck clean.
- **Focused tests (per plan)**: 2065/2065 pass across `fabro-types`, `fabro-store`, `fabro-workflow`, `fabro-server`.
- `cargo build --workspace` clean.
- `cargo +nightly-2026-04-14 fmt --check --all` clean.
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` clean.
- `git diff --check` clean.