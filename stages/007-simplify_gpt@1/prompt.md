Goal: # Mid-Stage Agent Interview Tools

## Summary

Add model-native question tools that let agents pause mid-stage and ask the human for input through Fabro's existing interview system.

OpenAI-profile agents get `request_user_input`; Anthropic-profile agents get `AskUserQuestion`. When either tool is called, Fabro creates pending interview questions, surfaces them through the existing web/API/Slack paths, waits for answers, then returns provider-shaped tool results so the model can continue the same stage.

## Key Changes

- Extend the existing interview contract without type sprawl:
  - Add optional `description` and `preview` fields to the canonical `fabro_types::InterviewOption`; reuse that type through `fabro-api` replacements instead of introducing `AgentQuestionOption`, API-only aliases, or adapter-only duplicate types.
  - Update OpenAPI, generated Rust/TypeScript clients, event conversion, projection, Slack/web mappers, and the existing `with_replacement("InterviewOption", "fabro_types::InterviewOption", ...)` parity tests.
  - Treat both fields as untrusted model-authored display data. Store and expose them after enforcing bounded lengths; truncate or reject oversized values consistently before persistence.
  - Initial UI behavior: display `description` under option labels where practical. Capture and expose `preview`, but do not render preview content specially in web or Slack v1.

- Add a shared run-level interview runtime:
  - Move the private human-node blocked-state refcount into a reusable run-level guard used by both `HumanHandler` and agent question tools, so `RunUnblocked` is emitted only when all human and agent interviews for the run are resolved.
  - Runtime accepts the interviewer, workflow emitter, stage scope, stage id, tool call id, and normalized questions.
  - Support batch asks as a first-class operation: emit/register all questions first, mark the run blocked once, await all answers concurrently, then emit completion/timeout/interrupted events per question and unblock when the batch resolves.
  - Batch support applies only to multiple `questions[]` inside one question-tool call. Do not aggregate multiple separate question-tool calls from the same model round.
  - Generate safe internal question IDs with a ULID/UUID plus stage visit/tool-call context; store original model question IDs/text in question metadata for provider result mapping.

- Add provider-specific agent tools:
  - `request_user_input` for `AgentProfileKind::OpenAi`.
    - Accept Codex-compatible schema: `questions[]` with `id`, `header`, `question`, and `options[] { label, description }`.
    - Normalize each question to `QuestionType::MultipleChoice` with `allow_freeform: true`.
    - Return JSON text matching Codex shape, keyed by the original model question ID: `{"answers":{"id":{"answers":["..."]}}}`.
  - `AskUserQuestion` for `AgentProfileKind::Anthropic`.
    - Accept Claude-compatible schema: `questions[]` with `question`, `header`, `options[] { label, description, preview? }`, and `multiSelect`.
    - Normalize single-select to `MultipleChoice`, multi-select to `MultiSelect`, always with `allow_freeform: true`.
    - Return Claude-style tool result text keyed by the original question text: `User has answered your questions: "...question..."="answer". You can now continue...`.
  - Answer formatting for both tools returns user-facing option labels to the model. Preserve internal option keys for validation and event storage. For multi-select, preserve the submission order supplied by the answer path.

- Thread workflow interview context into agent tool execution:
  - Add an explicit per-turn agent tool runtime context passed into `process_input` or an adjacent `process_input_with_runtime` API. It carries the interviewer, workflow emitter, stage scope/id, shared block guard, and provider answer formatter.
  - Do not capture stage-specific interview handles in the profile registry or cached session construction; cached full-fidelity sessions must receive the current turn's stage context dynamically.
  - Child/subagent sessions must not expose these question tools. If somehow called outside the root session, return a model-visible error.
  - Question tools must execute alone in a model tool round. If a round contains one question tool plus any other tool call, execute the question tool and return model-visible error results for the non-question peers, preserving tool-call/tool-result ordering. If a round contains multiple separate question-tool calls, execute only the first and return model-visible error results for the later question-tool calls instructing the model to combine questions into one `questions[]` batch.
  - Agent-originated questions have no per-question timeout in v1 because the provider schemas do not include timeout. They rely on existing stage timeout, wall-clock timeout, cancellation, and interruption behavior.

- Preserve existing answer paths:
  - Do not add a new answer endpoint.
  - Continue using `GET /runs/{id}/questions` and `POST /runs/{id}/questions/{qid}/answer`.
  - Keep `ControlInterviewer`, web `InterviewDock`, Slack blocks, and run projection as the delivery mechanism.

## Test Plan

- Unit tests for schema parsing and normalization:
  - Codex request with descriptions maps to Fabro multiple-choice questions and returns answers by model question ID.
  - Claude request with `multiSelect: true` maps to `MultiSelect` and returns comma-separated answer text.
  - Batched Codex and Claude requests surface all questions as pending before awaiting answers, then return one result with every answer mapped to the original model ID/text.
  - Optional `preview` and `description` survive event, projection, API conversion, OpenAPI replacement tests, and TypeScript client generation.
  - Oversized `description`/`preview` values are bounded before persistence and never rendered as trusted HTML.

- Workflow and agent tests:
  - OpenAI-profile session advertises `request_user_input`; Anthropic-profile session advertises `AskUserQuestion`; Gemini advertises neither.
  - Subagent profiles do not advertise the question tools.
  - Root agent can ask a question and resume after the answer.
  - Subagent or missing interview context returns a clear tool error.
  - Cached full-fidelity session emits interview events against the current stage, not the original cached stage.
  - A mixed tool round containing a human-question tool plus another tool preserves all required tool results and rejects the peer calls with model-visible errors.
  - A round with multiple separate question-tool calls executes only the first and rejects later question-tool calls with model-visible errors.

- Server, projection, and UI tests:
  - `InterviewStarted` with option metadata appears in pending questions.
  - Submitting valid selected, multi-selected, and freeform answers unblocks the waiting tool.
  - Duplicate answer submission remains rejected through existing accepted-question logic.
  - Parallel human gate plus agent question keeps the run blocked until both are answered.
  - Pause, cancel, and interrupt while an agent question is waiting resolve pending questions consistently and do not leave the run blocked.
  - Stage timeout or wall-clock timeout while an agent question is waiting interrupts the batch; no per-question timeout event is expected unless a future schema adds timeout.
  - Slack answer submissions work for agent-originated questions using the same pending interview transport.

- Run checks:
  - `cargo nextest run -p fabro-interview -p fabro-workflow -p fabro-server -p fabro-agent`
  - `cd apps/fabro-web && bun test && bun run typecheck`
  - Regenerate and verify OpenAPI-derived Rust and TypeScript clients after schema changes.

## Assumptions

- This feature is only for in-process/API-backed agent sessions, not ACP external agents in v1.
- `preview` is stored and exposed but not rendered specially in the first implementation.
- Human-question tools are available only during root agent execution inside a workflow run with an active interviewer.
- Existing interview events remain the source of truth for pending questions; no separate agent-question event family is added.
- The implementation should prefer extending existing interview structs and replacement mappings over adding parallel API DTOs or conversion-only aliases.


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
  - Model: gpt-5.5, 483.3k tokens in / 63.1k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/question_tools.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/interview_runtime.rs
- **simplify_opus**: succeeded
  - Model: claude-opus-4-7, 112.5k tokens in / 28.7k out
  - Files: /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/question_tools.rs, /home/daytona/workspace/fabro/lib/crates/fabro-agent/src/tool_execution.rs, /home/daytona/workspace/fabro/lib/crates/fabro-workflow/src/interview_runtime.rs


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