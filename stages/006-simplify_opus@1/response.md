## Summary

Three parallel review agents (reuse, quality, efficiency) ran against the automation API contract change. I applied four concrete fixes from their findings:

1. **`AutomationDraft.enabled` serde was asymmetric** — `skip_serializing_if = "is_true"` paired with `default = "default_true"` made round-trips lossy on a public API DTO (the test had to use `enabled: false` to dodge it). Dropped the skip attribute and removed the now-unused `is_true` helper (plus its `#[expect(clippy::trivially_copy_pass_by_ref)]`). The create-request round-trip test now uses `enabled: true` — the more interesting case. (`lib/crates/fabro-automation/src/model.rs`)

2. **Removed `NO_TYPE_IMPLS` constant** in `build.rs` — it was used only by the 5 new automation entries while all ~100 existing entries use inline `&[]`. Replaced with inline `&[]` for consistency. (`lib/crates/fabro-api/build.rs`)

3. **Genericized `IfMatch` parameter and `ETag` header descriptions** — both live in shared `components.parameters` / `components.headers` but were worded for "automation revision" only. Rewrote them to be resource-agnostic so future entities (secrets, variables, workflows) can `$ref` them without misleading prose. (`docs/public/api-reference/fabro-api.yaml`)

4. **Simplified round-trip tests** — replaced the runtime `assert_same_type<Api, Domain>()` TypeId check with compile-time witness `const _: fn(Api) -> Domain = |v| v;` (catches the same regressions without runtime tautology). Dropped the redundant second deserialize and `assert_eq!(api, domain)` from each JSON test — the two types are physically identical via `with_replacement`, so those assertions were `assert_eq!(api, api)`. (`lib/crates/fabro-api/tests/automation_round_trip.rs`)

Findings I explicitly did **not** act on (acknowledged but out of scope):

- **`pub use ... as ...` aliases** for `AutomationDraft → CreateAutomationRequest` / `AutomationReplace → ReplaceAutomationRequest`. The HTTP/domain naming gap is a real semantic boundary and the issue spec dictates the API names; renaming either side creates more churn than the alias does.
- **Splitting `fabro-automation` into a types-only sub-crate** to keep `croner`/`tokio`/`toml` out of the build graph of every `fabro-api` consumer — a meaningful build-time cost (`darling`, `derive_builder` added via croner), but a substantial refactor and not in scope here.
- **Hoisting `assert_same_type` into a shared helper** — duplicated across ~40 sibling test files in `fabro-api/tests/`. Pre-existing pattern.
- **Consolidating repeated 4xx error responses** into `components.responses/*` — pre-existing spec style across the whole file.

Verification:

- `cargo build -p fabro-api -p fabro-automation` ✅
- `cargo nextest run -p fabro-api -p fabro-automation` ✅ (175 passed, including the 3 automation round-trip tests)
- `cargo +nightly-2026-04-14 clippy -p fabro-api -p fabro-automation --all-targets -- -D warnings` ✅
- `cd lib/packages/fabro-api-client && bun run generate` ✅ (only updated the `If-Match` description text in three places of `automations-api.ts` to match the new generic phrasing)
- `git diff -- apps/fabro-web lib/crates/fabro-cli` is empty ✅ — acceptance criterion satisfied.