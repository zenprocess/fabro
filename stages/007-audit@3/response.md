Audit result: incomplete.

Evidence checked:
- `git status --short`: no output.
- Full direct-effect search still finds effects in hook/lib/test files.
- Route/component direct-effect search excluding `app/hooks`, `app/lib`, and tests returns no matches. This proves the narrow “no direct effects in route/component files” requirement.
- `useMountEffect` search only finds the primitive export in `app/hooks/effects.ts`, not component call sites.
- `cd apps/fabro-web && bun run typecheck` passes.
- `cd apps/fabro-web && bun test --isolate` passes: 493 pass, 0 fail.

Why completion is not proven:
- The policy says the goal is not merely to hide `useEffect` in hooks; remaining hook effects must be approved external integrations.
- At least one existing hotspot remains unresolved:
  - `apps/fabro-web/app/hooks/use-last-successful-run-files-data.ts` still uses an effect to react to SWR data, update refs, record timestamps, and emit an empty-transition toast. This is the exact run-files SWR data/ref bridge called out in the policy hotspot list as needing careful replacement. SWR data and React refs/state are not external systems under the policy, so this hook is not proven compliant.
- Additional remaining hooks also need audit/repair before declaring completion:
  - `apps/fabro-web/app/hooks/use-data-updated-at.ts` sets React state from async/SWR data changes.
  - `apps/fabro-web/app/hooks/use-stage-artifact-download-href.ts` resolves a generated URL from React inputs in an effect and stores it in state.

Passing tests and the clean route/component search are good evidence of progress, but they do not prove every remaining hook effect is an approved integration.

{
  "outcome": "failed",
  "preferred_next_label": "Continue",
  "failure_reason": "The remaining run-files SWR data/ref bridge in useLastSuccessfulRunFilesData is an explicit policy hotspot and is not proven to be an approved external integration.",
  "context_updates": {
    "goal_status": "incomplete",
    "goal_remaining_work": "Repair or replace useLastSuccessfulRunFilesData so failed-revalidation fallback, previous-to-sha tracking, timestamping, and empty-transition toasts are handled without an effect that mirrors SWR data into refs; then audit/fix useDataUpdatedAt and useStageArtifactDownloadHref."
  }
}