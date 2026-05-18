All 5827 tests pass, clippy is clean, formatting is clean, web build is clean.

The plan described in the goal — adding `Blocked` as a first-class run lifecycle status, the canonical operator status vocabulary (`submitted, queued, starting, running, blocked, paused, removing, succeeded, failed, dead`), `BlockedReason::HumanInputRequired`, the `run.queued`/`run.blocked`/`run.unblocked` lifecycle events, board column changes from `working|review|merge`/`waiting` to `initializing|running|blocked|succeeded|failed`, and all the projection/server/web/CLI plumbing — is **already fully implemented in the tree at HEAD**.

The work landed in an earlier merge (PR #165 "blocked status canonicalization cleanup" and follow-ups), and the prior `preflight_compile` and `preflight_lint` stages of this run already confirmed that the workspace compiles and lints cleanly.

Verified state of each plan section:

1. **Shared Types & OpenAPI** (`lib/crates/fabro-types/src/status.rs`, `docs/public/api-reference/fabro-api.yaml`)
   - `RunStatus` has all 10 canonical variants with `Blocked { blocked_reason }`, `Paused { prior_block }`, `Succeeded { reason }`, `Failed { reason }`, `Dead`.
   - OpenAPI `RunStatus` schema is a discriminated `oneOf` over the canonical vocabulary; `BlockedReason` schema exists; `BoardColumn` no longer contains `working|review|merge|waiting`.
   - `pending_interviews` is exposed on `RunProjection`.

2. **Lifecycle Events & Transition Rules** (`fabro-types/src/run_event/mod.rs`, `fabro-workflow/src/event/*`, `handler/human.rs`, `operations/start.rs`, `status.rs`)
   - `RunQueued(RunStatusEffectProps)`, `RunBlocked(RunBlockedProps { blocked_reason })`, `RunUnblocked(RunStatusEffectProps)` event variants present.
   - `BlockedStateTracker` exists for `0↔1` interview-count transitions.
   - `can_transition_to` covers `submitted→queued`, `queued→starting`, `running→blocked`, `blocked→running`, `blocked→paused`, `paused→blocked` (via the explicit `paused→running→blocked` event order), and non-terminal `→failed` from any active state.

3. **Durable Projection & Truthful Run APIs** (`fabro-store/src/run_state.rs`, `fabro-server/src/server.rs`, `demo/mod.rs`)
   - `RunStatusRecord` carries `status_reason` and `blocked_reason` (the previous `reason` rename is in place).
   - Paused-over-blocked preserves `blocked_reason`; `run.unblocked` while paused clears `blocked_reason` and keeps `Paused`.
   - `run.queued` is appended in the server `start`/`resume` path; `update_live_run_from_event` handles `run.queued`/`run.blocked`/`run.unblocked`; `pause_run` accepts `Blocked` and emits an immediate `run.paused`.
   - Tests in `run_state.rs` cover `run.queued`, `run.blocked`, `run.unblocked`, paused-over-blocked, unpause-to-still-blocked, interview-resolution-while-paused, and submitted-synthesis-on-missing-status (lines 1739, 1768, 1789, 1830, 1852, 2567).

4. **Web Board & UI** (`apps/fabro-web/app/data/runs.ts`, `app/lib/board-events.ts`, `routes/runs.tsx`)
   - `BoardColumn` is `queued | initializing | running | blocked | succeeded | failed | archived`; `columnForStatus` maps per the plan (paused→running, dead→failed, removing→null).
   - `runStatusDisplay` covers all 10 canonical statuses plus `archived`.
   - `BOARD_STATUS_EVENTS` includes `run.queued`, `run.blocked`, `run.unblocked`, plus the four `interview.*` events.

5. **CLI** (`fabro-cli/src/server_runs.rs`, `commands/runs/list.rs`, `commands/run/wait.rs`)
   - `wait.rs` polls on `RunStatus::is_terminal()`, breaking only on `Succeeded`/`Failed`/`Dead` so `Blocked` and `Queued` keep waiting; success exit requires `RunStatus::Succeeded`.
   - No synthetic `Dead` fallback for missing summary status; `server_runs.rs` reads `run.lifecycle.status` directly.
   - Test plan cases (`Dead`, `Succeeded`, `Failed`) are covered in the `wait.rs` test module.

Validation results:
- `cargo build --workspace`: ✅
- `cargo nextest run --workspace`: ✅ 5827/5827 passed
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`: ✅
- `cargo +nightly-2026-04-14 fmt --check --all`: ✅
- `bun test` (apps/fabro-web): ✅ 396/396 passed
- `bun run typecheck` (apps/fabro-web): ✅

No code changes were required; every step of the plan was already in place and is now verified end-to-end.