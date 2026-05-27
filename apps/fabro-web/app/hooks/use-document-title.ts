import { useEffect } from "react";

/**
 * Sets `document.title` to `title` and restores the previous title on unmount.
 * Synchronizes React with the browser's `document.title` global.
 */
export function useDocumentTitle(title: string): void {
  useEffect(() => {
    const previous = document.title;
    document.title = title;
    return () => {
      document.title = previous;
    };
  }, [title]);
}
