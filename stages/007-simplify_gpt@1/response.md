Reviewed the latest simplification diff with all three agents:

- Code reuse review: no actionable issues.
- Code quality review: no actionable issues. The only note was that `testPrincipal()` returns a fresh object per call, which is already safe and appropriate.
- Efficiency review: no findings.

No code changes were needed.

Verification run:

```bash
cd apps/fabro-web && bun test \
  app/components/run-summary-panel.test.tsx \
  app/data/runs.test.ts \
  app/lib/run-actions.test.ts \
  app/routes/automations-new.test.tsx \
  app/routes/run-detail.test.ts \
  app/routes/run-files.render.test.tsx \
  app/routes/runs.preferences.test.tsx \
  app/routes/runs.test.tsx
```

Result: `104 pass, 0 fail`.

Working tree remains clean.