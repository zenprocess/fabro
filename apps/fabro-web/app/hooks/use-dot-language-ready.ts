import { useState } from "react";
import { registerDotLanguage } from "../data/register-dot-language";
import { useMountEffect } from "./use-mount-effect";

/**
 * Triggers dot language registration with the Pierre syntax highlighter on
 * mount and returns `true` once the async registration resolves. Components
 * that render dot-syntax files should wait for this before rendering the
 * highlighted view to avoid a flash of unstyled content.
 *
 * Registration is idempotent; duplicate calls from Strict Mode remount are
 * harmless because `attachResolvedLanguages` only registers once per
 * highlighter instance.
 */
export function useDotLanguageReady(): boolean {
  const [ready, setReady] = useState(false);

  useMountEffect(() => {
    let cancelled = false;
    registerDotLanguage().then(() => {
      if (!cancelled) setReady(true);
    });
    return () => {
      cancelled = true;
    };
  });

  return ready;
}
