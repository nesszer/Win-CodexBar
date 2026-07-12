import { useEffect, useState } from "react";
import { useLocale } from "./useLocale";

/**
 * Format a provider's reset timestamp for display.
 *
 * When `relative` is true, returns a live "Resets in 3h 42m" style string
 * and includes the reset label because the locale strings include it.
 *
 * When `relative` is false, returns the absolute reset time converted to
 * the user's local timezone via `Intl.DateTimeFormat`, fixing the issue
 * where the backend-supplied `reset_description` was pre-formatted as UTC
 * wall time (e.g., `Mar 5 at 3:00PM`).
 *
 * Falls back to `fallback` (typically the backend's `resetDescription`) when
 * `resetsAt` is absent or unparseable. Some providers use that fallback for
 * non-time details, so callers should not assume it is safe to prefix.
 */
export function useFormattedResetTime(
  resetsAt: string | null,
  fallback: string | null,
  relative: boolean,
): string | null {
  const { t } = useLocale();
  const [now, setNow] = useState(() => Date.now());

  useEffect(() => {
    if (!resetsAt) return;
    const id = window.setInterval(() => setNow(Date.now()), 30_000);
    return () => window.clearInterval(id);
  }, [resetsAt, relative]);

  if (!resetsAt) {
    return fallback;
  }
  const target = Date.parse(resetsAt);
  if (Number.isNaN(target)) {
    return fallback;
  }

  if (relative) {
    const diffMs = target - now;
    if (diffMs <= 0) return t("TrayResetsDueNow");
    const totalMinutes = Math.floor(diffMs / 60_000);
    const days = Math.floor(totalMinutes / 1440);
    const hours = Math.floor((totalMinutes % 1440) / 60);
    const minutes = totalMinutes % 60;
    if (days > 0) {
      return t("ResetsInDaysHours")
        .replace("{}", String(days))
        .replace("{}", String(hours));
    }
    return t("ResetsInHoursMinutes")
      .replace("{}", String(hours))
      .replace("{}", String(minutes));
  }

  try {
    return new Intl.DateTimeFormat(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    }).format(new Date(target));
  } catch {
    return fallback;
  }
}
