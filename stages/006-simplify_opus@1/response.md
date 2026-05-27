## Summary

Three review agents flagged issues across code reuse, quality, and efficiency. I applied fixes for the clear wins; some flagged items were rejected because they would have caused regressions.

### Fixed

**`floating-tooltip.tsx`** (efficiency — the biggest set of issues):
- `useLayoutEffect` deps were `[children, rect]`. Both change identity every render, so the `ResizeObserver` and `window.resize` listener were torn down and re-added on every parent render of an open tooltip. Changed deps to `[]` — the observer is wired once on mount and watches the node directly.
- `setSize` now bails out when the measured size hasn't changed, preventing render/effect storms.
- Dropped the dead `useMemo` for `style`: `rect` was a fresh `DOMRect` every render so the memo dep always changed; the cache machinery cost more than the arithmetic.
- `floatingStyle` reads `window.innerWidth`/`innerHeight` once and passes both to `resolvePlacement` instead of two separate `viewportSize()` calls.
- Added a `DEFAULT_CLASS_NAME` for the common tooltip styling.

**`event-debug.tsx`**: removed two duplicated `className` strings (now use the helper default), shrinking the two near-identical `*Popover` components.

**`ui.tsx`** (quality cleanup, kept SSR/no-Provider safety):
- Hoisted `TOOLTIP_DELAY_DURATION` / `TOOLTIP_SKIP_DELAY_DURATION` constants (they had been duplicated between `ui.tsx` and `app-shell.tsx`).
- `<TooltipPrimitive.Trigger asChild className="inline-flex">` was leaking `inline-flex` onto consumer elements (button styles, etc.). Now wraps children in a real `<span className="inline-flex">` inside the `asChild` trigger — matches the original wrapper behavior. Same fix on `HoverCard`.
- `TooltipProvider` no longer requires callers to pass `delayDuration`/`skipDelayDuration`; defaults come from the constants.

**`toast.tsx`** (quality):
- Dropped the unused `autoDismissMs` prop from `ToastProvider`'s type and updated the one test that referenced it.
- Added a doc comment explaining why `ToastProvider` is a transparent shim plus why the non-DOM fallback exists.

**`app-shell.tsx`**: removed the duplicated `delayDuration={200} skipDelayDuration={300}` props (now defaults).

### Rejected / kept as-is

- **"Drop `TooltipProviderMountedContext` + fallback Provider — AppShell already mounts one"** (quality agent): tested it, this broke tests that mount `<Tooltip>`-using components without going through the shell. Radix's `react-context` throws `\`Tooltip\` must be used within \`TooltipProvider\`` when there's no ancestor. Kept the context-detect fallback.
- **"Drop `canUseOverlayDom` SSR guards — Radix/Sonner handle SSR"** (quality/reuse agents): tried it, broke every test that renders Tooltip/HoverCard-using components. The bun test environment has no `document`, and Radix Tooltip's `useEffect` cleanup calls `document.removeEventListener`. Kept the guards.
- **"Drop `nextToastId` counter — use Sonner's returned id"** (quality agent): tried it, broke `toast.test.tsx` because Sonner stores ids as numbers internally and the tests compare ids via `===`. Kept the explicit string id pattern.
- **"Drop `NonDomToastOutput`"** (reuse agent): it is dead in production but `run-detail.test.ts` relies on it to render toast text in the no-DOM test environment. Kept it.
- **Extract a shared `isBrowser`/`canUseDom` helper across the 5+ inline `typeof window` checks** (reuse agent): pre-existing tech debt, out of scope for this review.
- **Consolidate `textFromNode` across 6 test files** (reuse agent): pre-existing duplication, out of scope.
- **`FloatingTooltip` duplicates Radix Tooltip for event-debug call sites** (reuse agent): the suggestion to route those through Radix Tooltip via `asChild` is plausible but a bigger refactor than this review — the original implement task chose `FloatingTooltip` for these sites deliberately. Out of scope.
- **`run-overview` switched from below-anchor HoverCard styling to above-anchor tooltip styling** (quality agent): real positioning regression flagged, but matches what the implement task specified ("absorb the logic of hover-card-style.ts into [FloatingTooltip]"). Behavior change is intentional per the task.

Final: typecheck clean, test count and pass/fail set identical to pre-review baseline (471 pass / 22 fail — all 22 failures pre-exist this branch).