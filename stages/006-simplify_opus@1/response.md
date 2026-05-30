Typecheck passes. The fixes effectively revert the implement-stage diff (both files now match origin), which is the correct outcome.

## Summary

All three review agents converged on the same findings on the small implement-stage diff:

**Issues fixed:**

1. **Behavior regression in `automations.tsx`** — The new `disabled` prop dropped the `running` term from the run button's disabled state. While the `runAutomation` early-return guard prevented duplicate POSTs, the button was visually clickable with `aria-label="Starting run…"` during the in-flight run. Fixed by restoring `running` to the disabled predicate.

2. **Parameter sprawl in `AutomationCard`** — The new `disabled` prop was mechanically derivable from existing inputs (`busy || !automation.apiEnabled`), and the parent was duplicating the `busy` expression inside the new `disabled` expression. Reverted to internal derivation: `const runDisabled = busy || running || !automation.apiEnabled`.

3. **Inconsistent predicate in `automation-detail.tsx`** — The title prop was re-derived as `!apiTrigger?.enabled` while the adjacent `disabled` used `!canRun`. Reverted to the `canRun ? undefined : "..."` form so both lines share the same alias.

Net effect: the implement-stage diff is reverted to the prior, simpler shape that already lived on `main` after #456. Typecheck passes.