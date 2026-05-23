Goal: # Plan: Live `Run.timing` for in-flight runs

## Context

`Run.timing` is only populated for terminal runs, because `build_summary` reads it solely from `state.conclusion.timing`. As a result, the duration chip in the run-detail header (and the new duration popover) is hidden for every queued/running/blocked run — and the same blank state shows up wherever else the UI reads `Run.timing` (runs list, board, billing summary).

The data needed to populate timing for an in-flight run is already on the projection:

- `RunProjection.start.start_time` — set at `RunStarted`.
- Per-stage `StageProjection.timing: Option<StageTiming>` — set at `StageSucceeded` / `StageFailed`.

The fix is to derive a best-effort `RunTiming` at read time for any started-but-not-terminal run and overlay it onto the cached summary, leaving the event-sourced `Conclusion.timing` snapshot untouched.

## Design

`build_summary` stays deterministic and pure (no `now` argument). The cached `Run.summary` in `CachedRunProjection` keeps current behavior: `timing` is `Some(conclusion.timing)` for terminal runs and `None` otherwise.

Live derivation happens at cache read time. Two cache accessors take a `now: DateTime<Utc>` and, when `summary.timing.is_none()`, fill it in from the cached projection.

### Derivation rule (`RunProjection::live_run_timing(now)`)

- If `self.start.is_none()` → return `None` (run hasn't started; chip stays hidden, matching today's UX for queued).
- Otherwise:
  - `wall_time_ms = now - start.start_time` (saturating to 0).
  - `inference_time_ms = sum over stages where timing.is_some() of timing.inference_time_ms`.
  - `tool_time_ms = sum the same way`.
  - Build via `RunTiming::new(wall, inference, tool)` so `active_time_ms` is the derived `inference + tool`.

**Known limitation (call out in code comment and PR description):** `StageProjection` does not track live inference/tool times during a stage — those fields land only at stage completion. So `active_time_ms` for an in-flight run reflects work through the last *completed* stage, and steps forward each time a stage finishes, while `wall_time_ms` ticks continuously. This is acceptable; the popover row reads "Active (inference + tools)" which matches the semantics (sum of completed inference + tool time). Adding live tracking is out of scope for this change.

## Critical files

- **`lib/crates/fabro-types/src/run_projection.rs`** — add `RunProjection::live_run_timing(&self, now: DateTime<Utc>) -> Option<RunTiming>`. This mirrors the existing `StageProjection::live_wall_time_ms(now)` pattern (same file, lines 141-153) and reuses `RunTiming::new` from `lib/crates/fabro-types/src/timing.rs:92`.

- **`lib/crates/fabro-store/src/slate/projection_cache.rs`** — change `RunProjectionCache::get_summary` and `list` signatures to take `now: DateTime<Utc>`. When the cached `entry.summary.timing.is_none()`, call `entry.projection.live_run_timing(now)` and assign the result to `summary.timing` on a cloned copy before returning.

- **`lib/crates/fabro-store/src/slate/mod.rs:255,297`** — propagate the `now` parameter through `Slate::list` and `Slate::get_summary`. Update callers in the same file (`get_run`, etc., around line 291).

- **`lib/crates/fabro-server/src/`** — at the HTTP handler call sites (the GET `/runs/{id}` summary and the runs list endpoints), pass `Utc::now()`. There are only a handful of these; grep for `slate.get_summary(` and `slate.list(`.

- **`lib/crates/fabro-store/src/run_state.rs`** tests (around line 942 onward) — add unit tests for `RunProjection::live_run_timing`:
  - returns `None` when `start` is `None`
  - returns `Some` with derived wall + active from completed stages when started and in-flight
  - matches `conclusion.timing` semantics when called at the conclusion moment (sanity check)

## What does NOT change

- `Conclusion.timing` stays as the event-sourced terminal snapshot. All five internal readers (`run_state.rs:670`, `cli/output.rs:208`, `pipeline/finalize.rs:588`, `operations/start.rs:1771`, `pipeline/pull_request.rs:175`) continue to use it untouched. They semantically want "timing as of conclusion," which is what they get.
- `build_summary` signature stays the same — no `now` argument, no test churn from the existing 13+ test call sites.
- The web client. The duration chip and `DurationPopover` already render whenever `summary.timing` is non-null. Once the server overlays live timing on in-flight runs, the chip appears and the popover shows real values. The popover's "Wall-clock since created" remains client-computed from `now - created_at` (intentionally different from `wall_time_ms`, which excludes queue time).

## Verification

1. **Unit tests** in `fabro-store`: `cargo nextest run -p fabro-store live_run_timing`.

2. **API smoke test**: `cargo build --workspace` and start the server. With an in-flight run, hit `GET /api/v1/runs/{id}` twice ~5s apart and confirm `timing.wall_time_ms` increases between requests while `timing.active_time_ms` stays equal to the sum of completed stages' active times.

3. **Manual UI check** against an in-flight run (e.g. `http://127.0.0.1:32276/runs/01KSA36AH4GPG3D8P1EP0HBG7X`):
   - Run-detail header now shows the duration chip.
   - Hover reveals the popover with both rows populated.
   - "Wall-clock since created" > `wall_time_ms` (because it includes queue/setup time) — sanity check.
   - "Active" stays flat while a stage runs, then jumps when the stage completes. Verify this matches the documented behavior, not a bug.
   - Runs list and board show ticking durations for in-flight rows.

4. **Terminal-run regression check**: open a previously-completed run and confirm timing values are byte-identical to before this change (cached `Conclusion.timing` still wins because `summary.timing.is_some()`, so the overlay is skipped).


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
  - Model: gpt-5.5, 173.0k tokens in / 21.0k out


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