All checks green. Here's the summary:

## Summary

Three review agents flagged overlapping issues. I applied the high-value cleanups and skipped low-value or out-of-scope ones.

### Fixed
1. **Eliminated 4 duplicate types** between `fabro-agent` and `fabro-types` (`McpToolSummary`, `MemoryFileSummary`, `SkillSummary`, `SkillActivationSource`). `AgentEvent` now embeds the `fabro-types` versions directly, mirroring the existing `TodoCreated/Updated/Deleted` pattern. Removed ~50 lines of field-by-field cloning in `convert.rs`.
2. **Deleted dead `SkillExpanded` variant** entirely — its emit site had already been replaced with `SkillActivated { source: Slash }`. Removed from `types.rs` (variant, streaming-noise classification, trace arm, test), `convert.rs` (panic-list), and `names.rs` (name mapping).
3. **Strongly typed `provider_profile`** as `fabro_model::AgentProfileKind` (Copy enum with existing `Display`) on both wire props and in-memory variants instead of `String`. Removed two `.to_string()` calls and per-event clones.
4. **Dropped redundant derived fields** from in-memory variants:
   - `MemoryDocument.loaded_bytes` → always equals `content.len()`.
   - `AgentEvent::McpServerReady.tool_count` → always equals `tools.len()`.
   - `AgentEvent::MemoryLoaded.total_loaded_bytes` → sum of `files[].loaded_bytes`.
   - All three are computed at the conversion boundary so the wire shape is preserved.

### Skipped (with reason)
- **`build_system_prompt` signature change** (memory clone elimination): touches 6 files including public SDK reference docs and 4 profile impls for a single 32 KiB clone at session init. Not worth the API churn.
- **MCP tool N+1 fix**: one-shot pass at session startup, not a hot path.
- **`ToolContext` Option accretion**: out of scope for this PR; should be its own refactor when the next field is added.
- **Two-pass over memory in init**: micro-optimization, current code is clearer.

### Verification
- `cargo check --workspace --all-targets` — clean
- `cargo nextest run -p fabro-agent -p fabro-workflow -p fabro-types -p fabro-mcp` — 1840 passed, 0 failed
- `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings` — clean
- `cargo +nightly-2026-04-14 fmt --all` — applied

Net change: removed ~100 lines while preserving all behavior, wire format, and test coverage.