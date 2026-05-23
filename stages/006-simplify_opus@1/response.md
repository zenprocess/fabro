Confirmed: all 3 failures are pre-existing (graph rendering subprocess issue) and unrelated to my changes.

## Summary

Ran three parallel review agents (reuse, quality, efficiency) on the live-timing change and applied the highest-leverage fixes:

**Fixed**

1. **`live_run_timing` body** (`fabro-types/src/run_projection.rs`) — replaced the hand-rolled `(u64, u64)` tuple fold with the existing primitives `RunTiming::from(StageTiming)` + `RunTiming::saturating_add` + `RunTiming::with_wall_time`, matching how `billing_rollup` already aggregates stage timings. Also removed the dead `unwrap_or(0)` (the `.max(0)` clamp guarantees the conversion can't fail).

2. **DRY in `RunProjectionCache`** (`fabro-store/src/slate/projection_cache.rs`) — `get_summary` no longer inlines its own children-count + timing overlay; both `list` and `get_summary` now share a single `apply_read_overlays` helper and a single `with_children_count` helper.

3. **Encapsulation: overlay condition** — switched from `summary.timing.is_none()` to `projection.conclusion.is_none()`. This expresses the actual intent ("apply live overlay only when the run has not concluded") instead of inferring it from a derived value, and stops coupling the cache to `build_summary`'s internal contract.

4. **Efficiency: overlay no longer runs under the cache mutex** — the timing computation now runs on the cloned `CachedRunProjection` Vec after the mutex is released, so `live_run_timing(now)` iteration no longer contends with the event-ingest hot path.

5. **Deleted a tautological test** — `live_run_timing_matches_conclusion_timing_at_conclusion_moment` constructed both sides of its equality by hand; it exercised nothing the other two tests didn't already cover.

**Skipped (deliberately)**

- The biggest single finding was parameter sprawl: `now: DateTime<Utc>` threaded through 6 store APIs. The original plan explicitly directed this design, so reversing it is out of scope for a cleanup pass.
- Moving `live_run_timing` from `fabro-types` to `fabro-store` was also explicitly directed by the plan.

**Verification**

- `cargo check --workspace` — clean
- `cargo clippy --workspace --all-targets -- -D warnings` (nightly-2026-04-14) — clean
- `cargo fmt --all` — clean
- `cargo nextest run -p fabro-store -p fabro-types` — 432/432 pass
- `cargo nextest run -p fabro-server` — 583/586 pass; the 3 failures are pre-existing graph-rendering subprocess issues (verified by stashing my changes and reproducing the same 3 failures on the parent commit).