Continued the React Effects Policy migration and repaired the three audit blockers.

What changed:
- Removed the `AskFabroLayout` effect bridge:
  - Deleted `app/lib/ask-fabro-layout.tsx`.
  - `RunDetailAskFabroShell` now publishes sidebar layout declaratively via a rendered `<style>` tag with CSS variables.
  - `AppShell` reads those CSS variables for right padding/transition.
  - `AskFabroSidebar` now receives resize-state callbacks through props instead of writing to context.
- Moved install session loading out of a manual effect:
  - Added `app/install-query.ts` with `useInstallSessionQuery(...)` backed by SWR.
  - Removed `useInstallSessionLoader`.
  - Changed install root redirect from an effect to render-time `<Navigate replace />`.
  - Improved initial install token handling so a URL token is read during initial state setup, avoiding a token-entry flash before URL scrubbing runs.
- Replaced the cleanup-only `useMountEffect` in `insights-editor.tsx`:
  - Added `app/hooks/use-insights-query-runner.ts`, a purpose-named timer hook that owns mock query timer setup/cleanup.
  - `InsightsEditor` now consumes that hook and no longer calls `useMountEffect`.

Validation run:
- Direct effects outside hooks/lib/tests:
  ```sh
  rg -n "\buseEffect\b|React\.useEffect|\buse(Layout|Insertion)?Effect\b" apps/fabro-web/app --glob '*.{ts,tsx}' --glob '!apps/fabro-web/app/hooks/**' --glob '!apps/fabro-web/app/lib/**' --glob '!**/*.test.ts' --glob '!**/*.test.tsx'
  ```
  Result: no output.
- `useMountEffect` call-site search:
  ```sh
  rg -n "useMountEffect\(" apps/fabro-web/app --glob '*.{ts,tsx}'
  ```
  Result: only the primitive export in `app/hooks/effects.ts`.
- `cd apps/fabro-web && bun run typecheck` ✅
- `cd apps/fabro-web && bun test --isolate` ✅ — 493 pass, 0 fail

Current modified/untracked files:
- Modified:
  - `apps/fabro-web/app/components/chats/ask-fabro-sidebar.tsx`
  - `apps/fabro-web/app/hooks/use-install-effects.ts`
  - `apps/fabro-web/app/install-app.tsx`
  - `apps/fabro-web/app/layouts/app-shell.tsx`
  - `apps/fabro-web/app/routes/ask-fabro.tsx`
  - `apps/fabro-web/app/routes/insights-editor.tsx`
  - `apps/fabro-web/app/routes/run-detail/docked-controls.tsx`
- Deleted:
  - `apps/fabro-web/app/lib/ask-fabro-layout.tsx`
- Added:
  - `apps/fabro-web/app/hooks/use-insights-query-runner.ts`
  - `apps/fabro-web/app/install-query.ts`

Remaining uncertainty:
- The immediate audit blockers are addressed. A full completion audit should still review the remaining hook/lib effect usages as approved integrations, since the full search still reports effects inside hook modules, SSE libs, and tests.