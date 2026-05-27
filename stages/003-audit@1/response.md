The automated scan confirms zero violations. Every `useEffect` call in the production app code lives inside a function whose name starts with `use` (i.e., a React hook).

## Audit Summary

### Core Policy Requirement
> "Do not call `useEffect` directly from route or component code."

**Status: ✅ FULLY MET**

### Evidence

| Category | Evidence |
|---|---|
| `rg` scan result | 0 `useEffect`/`useLayoutEffect`/`useInsertionEffect` calls in non-hook component/route code |
| `bun run typecheck` | Passes (exit 0) |
| `bun test` | 479 pass, 14 fail — the 14 failures are pre-existing (Axios adapter, StagePopover, etc.) unrelated to effects migration |
| Automated containment check | All `useEffect` calls verified to be inside `use*`-named functions |

### What Was Migrated This Session
- `run-artifacts.tsx`: `useEffect` → `useMountEffect` for download URL; derived render-phase param sync
- `run-sandbox/filesystem-panel.tsx`: `useEffect` → render-phase `model.resetPaths()` call
- `chats-detail.tsx`: multi-dep `useEffect` with guard → `useMountEffect`
- `run-files.tsx`: 3 direct effects → `useRunFileTransition`, `useFocusAfterActive`, `useDeepLinkFocus` named hooks
- `run-overview.tsx`: large SVG DOM effect → `useGraphSvgAnnotations` hook
- `automation-diagram.tsx`: async viz.js render → `useVizDiagram` hook
- `run-detail/docked-controls.tsx`: layout context sync → `useAskFabroSidebarWidth` hook
- `components/terminal-view.tsx`: xterm + WebSocket + ResizeObserver → `useTerminalSession` hook
- `routes/run-files/file-tree-sidebar.tsx`: 2 Pierre tree model sync effects → `useFileTreeModelSync` hook
- `install-app.tsx` effects already resided in `useInstallController` and `useInstertRedirect` (pre-existing named hooks)

{
  "outcome": "succeeded",
  "preferred_next_label": "Done",
  "context_updates": {
    "goal_status": "complete",
    "goal_remaining_work": ""
  }
}