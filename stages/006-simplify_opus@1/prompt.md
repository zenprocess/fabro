Goal: # Agent Context Observability Events Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add durable API/backend events that report loaded memory files, discovered and activated skills, and per-server MCP tool names for agent runs.

**Architecture:** Keep this API-backend scoped. Emit typed `AgentEvent` variants from the existing `fabro-agent` initialization and skill activation paths, convert them through `fabro-workflow` into durable `fabro-types` run events, and document the event contracts. Do not add run projection fields in this pass; consumers can read the event stream/history.

**Tech Stack:** Rust, Serde, Fabro agent/session events, Fabro workflow event conversion, Fabro MCP connection manager, `cargo nextest`.

---

## Scope

Implement these event changes:

- Add `agent.memory.loaded` with memory file paths, byte counts, loaded byte counts, truncation flags, provider profile, total loaded bytes, and budget bytes.
- Add `agent.skills.discovered` with source directories, provider profile, and sorted skill summaries.
- Add persisted `agent.skill.activated` for slash skill expansion and successful `use_skill` tool calls.
- Enrich `agent.mcp.ready` with names-only tool summaries: qualified tool name and original server tool name.

Do not implement ACP-native equivalents in this pass. Do not include memory file contents in any event payload. Do not include MCP tool descriptions or schemas.

## Existing Patterns To Follow

- Read `docs/internal/events-strategy.md` before changing event variants, names, conversion, or progress JSONL behavior.
- Read `docs/internal/testing-strategy.md` before adding or reorganizing tests.
- Follow the current `AgentEvent` flow:
  - `lib/crates/fabro-agent/src/types.rs`
  - `lib/crates/fabro-agent/src/session.rs`
  - `lib/crates/fabro-workflow/src/handler/llm/api.rs`
  - `lib/crates/fabro-workflow/src/event/convert.rs`
  - `lib/crates/fabro-workflow/src/event/names.rs`
  - `lib/crates/fabro-types/src/run_event/agent.rs`
  - `lib/crates/fabro-types/src/run_event/mod.rs`
- Follow Rust import style from `AGENTS.md`: import types by name, import functions through their parent module, and avoid glob imports in production code.

## File Map

- Modify `lib/crates/fabro-types/src/run_event/agent.rs`: add new prop structs and extend `AgentMcpReadyProps`.
- Modify `lib/crates/fabro-types/src/run_event/mod.rs`: add `EventBody` variants for the new event names.
- Modify `lib/crates/fabro-agent/src/types.rs`: add internal `AgentEvent` variants, trace output, and noise filtering decisions.
- Modify `lib/crates/fabro-agent/src/memory.rs`: return memory content plus metadata instead of bare strings.
- Modify `lib/crates/fabro-agent/src/session.rs`: emit memory, skills, skill activation, and enriched MCP events.
- Modify `lib/crates/fabro-agent/src/skills.rs`: emit tool-sourced skill activation from `use_skill`.
- Modify `lib/crates/fabro-mcp/src/connection_manager.rs`: expose or support deterministic names-only tool summaries per server.
- Modify `lib/crates/fabro-workflow/src/event/convert.rs`: convert new agent events to durable event bodies.
- Modify `lib/crates/fabro-workflow/src/event/names.rs`: add event names.
- Modify `lib/crates/fabro-workflow/src/event/events.rs` only if the agent event name mapping also lives there for these variants.
- Modify `lib/crates/fabro-workflow/src/event/stored_fields.rs` only if a new event needs non-standard stored fields; otherwise rely on existing `Event::Agent` handling.
- Modify `docs/internal/events.md`: document new event shapes and the richer MCP payload.
- Add or update tests in `lib/crates/fabro-agent`, `lib/crates/fabro-mcp`, `lib/crates/fabro-types`, and `lib/crates/fabro-workflow`.

---

### Task 1: Add Typed Durable Event Contracts

**Files:**
- Modify: `lib/crates/fabro-types/src/run_event/agent.rs`
- Modify: `lib/crates/fabro-types/src/run_event/mod.rs`
- Test: existing `fabro-types` run event serde tests, or add focused coverage near the existing run event tests.

- [ ] **Step 1: Add agent memory props**

Add event prop structs with this shape:

```rust
pub struct AgentMemoryLoadedProps {
    pub provider_profile:  String,
    pub files:             Vec<AgentMemoryFileProps>,
    pub total_loaded_bytes: usize,
    pub budget_bytes:      usize,
    pub visit:             u32,
}

pub struct AgentMemoryFileProps {
    pub path:         String,
    pub byte_count:   usize,
    pub loaded_bytes: usize,
    pub truncated:    bool,
}
```

- [ ] **Step 2: Add skill props**

Add skill discovery and activation props:

```rust
pub struct AgentSkillsDiscoveredProps {
    pub provider_profile: String,
    pub source_dirs:      Vec<String>,
    pub skills:           Vec<AgentSkillSummary>,
    pub visit:            u32,
}

pub struct AgentSkillSummary {
    pub name:        String,
    pub description: String,
}

pub enum AgentSkillActivationSource {
    Slash,
    Tool,
}

pub struct AgentSkillActivatedProps {
    pub skill_name: String,
    pub source:     AgentSkillActivationSource,
    pub visit:      u32,
}
```

Use serde names `slash` and `tool` for `AgentSkillActivationSource`. If a local enum string pattern already exists, follow that pattern.

- [ ] **Step 3: Extend MCP ready props**

Extend `AgentMcpReadyProps` with a backwards-compatible field:

```rust
#[serde(default, skip_serializing_if = "Vec::is_empty")]
pub tools: Vec<AgentMcpToolSummary>,
```

Add:

```rust
pub struct AgentMcpToolSummary {
    pub name:          String,
    pub original_name: String,
}
```

- [ ] **Step 4: Add EventBody variants**

Add `EventBody` variants using these serialized event names:

- `agent.memory.loaded`
- `agent.skills.discovered`
- `agent.skill.activated`

Keep existing `agent.mcp.ready` name unchanged and only enrich its props.

- [ ] **Step 5: Add serde tests**

Cover:

- New event names serialize to the expected dot names.
- `AgentSkillActivationSource` serializes as `slash` and `tool`.
- Old `agent.mcp.ready` JSON without `tools` still deserializes with `tools == []`.

---

### Task 2: Add Internal Agent Events And Conversion

**Files:**
- Modify: `lib/crates/fabro-agent/src/types.rs`
- Modify: `lib/crates/fabro-workflow/src/event/convert.rs`
- Modify: `lib/crates/fabro-workflow/src/event/names.rs`
- Modify: `lib/crates/fabro-workflow/src/event/events.rs` if needed by the existing name mapping.
- Test: `lib/crates/fabro-workflow` event conversion tests.

- [ ] **Step 1: Add internal AgentEvent variants**

Add variants equivalent to:

```rust
MemoryLoaded {
    provider_profile: String,
    files: Vec<MemoryFileSummary>,
    total_loaded_bytes: usize,
    budget_bytes: usize,
}

SkillsDiscovered {
    provider_profile: String,
    source_dirs: Vec<String>,
    skills: Vec<SkillSummary>,
}

SkillActivated {
    skill_name: String,
    source: SkillActivationSource,
}

McpServerReady {
    server_name: String,
    tool_count: usize,
    tools: Vec<McpToolSummary>,
}
```

Prefer small shared internal structs near `AgentEvent` if that matches the existing file organization.

- [ ] **Step 2: Persist skill activation**

Do not classify `SkillActivated` as streaming noise. The existing `SkillExpanded` event is currently filtered before persistence; replace slash expansion emissions with `SkillActivated { source: Slash }` or keep `SkillExpanded` internal-only if removing it would create unnecessary churn.

- [ ] **Step 3: Add trace behavior**

Update `AgentEvent::trace` so the new events emit concise tracing summaries:

- memory loaded: profile, file count, total loaded bytes, budget bytes
- skills discovered: profile, skill count, source dir count
- skill activated: name and source
- MCP ready: server, count, and summary count

- [ ] **Step 4: Convert to durable events**

Update `fabro-workflow` event conversion so the new agent events map to the new `fabro-types` props and include `visit`.

- [ ] **Step 5: Add conversion tests**

Cover each new event with a focused conversion assertion that checks:

- durable event name
- `visit`
- core fields
- no memory content in the converted payload

---

### Task 3: Emit Memory Loaded Metadata

**Files:**
- Modify: `lib/crates/fabro-agent/src/memory.rs`
- Modify: `lib/crates/fabro-agent/src/session.rs`
- Test: relevant `fabro-agent` memory/session tests.

- [ ] **Step 1: Change memory discovery return type**

Change memory discovery from bare `Vec<String>` to a document type carrying both prompt content and event metadata:

```rust
pub struct MemoryDocument {
    pub path:         String,
    pub content:      String,
    pub byte_count:   usize,
    pub loaded_bytes: usize,
    pub truncated:    bool,
}
```

Keep existing behavior unchanged:

- provider profile filename candidates stay the same
- root-to-working-dir walk stays the same
- content dedupe stays the same
- empty files are skipped
- total budget remains 32 KiB
- truncated content keeps the existing truncation marker

- [ ] **Step 2: Preserve prompt assembly behavior**

Adjust session/profile prompt assembly to pass only memory contents where prompt assembly expects memory text. The system prompt should be byte-for-byte equivalent except where existing tests allow non-semantic ordering differences.

- [ ] **Step 3: Emit agent.memory.loaded**

In `Session::initialize()`, emit `AgentEvent::MemoryLoaded` immediately after memory discovery, before skills and MCP initialization.

Emit the event even when no memory files are loaded. That lets consumers distinguish "no memory" from "not reported."

- [ ] **Step 4: Add memory tests**

Cover:

- loaded file path appears in event metadata
- `byte_count` is the original file byte count
- `loaded_bytes` reflects bytes actually loaded into the prompt budget
- `truncated` is true only for truncated files
- event payload never contains memory file contents
- empty discovery still emits a memory-loaded event with `files == []`

---

### Task 4: Emit Skills Discovered And Skill Activated

**Files:**
- Modify: `lib/crates/fabro-agent/src/session.rs`
- Modify: `lib/crates/fabro-agent/src/skills.rs`
- Test: relevant `fabro-agent` skill/session tests.

- [ ] **Step 1: Emit skills discovered**

After `discover_skills(...)`, emit `AgentEvent::SkillsDiscovered` with:

- `provider_profile`
- `source_dirs`
- sorted `skills: [{ name, description }]`

Emit the event even when no skills are discovered.

- [ ] **Step 2: Emit slash activation**

Where slash skill expansion currently emits or creates `SkillExpanded`, emit:

```rust
AgentEvent::SkillActivated {
    skill_name,
    source: SkillActivationSource::Slash,
}
```

- [ ] **Step 3: Emit tool activation**

In `make_use_skill_tool`, use `ToolContext::emit_agent_event(...)` after a requested skill is found and before returning the skill template. Emit:

```rust
AgentEvent::SkillActivated {
    skill_name: name.to_string(),
    source: SkillActivationSource::Tool,
}
```

Do not emit activation for failed `use_skill` lookups.

- [ ] **Step 4: Add skill tests**

Cover:

- discovery event includes all discovered skills sorted by name
- discovery event includes configured source directories
- empty discovery emits `skills == []`
- slash expansion emits `source == slash`
- successful `use_skill` emits `source == tool`
- failed `use_skill` does not emit activation

---

### Task 5: Enrich agent.mcp.ready With Names-Only Tool Summaries

**Files:**
- Modify: `lib/crates/fabro-mcp/src/connection_manager.rs`
- Modify: `lib/crates/fabro-agent/src/session.rs`
- Test: relevant `fabro-mcp` or `fabro-agent` MCP tests.

- [ ] **Step 1: Add deterministic tool summaries**

Expose a helper on `McpConnectionManager` or compute in `Session` from `all_tools()`:

- filter tools by `server_name`
- return qualified tool name as `name`
- return server-provided tool name as `original_name`
- sort by qualified `name`

- [ ] **Step 2: Enrich ready emissions**

When emitting `AgentEvent::McpServerReady`, include the tool summaries for that server. Keep existing `server_name` and `tool_count`.

- [ ] **Step 3: Add MCP tests**

Cover:

- ready event includes only tools from the ready server
- summaries are sorted by qualified name
- `name` is the Fabro-qualified MCP tool name
- `original_name` is the server-provided tool name
- descriptions and input schemas are not included

---

### Task 6: Update Event Documentation

**Files:**
- Modify: `docs/internal/events.md`

- [ ] **Step 1: Document new events**

Add sections for:

- `agent.memory.loaded`
- `agent.skills.discovered`
- `agent.skill.activated`

For `agent.memory.loaded`, explicitly state that file contents are excluded.

- [ ] **Step 2: Update MCP ready docs**

Update `agent.mcp.ready` to show:

```json
{
  "server_name": "github",
  "tool_count": 2,
  "tools": [
    {
      "name": "mcp__github__create_issue",
      "original_name": "create_issue"
    }
  ],
  "visit": 1
}
```

- [ ] **Step 3: Record skill event replacement**

If `agent.skill.expanded` remains in internal code or docs, mark it internal-only or replaced by `agent.skill.activated`.

---

### Task 7: Verify

**Files:**
- No new files unless test placement requires it.

- [ ] **Step 1: Run focused tests**

Run:

```bash
cargo nextest run -p fabro-agent -p fabro-workflow -p fabro-types -p fabro-mcp
```

- [ ] **Step 2: Run formatting**

Run:

```bash
cargo +nightly-2026-04-14 fmt --all
```

- [ ] **Step 3: Run clippy for touched crates or workspace**

Prefer the workspace command if time permits:

```bash
cargo +nightly-2026-04-14 clippy --workspace --all-targets -- -D warnings
```

- [ ] **Step 4: Final sanity checks**

Confirm:

- memory events never contain file contents
- skills discovered and memory loaded are emitted even for empty lists
- skill activation is persisted rather than filtered as streaming noise
- `agent.mcp.ready` remains backwards-compatible for old events without `tools`
- docs match the serialized event names and payload shapes



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
  - Model: claude-opus-4-7, 220.1k tokens in / 51.7k out
  - Files: /home/daytona/workspace/fabro/docs/internal/events.md, /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/lib.rs, /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/memory.rs, /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/session.rs, /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/skills.rs, /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/types.rs, /home/daytona/workspace/fabro/lib/crates/fabro-mcp/src/connection_manager.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_event/agent.rs, /home/daytona/workspace/fabro/lib/crates/fabro-types/src/run_event/mod.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/event/convert.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/event/names.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/handler/prompt.rs


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