import { useEffect } from "react";

/**
 * Runs `setup` once on mount. The function may return a cleanup that runs on
 * unmount. Use this only when the code attaches to, creates, or subscribes to
 * an external resource and the cleanup disposes it.
 *
 * Do not use `useMountEffect` as a way to avoid dependency arrays when the
 * effect actually depends on changing React values — write a purpose-named hook
 * with those values in its API instead.
 */
// eslint-disable-next-line react-hooks/exhaustive-deps
export function useMountEffect(setup: () => void | (() => void)): void {
  // eslint-disable-next-line react-hooks/exhaustive-deps
  useEffect(setup, []);
}
