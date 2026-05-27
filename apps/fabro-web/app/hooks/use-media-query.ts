import { useSyncExternalStore } from "react";

const noop = () => () => {};

/**
 * Returns `true` while the browser matches the given CSS media query string.
 * Uses `useSyncExternalStore` to stay in sync with `MediaQueryList` changes
 * without an effect. Falls back to `false` in SSR and test environments
 * without a `window` global.
 */
export function useMediaQuery(query: string): boolean {
  return useSyncExternalStore(
    typeof window === "undefined"
      ? noop
      : (onStoreChange) => {
          const mql = window.matchMedia(query);
          mql.addEventListener("change", onStoreChange);
          return () => mql.removeEventListener("change", onStoreChange);
        },
    () =>
      typeof window === "undefined" ? false : window.matchMedia(query).matches,
    () => false,
  );
}
