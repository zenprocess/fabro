import { useEffect, useRef, type RefObject } from "react";

/**
 * Attaches a `ResizeObserver` to the element referenced by `ref` and calls
 * `callback` with each `ResizeObserverEntry`. Disconnects on unmount or when
 * the observed element changes.
 *
 * The callback ref is updated on every render so the latest version fires
 * without restarting the observer. Synchronizes React with the browser
 * `ResizeObserver` API.
 */
export function useResizeObserver<T extends Element>(
  ref: RefObject<T | null>,
  callback: (entry: ResizeObserverEntry) => void,
): void {
  const callbackRef = useRef(callback);
  callbackRef.current = callback;

  useEffect(() => {
    const el = ref.current;
    if (!el) return;

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) callbackRef.current(entry);
    });
    observer.observe(el);
    return () => observer.disconnect();
  }, [ref]);
}
