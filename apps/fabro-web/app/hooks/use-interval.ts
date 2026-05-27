import { useEffect, useRef } from "react";

/**
 * Calls `callback` every `delayMs` milliseconds while `active` is true
 * (default: always active). The interval is cleared when the component
 * unmounts or when `active` or `delayMs` changes.
 *
 * The callback ref is updated on every render so the interval always sees
 * the latest version without restarting. Synchronizes React with
 * `setInterval`.
 */
export function useInterval(
  callback: () => void,
  delayMs: number,
  active = true,
): void {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    if (!active) return;
    const id = setInterval(() => callbackRef.current(), delayMs);
    return () => clearInterval(id);
  }, [delayMs, active]);
}
