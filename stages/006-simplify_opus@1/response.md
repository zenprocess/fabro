## Summary

Aggregated three review agents' findings and applied the consensus high-value fixes:

**Quality / reuse fixes:**
- `tool_permissions::known_tool_category` now returns `Option<AgentToolCategory>` directly. Deleted the brittle `Some("read")` → `Read` string match in `handler/llm/api.rs`. `is_auto_approved` and `tool_category` are now enum-typed end-to-end; cli.rs and its tests updated accordingly. Adding a new category is now a compile-time exhaustiveness error instead of a silent `Other`.
- `ToolSource::Mcp` now carries `original_name` populated from `info.original_tool_name` in `make_mcp_tools`. Workflow code no longer re-parses qualified MCP names (eliminates the silent fallback bug where a malformed prefix would store the qualified name as "original").
- Moved `agent_tool_source` + `agent_tool_summaries_from_definitions` out of `handler/llm/api.rs`. New `ToolDefinitionWithSource::to_agent_tool_summary()` + `Session::agent_tool_summaries()` colocate the conversion next to the source type. Tests moved to `tool_registry.rs`.
- Collapsed `Session::available_tools` and the duplicate private `effective_tools` into one public `effective_tools()` method.
- Merged `ToolAvailableIcon` and `TodoPendingIcon` into a single `EmptyCircleIcon` component in the sidebar.

**Efficiency fix:**
- Gated `emit_agent_tools_available` on `!is_reused` in the primary path. Reused steerable sessions had been re-emitting the full tool list (with descriptions cloned into the event payload) on every prompt even though registry, access policy, and exposure mode are immutable for the session's lifetime. Failover path keeps its emit since it always builds a fresh session.

**Skipped (cost > benefit):** `mcp_server_from_tool_name` dedup in `fabro-store` would require pulling `fabro-mcp` (with `rmcp`) into the store crate or relocating the parse helper into `fabro-types`. The duplicate is 5 lines using the same delimiter convention — not worth the dependency surface.

**Verification:** `cargo check --workspace --all-targets`, `cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings`, `cargo +nightly-2026-04-14 fmt --check --all`, `cargo nextest run -p fabro-types -p fabro-workflow -p fabro-store -p fabro-api -p fabro-agent` (2250+464 tests), `bun test stage-insights-sidebar` (10/10), and `bun run typecheck` all pass. The 14 unrelated `run-actions.test.ts` failures pre-exist on the parent commit.