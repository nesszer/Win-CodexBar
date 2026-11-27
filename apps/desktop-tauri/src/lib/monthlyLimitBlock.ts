import type { MonthlyLimitBlockSnapshot } from "../types/bridge";

/** Reset of a block in epoch ms; `null` when unknown or unparseable. */
export function monthlyLimitBlockResetMs(
  block: MonthlyLimitBlockSnapshot | null | undefined,
): number | null {
  if (!block?.resetsAt) return null;
  const reset = Date.parse(block.resetsAt);
  return Number.isNaN(reset) ? null : reset;
}

/**
 * Whether a longer exhausted pool still blocks a window at `now` (upstream
 * 0.69.0 #4091). A block without a known reset holds, as upstream does for an
 * exhausted pool with no reset; a known reset lifts it once reached.
 */
export function isMonthlyLimitBlockActive(
  block: MonthlyLimitBlockSnapshot | null | undefined,
  now: number,
): boolean {
  if (!block) return false;
  const reset = monthlyLimitBlockResetMs(block);
  return reset === null || reset > now;
}
