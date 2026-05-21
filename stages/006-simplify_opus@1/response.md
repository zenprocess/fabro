Clean. Let me write a summary of what was fixed:

## Summary of Cleanup

Three review agents identified converging issues around the new `StageTiming`/`RunTiming` value objects. Key fixes applied:

### `fabro-types/src/timing.rs` — make `RunTiming` a real run-level value object
- **Added `RunTiming::wall_only(ms)`** mirroring `StageTiming::wall_only`.
- **Added `RunTiming::saturating_add`** mirroring `StageTiming::saturating_add` so accumulators can be `RunTiming` directly.
- **Added `impl From<StageTiming> for RunTiming`** to eliminate destructure-reconstruct sites.
- **Added `RunTiming::with_wall_time(self, ms)`** for the finalize case where active fields come from the rollup but wall comes from the executor clock.
- **Deleted dead `RunTiming::add_stage_active`** — only its own unit test referenced it; the codebase uses `saturating_add` instead.
- Added unit tests for each new helper.

### Re-type accumulators as `RunTiming` (they always were semantically run-level)
- `ProjectionBillingRollup.timing: StageTiming` → `RunTiming` (`fabro-workflow/src/billing_rollup.rs`).
- `BillingAccumulator.total_timing: StageTiming` → `RunTiming` (`fabro-server/src/server.rs`).

### Replace destructure-reconstruct with conversions
- `fabro-workflow/src/pipeline/finalize.rs`: `RunTiming::new(run_wall_time_ms, t.inference_time_ms, t.tool_time_ms)` → `projection_billing.timing.with_wall_time(run_wall_time_ms)`.
- `fabro-server/src/server/handler/billing.rs`: identical destructure → `totals_timing.into()`.
- `fabro-server/src/server/handler/system.rs`: identical destructure → just pass `agg.total_timing`.

### Delete orphan `RunTimings` (plural) schema
- Removed from `docs/public/api-reference/fabro-api.yaml` (was unreferenced; collided cognitively with new `RunTiming`).
- Deleted regenerated `lib/packages/fabro-api-client/src/models/run-timings.ts` and its entries in `index.ts` / `FILES`.

### Replace `RunTiming::new(N, 0, 0)` (54 sites) with `RunTiming::wall_only(N)`
Bulk-applied across all production and test code in `fabro-types`, `fabro-workflow`, `fabro-server`, `fabro-store`, `fabro-cli`, `fabro-api`, `fabro-dump`. Improves intent clarity and locks down the `active = inference + tool` invariant.

### Web test fixture cleanup
- Added a local `stageTiming(wall, inference, tool)` helper in `run-billing.test.tsx`, replacing 9 hand-rolled `{ wall_time_ms: ..., inference_time_ms: 0, ..., active_time_ms: 0 }` literals (which previously silently violated the invariant on lines using `wall_time_ms: 42000` with `active_time_ms: 0`).

### Findings not addressed (intentionally skipped)
- **`NodeResult` Duration round-trip / 6-arg constructor sprawl** — non-trivial refactor of the executor↔lifecycle contract; out of scope for cleanup of this PR.
- **`RunStage.wall_time_ms` vs `timing: StageTiming` inconsistency on the public API** — would require new spec design choices about whether the `/runs/{id}/stages` listing exposes active breakdown.
- **`get_run_billing` two-pass over projection** — the two passes have different semantics (rollup excludes in-flight; live_rows include them), so merging them is more than mechanical.
- **TS-side `RunTiming` vs `StageTiming` interface identity** — purely cosmetic.

### Validation
- `cargo nextest run -p fabro-types -p fabro-store -p fabro-workflow -p fabro-server`: **2068 passed**.
- `cargo nextest run` across the wider set including `fabro-cli`/`fabro-api`: **3242 passed**.
- `cargo build -q --workspace`: clean.
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`: clean.
- `cargo +nightly-2026-04-14 fmt --check --all`: clean.
- `bun test` (fabro-web): **408 passed**.
- `bun run typecheck` (fabro-web): clean.
- `git diff --check`: clean.