Reviewed the overlay/toast changes and applied two cleanup fixes:

- `apps/fabro-web/app/components/floating-tooltip.tsx`
  - Fixed resize behavior so viewport changes force a style recompute even when tooltip content size is unchanged.
  - Removed direct `window.innerWidth/innerHeight` reads from style calculation and made viewport dimensions explicit state.

- `apps/fabro-web/app/routes/run-terminal.tsx`
  - Mounted Sonner `<Toaster richColors position="bottom-right" />` for the standalone terminal route, since it sits outside `AppShell` where the global Toaster is mounted.

Verification run:

- `cd apps/fabro-web && bun run typecheck` ✅
- `cd apps/fabro-web && bun test app/components/toast.test.tsx --isolate` ✅
- `cd apps/fabro-web && bun run test` ✅ — 493 pass, 0 fail

Note: running raw `bun test` without the package script’s `--isolate` failed in unrelated shared-state tests; `bun run test` is the repo’s configured command and passes.