export const USAGE_ITEM_METRIC_PREFIX = "metric:";

/** Stable bridge ID for a metric or extra usage row. */
export function usageItemId(rawMetricId: string): string {
  return `${USAGE_ITEM_METRIC_PREFIX}${rawMetricId}`;
}

export function isUsageItemVisible(
  hiddenUsageItemIds: readonly string[] | undefined,
  rawMetricId: string,
): boolean {
  return !hiddenUsageItemIds?.includes(usageItemId(rawMetricId));
}
