All checks pass. Here's the summary of fixes applied based on the three review agents:

## Summary

All three review agents agreed on the same top issues. I applied 10 focused fixes:

### Code reuse / quality
1. **Renamed enum `SandboxProvider` → `SandboxProviderKind` across the workspace** (Reuse #4, Quality #1, #6a). The dual `pub use … SandboxProvider as SandboxProviderKind` aliased the same type under two names, producing **duplicate `SandboxProvider`/`SandboxProviderKind` schemas in OpenAPI and duplicate TypeScript SDK models**. Renamed the enum (~15 Rust files in 8 crates), deleted the duplicate OpenAPI schema, regenerated the TS client, and removed the `RuntimeSandboxProvider`/`SandboxProviderTrait` collision workarounds. The trait now cleanly owns `SandboxProvider`, the enum owns `SandboxProviderKind`.
2. **Replaced `render_error` with `fabro_util::error::collect_chain(...).join(": ")`** (Reuse #1, Quality #5b) — matches the convention used in fabro-workflow, fabro-server, etc.
3. **Added `managed_labels::is_managed()` and `MANAGED_LABEL_VALUE` constant** (Reuse #2, Quality #5a) — replaced repeated `labels.get(MANAGED_LABEL).map(String::as_str) == Some("true")` in both providers, plus removed the magic `"true"` literal in label-filter format strings.
4. **Moved `FakeSandboxProvider` to `fabro_sandbox::test_support`** (Reuse #3, Quality #3a) — the two near-identical `FakeProvider`s in `provider.rs` tests and the server's `sandboxes.rs` tests are now a single shared helper behind the existing `test-support` feature (fabro-server already had the dual-listing dev-dep).
5. **Removed `_default_config` field from `DockerSandboxProvider`** (Quality #4b) — was stored but never read; deleted the manifest plumbing in `build_sandbox_provider_registry`.
6. **Dropped the empty `SandboxListFilter` unit struct** (Quality #6b) — was a placeholder parameter every caller passed and every impl ignored; YAGNI.

### Efficiency
7. **Parallelized providers in `SandboxProviderRegistry`** (Efficiency #1) — `list_managed` and `get_managed_by_native_id` now use `futures::future::join_all` instead of serial `for … await`. Docker + Daytona runs concurrently.
8. **Parallelized Docker inspects and reused the client** (Efficiency #2) — `DockerSandboxProvider::list` previously built a new Docker client *per container* via `Self::inspect`, serially. Now it builds one client per request, runs all `inspect_container` calls in parallel via `try_join_all`, and `get`/`delete` share an `inspect_container` helper that takes `&Docker`.
9. **Dropped redundant client-side managed-label re-check in list paths** (Efficiency #2, #3) — daemon-side label filter is the source of truth in `list`. The managed re-check is still enforced in `get` and `delete` since those bypass the filter.

### Robustness
10. **Stopped panicking on vault contention at startup** (Quality #4c) — replaced `vault.try_read().expect("...")` with reading `DAYTONA_API_KEY` from the `Vault` value *before* wrapping it in `Arc<AsyncRwLock<_>>`, then passing the `Option<String>` into `build_sandbox_provider_registry`.

### Verification
- `cargo +nightly-2026-04-14 fmt --check --all` ✅
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` ✅
- `cargo nextest run --workspace` ✅ (6441 tests passed, 181 skipped)
- `cd lib/packages/fabro-api-client && bun run typecheck` ✅

### Findings I deliberately did not act on
- **`SandboxCreateSpec` public surface without external callers** (Quality #4a). The plan explicitly required `create` and `delete` on the trait, so I kept them and didn't gut the public surface beyond what the plan asks. The duplicate `_default_config` was the only piece I removed because it was independently dead.
- **Pre-existing stale `sandbox-provider.ts` and `run-sandbox-settings.ts` in the generated TS client.** These are orphaned files from before this PR (not in the FILES manifest, not exported from `index.ts`); my changes don't worsen them. Cleaning them up is out of scope.