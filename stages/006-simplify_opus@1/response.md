Cleanup complete. Here's what was fixed based on the parallel reviews:

**Fixed (3 issues consolidated from reviews):**

1. **Moved `context_window_from_response_usage` from `session.rs` to `context_window.rs`** — it was a free function that only operated on `StageContextWindowProjection` + `TokenCounts` and just delegated to `scaled_snapshot`. Co-located with its only collaborator. This also let `session.rs` drop its imports of `scaled_snapshot` and `StageContextWindowCountMethod`.

2. **Eliminated avoidable `StageContextWindowProjection.clone()` in the per-LLM-turn hot path** — replaced `let local_context_window = built_request.context_window.clone(); let request = built_request.request;` with a destructuring let `BuiltRequest { request, context_window: local_context_window } = self.build_request();`.

3. **Downgraded `warnings_from_llm` from `pub(crate)` to private `fn`** — its only outside caller was removed when the provider token-count path was deleted; it's now only used within `context_window.rs`.

**Reviewed but skipped (false positives or below-the-bar):**

- `BuiltRequest` 2-field struct could be a tuple, but the named-field destructure now reads cleanly and avoids the clone, so it earns its keep.
- `AssistantMessage` 5-field variant — borderline but tightening it to a struct would just rename without compressing.
- Reducer inline clone+stamp pattern — single use site, no helper warranted.
- The producer-side `Option<StageContextWindowProjection>` always being `Some` in `Session::run_single_input` — kept as `Option` because tests and non-AssistantMessage emit sites need `None`, and the wire schema is optional anyway.

**Verification:** `cargo check --workspace --all-targets`, `cargo nextest run -p fabro-agent -p fabro-workflow -p fabro-store -p fabro-types` (2089 tests pass), and the 7 `get_run_stage_context_window` server tests all pass. Pre-existing nightly clippy warning in `fabro-sandbox` (large_enum_variant) is unrelated to this change.