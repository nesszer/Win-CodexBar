import type {
  CostSummaryDisplayStyle,
  ProviderChartData,
  ProviderThroughputSnapshot,
  ProviderUsageSnapshot,
} from "../../types/bridge";
import { formatCostAmount } from "../../lib/cardCost";
import type { MetricEntry } from "./CardMetricRow";

export interface MenuCardPresence {
  hasMetrics: boolean;
  /** Inventory rows or provider key-value details. */
  hasDetailSections: boolean;
  /**
   * Draw the detail sections: always on the full card, and on the compact
   * Overview only when it has no metric rows (upstream
   * `showsOverviewSupplementalContent`: `!compact || metrics.isEmpty`).
   */
  showsDetailSections: boolean;
  hasCost: boolean;
  localUsage: ProviderChartData["localUsage"] | null;
  wayfinderUsage: ProviderUsageSnapshot["wayfinderUsage"] | null;
  /** Last-hour Tokens/min grid (OpenAI API Live Usage); full card only. */
  openAiThroughput: ProviderThroughputSnapshot | null;
  hasDetails: boolean;
  /**
   * Plan renewal / expiry line under the quota rows. The compact Overview
   * drops it when quota bars are present (upstream 0.75.0 #4484).
   */
  planDateNote: string | null;
}

/**
 * A cost that carries only a balance (no cap, not a spend summary) adds
 * nothing when a provider detail row already shows that balance. Upstream
 * has no cost block for such providers, so the card keeps the detail row.
 */
function balanceRepeatsDetail(provider: ProviderUsageSnapshot): boolean {
  const cost = provider.cost;
  // A provider that styles its cost block (upstream `ProviderCostMenuCardStyle`,
  // such as LithosAI's "Credits") keeps it beside the matching detail row.
  if (cost?.titleKey) return false;
  if (!cost || cost.balance == null || (cost.limit ?? 0) > 0 || cost.alwaysVisible === true) {
    return false;
  }
  const balance = cost.formattedBalance || formatCostAmount(cost.balance, cost.currencyCode);
  return (provider.displayDetails ?? []).some((detail) => detail.value.trim() === balance);
}

/**
 * Which card sections exist. A card with an error, or whose only content the
 * compact Overview drops, renders its header alone with no divider.
 */
export function describeCard(
  provider: ProviderUsageSnapshot,
  chartData: ProviderChartData | null,
  visibleMetrics: MetricEntry[],
  costSummaryDisplayStyle: CostSummaryDisplayStyle = "detailed",
  compactOverview = false,
  planNote: string | null = null,
): MenuCardPresence {
  const hasError = !!provider.error;
  const hasMetrics = !hasError && visibleMetrics.length > 0;
  const hasCost =
    !hasError &&
    !!provider.cost &&
    (costSummaryDisplayStyle !== "hidden" || provider.cost.alwaysVisible === true) &&
    !balanceRepeatsDetail(provider);
  const hasDetailSections =
    !hasError &&
    ((provider.inventory?.length ?? 0) > 0 || (provider.displayDetails?.length ?? 0) > 0);
  const showsDetailSections = hasDetailSections && (!compactOverview || !hasMetrics);
  const localUsage = hasError ? null : chartData?.localUsage ?? null;
  const wayfinderUsage =
    !hasError && provider.providerId === "wayfinder" ? provider.wayfinderUsage ?? null : null;
  const planDateNote = !hasError && planNote && (!compactOverview || !hasMetrics) ? planNote : null;
  // The Tokens/min grid is a full-card section; the compact Overview leaves it out.
  const openAiThroughput =
    !hasError && !compactOverview && provider.providerId === "openaiapi"
      ? provider.throughput ?? null
      : null;
  const hasDetails = compactOverview
    ? hasMetrics || showsDetailSections || !!wayfinderUsage || !!planDateNote
    : hasMetrics ||
      hasDetailSections ||
      hasCost ||
      !!localUsage ||
      !!wayfinderUsage ||
      !!openAiThroughput ||
      !!planDateNote;
  return {
    hasMetrics,
    hasDetailSections,
    showsDetailSections,
    hasCost,
    localUsage,
    wayfinderUsage,
    openAiThroughput,
    hasDetails,
    planDateNote,
  };
}
