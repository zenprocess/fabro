Cleanup review completed. I found and fixed three small issues:

- Moved `TEST_PRINCIPAL` import in `app/lib/run-actions.test.ts` back with the other imports.
- Removed an unnecessary explanatory comment from `app/lib/test-fixtures.ts`.
- Avoided computing `principalDisplay(run.createdBy)` when the `created_by` column is hidden by extracting a small `CreatedByCell` component.

Verification run:

- `cd apps/fabro-web && bun run typecheck`
- Targeted `bun test` for the touched frontend test files: 104 passed, 0 failed.
- `git diff --check` passed.

Current working tree has the three cleanup modifications above.