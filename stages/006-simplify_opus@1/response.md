Net **−84 lines** in `diagnostics.rs`, all tests still pass, clippy clean.

## Summary

Three review agents ran in parallel against the diff. After aggregating their findings, I applied these fixes to `lib/crates/fabro-server/src/diagnostics.rs`:

1. **Reused `fabro_llm::model_test::run_basic_model_probe`** instead of the duplicated `probe_llm_provider` function. Same `"Say OK"` / `max_tokens=16` / 30s timeout flow already lived in `fabro-llm`; ran_manifest.rs already uses it. Deleted the local copy.

2. **Eliminated the `selected`/`extras` HashSet+Vec dance.** `state.configured_llm_provider_ids()` already returns providers in catalog order, and auth/registration-issue providers are by construction a subset (resolve only runs on providers with credential material). The new code is a single `join_all` over `configured_providers`.

3. **Dropped the redundant `has_provider` check** in `probe_single_provider`. With the new structure (iterate configured providers, attach pre-probe issues), a provider with no auth/registration issue is guaranteed to be registered.

4. **Collapsed two `provider_probe_error*` helpers into one** with an optional `diagnostic_detail` parameter. Kept the `diagnostic_detail` field itself because it carries real signal (auth-issue messages already embed the provider's display name, so the diagnostics consumer must not re-prefix).

5. **Deleted `public_error_message`** — it was a 1-line wrapper over `redact_string`. Inlined direct calls.

6. **Kept `u32::try_from(...).unwrap_or(u32::MAX)` with `saturating_sub`** — clippy rejected the simpler `as u32` and this is the codebase idiom (8+ existing call sites).

**Findings I did not act on** (noted but not worth fixing):
- Reuse `ModelTestStatus`/`ModelTestOutcome` directly as the API DTO: OpenAPI status enums differ (`ok|error|skip` vs `ok|error`) and the current `ProviderTestResult` shape is intentionally different (adds `provider`/`model_id`).
- Split internal vs API DTO types: `#[serde(skip)]` on `diagnostic_detail` keeps it off the wire; the smell is minor for a single field.
- Hot-path/concurrency findings: efficiency reviewer confirmed no regressions; `resolve_llm_client` is still called once per diagnostics run.
- Test fixture deduplication: out of scope for this cleanup.

**Verification**: 21 targeted tests pass (`diagnostics test_providers`), full `fabro-server` suite runs 670 tests with only 3 pre-existing graphviz failures (confirmed by re-running on origin's HEAD). `cargo build -p fabro-api` clean. `cargo clippy --workspace --all-targets` clean.