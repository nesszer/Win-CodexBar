import { useEffect, useState } from "react";
import type { MonthlyLimitBlockSnapshot } from "../types/bridge";
import { monthlyLimitBlockResetMs } from "../lib/monthlyLimitBlock";

// Browsers clamp timers above 2^31-1 ms (about 24.8 days); re-arm before that.
const MAX_WAIT_MS = 6 * 60 * 60 * 1000;

/**
 * Current time for monthly-limit block checks. Schedules a re-render when the
 * earliest future block reset passes, so a cached snapshot unblocks on time
 * instead of waiting for the next refresh.
 */
export function useMonthlyLimitBlockNow(
  blocks: readonly (MonthlyLimitBlockSnapshot | null | undefined)[],
): number {
  const [tick, setTick] = useState(0);
  const now = Date.now();
  let nextReset: number | null = null;
  for (const block of blocks) {
    const reset = monthlyLimitBlockResetMs(block);
    if (reset !== null && reset > now && (nextReset === null || reset < nextReset)) {
      nextReset = reset;
    }
  }

  useEffect(() => {
    if (nextReset === null) return;
    const wait = Math.min(Math.max(nextReset - Date.now(), 0), MAX_WAIT_MS);
    const id = window.setTimeout(() => setTick((value) => value + 1), wait);
    return () => window.clearTimeout(id);
  }, [nextReset, tick]);

  return now;
}
