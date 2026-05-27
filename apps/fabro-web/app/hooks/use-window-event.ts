import { useEffect, useRef } from "react";

/**
 * Adds `handler` as a `window` event listener for `type` and removes it on
 * unmount. The subscription restarts when `type` or `active` changes.
 *
 * The handler ref is updated on every render so the latest version fires
 * without restarting the listener. Synchronizes React with `window.addEventListener`.
 */
export function useWindowEvent<K extends keyof WindowEventMap>(
  type: K,
  handler: (event: WindowEventMap[K]) => void,
  options?: boolean | AddEventListenerOptions,
  active = true,
): void {
  const handlerRef = useRef(handler);
  handlerRef.current = handler;

  useEffect(() => {
    if (!active || typeof window === "undefined") return;
    const listener = (event: WindowEventMap[K]) => handlerRef.current(event);
    window.addEventListener(type, listener, options);
    return () => window.removeEventListener(type, listener, options);
    // options intentionally omitted: changing options identity should not
    // restart the listener. Pass a stable object if needed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [type, active]);
}
