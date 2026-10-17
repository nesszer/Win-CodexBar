import type {
  CostSnapshotBridge,
  ProviderInventoryItem,
  SubscriptionMetadataSnapshot,
} from "../types/bridge";

export const USAGE_ITEM_METRIC_PREFIX = "metric:";
export const USAGE_ITEM_DETAIL_SECTION_PREFIX = "detailSection:";

/** Non-metric card sections with their own choice (upstream 0.75.0 #4484). */
export const CLAUDE_RESET_CREDITS_ITEM_ID = "section:claude-reset-credits";
export const CLAUDE_EXTRA_USAGE_ITEM_ID = "section:claude-extra-usage";
export const SUBSCRIPTION_ITEM_ID = "section:subscription";
const CLAUDE_RESET_CREDITS_INVENTORY_ID = "reset-credits";

/** Stable bridge ID for a metric or extra usage row. */
export function usageItemId(rawMetricId: string): string {
  return `${USAGE_ITEM_METRIC_PREFIX}${rawMetricId}`;
}

/** Stable bridge ID for a provider detail section (upstream 0.62.0 #3638). */
export function detailSectionItemId(rawSectionTitle: string): string {
  return `${USAGE_ITEM_DETAIL_SECTION_PREFIX}${rawSectionTitle}`;
}

export function isUsageItemVisible(
  hiddenUsageItemIds: readonly string[] | undefined,
  rawMetricId: string,
): boolean {
  return !hiddenUsageItemIds?.includes(usageItemId(rawMetricId));
}

/** A detail section is visible unless its title-based ID is hidden. */
export function isDetailSectionVisible(
  hiddenUsageItemIds: readonly string[] | undefined,
  rawSectionTitle: string,
): boolean {
  return !hiddenUsageItemIds?.includes(detailSectionItemId(rawSectionTitle));
}

interface ClaudeSections {
  inventory?: ProviderInventoryItem[];
  cost: CostSnapshotBridge | null;
  subscription?: SubscriptionMetadataSnapshot | null;
  sourceLabel: string | null;
  hiddenUsageItemIds?: string[];
}

/**
 * Claude's Extra usage / prepaid balance block. Admin API spend is a
 * separate cost that this choice never hides (mirrors the backend rule).
 */
function isClaudeExtraUsageCost(value: ClaudeSections): boolean {
  return !!value.cost && value.cost.alwaysVisible !== true && value.sourceLabel !== "admin-api";
}

/**
 * Drop the Claude sections the user hid: saved reset credits, Extra usage /
 * prepaid Credits, and plan dates. Provider-specific by design; every other
 * provider passes through unchanged.
 */
export function projectClaudeSections<T extends ClaudeSections>(providerId: string, value: T): T {
  if (providerId !== "claude") return value;
  const hidden = value.hiddenUsageItemIds ?? [];
  let projected = value;
  if (hidden.includes(CLAUDE_RESET_CREDITS_ITEM_ID) && value.inventory?.length) {
    projected = {
      ...projected,
      inventory: value.inventory.filter((item) => item.id !== CLAUDE_RESET_CREDITS_INVENTORY_ID),
    };
  }
  if (hidden.includes(CLAUDE_EXTRA_USAGE_ITEM_ID) && isClaudeExtraUsageCost(value)) {
    projected = { ...projected, cost: null };
  }
  if (hidden.includes(SUBSCRIPTION_ITEM_ID) && value.subscription) {
    projected = { ...projected, subscription: null };
  }
  return projected;
}

type Translate = (key: "PlanRenewsOn" | "PlanExpiresOn" | "IntlLocale") => string;

function formatPlanDate(raw: string, dateOnly: boolean, locale: string): string | null {
  const date = new Date(raw);
  if (Number.isNaN(date.getTime())) return null;
  const options: Intl.DateTimeFormatOptions = {
    month: "short",
    day: "numeric",
    year: "numeric",
    // A calendar-only date is stored as UTC midnight; local time could move it a day.
    ...(dateOnly ? { timeZone: "UTC" } : {}),
  };
  try {
    return new Intl.DateTimeFormat(locale, options).format(date);
  } catch {
    return new Intl.DateTimeFormat("en-US", options).format(date);
  }
}

/**
 * Upstream `subscriptionMetadataNotes`: one "Renews: <date>" line, else
 * "Plan expires: <date>". Claude only; it sits under the quota rows.
 */
export function claudePlanDateNote(
  providerId: string,
  subscription: SubscriptionMetadataSnapshot | null | undefined,
  t: Translate,
): string | null {
  if (providerId !== "claude" || !subscription) return null;
  const locale = t("IntlLocale");
  if (subscription.renewsAt) {
    const date = formatPlanDate(subscription.renewsAt, subscription.renewsAtIsDateOnly === true, locale);
    if (date) return t("PlanRenewsOn").replace("{}", date);
  }
  if (subscription.expiresAt) {
    const date = formatPlanDate(subscription.expiresAt, subscription.expiresAtIsDateOnly === true, locale);
    if (date) return t("PlanExpiresOn").replace("{}", date);
  }
  return null;
}
