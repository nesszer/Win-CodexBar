import { useEffect } from "react";
import type { ThemePreference } from "../types/bridge";

export type ResolvedTheme = "light" | "dark";

const MEDIA_QUERY = "(prefers-color-scheme: light)";

function prefersLight(): boolean {
  if (typeof window === "undefined" || !window.matchMedia) {
    return false;
  }
  return window.matchMedia(MEDIA_QUERY).matches;
}

export function resolveTheme(preference: ThemePreference): ResolvedTheme {
  if (preference === "light") return "light";
  if (preference === "dark") return "dark";
  return prefersLight() ? "light" : "dark";
}

function apply(theme: ResolvedTheme) {
  if (typeof document === "undefined") return;
  document.documentElement.dataset.theme = theme;
  document.documentElement.style.colorScheme = theme;
}

/**
 * Applies the user's theme preference at runtime.
 *
 * When `preference === "auto"`, subscribes to `prefers-color-scheme` so the
 * UI flips along with the OS. Explicit "light"/"dark" overrides detach from
 * the media query.
 */
export function useTheme(preference: ThemePreference): void {
  useEffect(() => {
    apply(resolveTheme(preference));

    if (preference !== "auto" || typeof window === "undefined" || !window.matchMedia) {
      return;
    }

    const mq = window.matchMedia(MEDIA_QUERY);
    const handle = (event: MediaQueryListEvent) => apply(event.matches ? "light" : "dark");

    if (typeof mq.addEventListener === "function") {
      mq.addEventListener("change", handle);
      return () => mq.removeEventListener("change", handle);
    }
    // Safari < 14 fallback.
    mq.addListener(handle);
    return () => mq.removeListener(handle);
  }, [preference]);
}
