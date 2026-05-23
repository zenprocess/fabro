## Summary

Three review agents flagged ~25 findings; I applied the high-impact, low-risk fixes (about 12 lines net reduction across the touched files, plus correctness wins on schema compilation).

**Fixed:**
1. **Schema compiled twice per node** (efficiency #1, reuse #3, quality #1) — `OutputSchemaKind::JsonSchema` now stores an `Arc<jsonschema::Validator>` compiled once in `parse_node_output_schema`; the routing validator is a `LazyLock<Validator>`, so repair iterations no longer recompile. Also removed the redundant `compile_schema` helper.
2. **`read_sandbox_file` reimplemented `Sandbox::read_file_text`** (reuse #2) — replaced the `cat <quoted>` exec_command call with the existing trait method; dropped the unused `shell_quote` import and the magic 5s timeout.
3. **Redundant re-validation** in `validate_agent_output_sources` (efficiency #6) — bound the first error and reused it instead of calling `validate_response_text` again just to `.expect_err(...)`.
4. **`OneShotCompletion::actual_speed` was always `controls.speed`** (quality #2, reuse #4) — collapsed the three `actual_*` fields into a single `model: ModelRef`, removing the manual reconstruction one line later.
5. **`extract_status_fields` wrapper deletion** (reuse #1, quality #5) — renamed `extract_status_fields_loose` → `extract_status_fields` in `structured_output.rs`; agent.rs re-exports it so existing call sites (including `prompt.rs`) work unchanged.
6. **`find_json_objects` visibility** (quality #6) — tightened from `pub(crate)` to `fn`.
7. **`last_touched_file` cheap accessor** (efficiency #5) — added a tiny helper that grabs only `.last`; the repair loop no longer clones+sorts the full `touched` set each iteration.
8. **`ROUTING_KEYWORD` constant** (quality #8) — extracted the literal `"routing"` to a const used by `parse_node_output_schema`.

**Skipped:**
- Folding `Error::OutputSchemaValidation` into `Error::Validation` — `Validation` adds a `"Validation error: "` prefix that would change user-visible failure messages and downstream event content. Not worth the drift for one extra enum variant.
- Other suggestions (request structs for multi-param functions, deduping the two repair loops across `one_shot`/`run`, moving `validate_agent_output_sources` into `structured_output.rs`, collapsing `StructuredOutputErrorKind`) — judged as larger refactors with no clear correctness payoff for this iteration.

**Verification:** `cargo nextest run -p fabro-types -p fabro-workflow` → 1461 tests pass; `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` clean; `cargo +nightly-2026-04-14 fmt --check --all` clean.