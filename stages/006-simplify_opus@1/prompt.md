Goal: # Replace DIY overlay primitives in fabro-web

## Context

`apps/fabro-web` hand-rolls Tooltip, HoverCard, and a Toast system. ~285 lines of overlay code with weak collision detection, no keyboard a11y on the CSS-only tooltips, and a custom Toast context that no longer earns its complexity. Already on `@headlessui/react` for Dialog/Menu — Headless doesn't ship Tooltip/HoverCard/Toast, so this is a real gap, not redundancy.

Goal: delete the DIY code, gain real a11y/positioning, keep call sites stable.

## Scope (3 areas)

### 1. Tooltip + HoverCard → Radix wrappers

Add `@radix-ui/react-tooltip` and `@radix-ui/react-hover-card`.

Keep the public API (`<Tooltip label={x}>{children}</Tooltip>`, `<HoverCard content={x}>{children}</HoverCard>`) by reimplementing the two components in `app/components/ui.tsx` as thin Radix wrappers. All 13 existing call sites remain unchanged.

- Delete `useHoverAnchor` (ui.tsx:141-179).
- Mount one `TooltipProvider` in `app/layouts/app-shell.tsx` (delay 200, skipDelayDuration 300) so siblings share a delay group.
- HoverCard wrapper passes `openDelay` (default 0, stage-sidebar still passes 200) → Radix `openDelay`.
- Keep `PopoverHeader` / `PopoverRows` / `PopoverRow` unchanged — presentational, used inside HoverCard `content`.

Call sites (do not touch): `run-billing`, `settings-live-events`, `run-sandbox/{services,vnc,filesystem}-panel`, `terminal-view`, `size-chip`, `run-summary-panel`, `event-debug` (Tooltip wrapper use), `meta-bar`, `human-qa`, `run-table-row`, `run-waterfall`, `stage-sidebar`, `run-stages`, `run-detail/header`.

### 2. Toast system → Sonner

Add `sonner`. Mount `<Toaster richColors position="bottom-right" />` in `app/layouts/app-shell.tsx` next to the new `TooltipProvider`.

Replace `app/components/toast.tsx` with a tiny shim that preserves the current API:
```ts
// useToast() returns { push, dismiss, clear }
// push({ message, tone, autoDismissMs }) → toast(msg) / toast.error(msg) / toast(msg, { duration })
```
Keep the shim so the 10 consumers + `useRunToasts` need zero changes. `action` field unused in production — drop from the type (only the test referenced it).

Rewrite `toast.test.tsx` against the shim's observable behavior (rendered text, error persistence) rather than `data-toast-id`. Other tests that wrap in `<ToastProvider>` keep working because the shim re-exports a no-op `ToastProvider` (sonner's `Toaster` is mounted globally).

### 3. CSS-only tooltips → real Tooltip

Replace the inline `group-hover/*` blocks in `app/routes/settings-models.tsx` (test-error message ~L519, alias list ~L545) with the new `<Tooltip label={...}>` wrapper. Gains keyboard focus + Esc dismiss + collision avoidance.

### 4. SVG-anchored hovers → shared `FloatingTooltip` helper

Two sites anchor to a measured `DOMRect` from SVG/Graphviz output (no wrappable trigger element): `app/routes/run-overview.tsx:303-318` and `app/components/event-debug.tsx:423-432` (+ the thread-DNA one near :639).

Extract a single helper in `app/components/floating-tooltip.tsx`:
```ts
function FloatingTooltip({ rect, placement, children }) // portals to body, applies collision-avoiding style
```
Absorb the logic of `hover-card-style.ts` into it (cover `top`/`bottom` placements). Delete `app/components/hover-card-style.ts`. Both sites use the helper; `run-overview` renders `<StagePopover>` inside.

## Files to modify

Modify:
- `app/components/ui.tsx` — replace Tooltip/HoverCard impls; delete useHoverAnchor
- `app/components/toast.tsx` — shrink to ~30-line sonner shim
- `app/components/toast.test.tsx` — rewrite assertions
- `app/layouts/app-shell.tsx` — mount `TooltipProvider` + sonner `<Toaster />`, drop `<ToastProvider>`
- `app/routes/settings-models.tsx` — swap two inline CSS tooltips for `<Tooltip>`
- `app/routes/run-overview.tsx` — use `FloatingTooltip`
- `app/components/event-debug.tsx` — use `FloatingTooltip` (two call sites)
- `apps/fabro-web/package.json` — add `@radix-ui/react-tooltip`, `@radix-ui/react-hover-card`, `sonner`

Create:
- `app/components/floating-tooltip.tsx`

Delete:
- `app/components/hover-card-style.ts`

## Verification

1. `cd apps/fabro-web && bun run typecheck` — no type errors.
2. `cd apps/fabro-web && bun test` — `toast.test.tsx` passes against new shim; all other tests unchanged.
3. Run dev locally (`fabro server start` + `cd apps/fabro-web && bun run dev`) and exercise:
   - Tooltips: hover the refresh button on `/runs/:id/sandbox/services`, status chip on `/runs/:id/billing`, run-table-row status icons. Confirm hover delay (~200ms shared), Esc dismisses, keyboard focus opens.
   - HoverCards: hover stage rows in the stage sidebar, waterfall rows, and the run-detail header chips. Confirm positioning flips near viewport edges (Radix collision detection).
   - Toasts: trigger a failed `/runs/:id` action (e.g. retry an unretryable run), confirm red toast persists; trigger a success toast (e.g. archive), confirm auto-dismiss; deep-link to a missing file under `/runs/:id/files/missing-path` — confirm 5s warning.
   - SVG hovers: hover Graphviz nodes on `/runs/:id` overview; hover the waterfall event chips in event-debug. Confirm tooltips appear above and clamp to viewport.
4. Lighthouse/axe spot check on settings-models: confirm aliases + test-error tooltips now reachable via keyboard.

## Out of scope

- `ConfirmDialog`, `RowActionsMenu` — already on Headless UI Dialog/Menu, no change.
- `CollapsibleFile` — 40-line one-off, marginal win, leave.
- Theming changes; visual output should match current styling pixel-close.

## Open questions

- Do we want to brand sonner toasts (custom `toastOptions` for color tokens), or accept sonner defaults? Defaults are dark-themed and read well against `bg-panel`, so likely fine.
- `TooltipProvider` `skipDelayDuration` value — 300ms is a sensible default for grouped hovers across a sidebar; revisit if it feels off in use.


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
  - Model: gpt-5.5, 3.3m tokens in / 39.0k out


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