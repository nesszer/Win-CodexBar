import type { CostSnapshotBridge, CostSummaryDisplayStyle } from "../types/bridge";
import type { LocaleKey } from "../i18n/keys";
import { localizeProviderText } from "./providerText";

type Translate = (key: LocaleKey) => string;

/**
 * The card's cost block, following upstream `MenuCardView+Costs.swift`:
 * a capped spend is "Extra usage" (or "Quota usage") with "period: used / limit"
 * and "N% used"; metered spend is "API spend"; a balance is one inline line.
 */
export interface CardCostSection {
  title: string;
  spendLine: string;
  /** 0..100 for the bar; null when the block has no cap. */
  percentUsed: number | null;
  percentLine: string | null;
  balanceLine: string | null;
  /** Title and value on one line, with no bar. */
  inline: boolean;
}

const currencyFormatters = new Map<string, Intl.NumberFormat>();

export function formatCostAmount(amount: number, code: string): string {
  try {
    let formatter = currencyFormatters.get(code);
    if (!formatter) {
      formatter = new Intl.NumberFormat("en-US", { style: "currency", currency: code });
      currencyFormatters.set(code, formatter);
    }
    return formatter.format(amount);
  } catch {
    return `${code} ${amount.toFixed(2)}`;
  }
}

function clampPercent(value: number): number {
  return Math.min(100, Math.max(0, value));
}

/** Mac `localizedPeriodLabel`: the common English periods map to locale strings. */
function periodLabel(raw: string | null | undefined, fallback: string, t: Translate): string {
  const trimmed = raw?.trim() ?? "";
  if (!trimmed) return fallback;
  switch (trimmed.toLowerCase()) {
    case "last 30 days":
      return t("CostPeriodLastDays").replace("{}", "30");
    case "this month":
      return t("CostPeriodThisMonth");
    case "today":
      return t("PanelToday");
    default:
      return localizeProviderText(trimmed, t) || trimmed;
  }
}

const creditsFormatter = new Intl.NumberFormat("en-US", { maximumFractionDigits: 2 });

/** Mac `UsageFormatter.creditsNumberString`: a unitless credit count, max 2 decimals. */
export function formatCreditsNumber(value: number): string {
  return creditsFormatter.format(value);
}

/** Codex costs are credits; upstream `creditsUsageSection` prints them unitless. */
const CREDIT_COST_PROVIDERS = new Set(["codex"]);

/** A cost whose period names a balance carries the balance in `used`. */
function periodNamesBalance(period: string): boolean {
  const normalized = period.trim().toLowerCase();
  return normalized === "credits" || normalized.endsWith("balance");
}

export function cardCostSection(
  cost: CostSnapshotBridge | null | undefined,
  style: CostSummaryDisplayStyle | undefined,
  t: Translate,
  providerId?: string,
): CardCostSection | null {
  if (!cost) return null;
  // Pay-as-you-go spend is the provider's own usage, not an optional summary.
  const payAsYouGo = cost.payAsYouGo === true;
  if (style === "hidden" && cost.alwaysVisible !== true && !payAsYouGo) return null;

  const isQuota = cost.currencyCode === "Quota";
  const isCredits = providerId !== undefined && CREDIT_COST_PROVIDERS.has(providerId);
  const money = (value: number, formatted?: string | null) =>
    isQuota
      ? value.toFixed(0)
      : isCredits
        ? formatCreditsNumber(value)
        : formatted || formatCostAmount(value, cost.currencyCode);
  const balanceText = (value: number) =>
    `${t("DetailCostBalance")}: ${money(value, cost.formattedBalance)}`;
  const limit = cost.limit ?? 0;
  // A provider-declared title (upstream `ProviderCostMenuCardStyle`) wins.
  const title = (fallback: LocaleKey) => t(cost.titleKey ?? fallback);

  if (payAsYouGo) {
    return {
      title: title("PayAsYouGoTitle"),
      spendLine: `${periodLabel(cost.period, t("CostPeriodThisMonth"), t)}: ${money(cost.used, cost.formattedUsed)}`,
      percentUsed: null,
      percentLine: null,
      balanceLine: cost.balance != null ? balanceText(cost.balance) : null,
      inline: false,
    };
  }

  if (limit > 0) {
    const percentUsed = clampPercent((cost.used / limit) * 100);
    // A folded cap is the primary meter's bar, so only the caption remains.
    const folded = cost.foldedIntoPrimary === true;
    return {
      title: title(isQuota ? "QuotaUsageTitle" : "ExtraUsage"),
      spendLine: `${periodLabel(cost.period, t("CostPeriodThisMonth"), t)}: ${money(cost.used, cost.formattedUsed)} / ${money(limit, cost.formattedLimit)}`,
      percentUsed: folded ? null : percentUsed,
      percentLine: folded ? null : t("UsedPercent").replace(/\{[^}]*\}/, percentUsed.toFixed(0)),
      balanceLine:
        style !== "compact" && cost.balance != null ? balanceText(cost.balance) : null,
      inline: false,
    };
  }

  if (cost.alwaysVisible === true) {
    return {
      title: title("ApiSpendTitle"),
      spendLine: `${periodLabel(cost.period, t("CostPeriodLastDays").replace("{}", "30"), t)}: ${money(cost.used, cost.formattedUsed)}`,
      percentUsed: null,
      percentLine: null,
      balanceLine: null,
      inline: false,
    };
  }

  const inline = (spendLine: string): CardCostSection => ({
    title: title("ExtraUsage"),
    spendLine,
    percentUsed: null,
    percentLine: null,
    balanceLine: null,
    inline: true,
  });
  if (cost.balance != null) return inline(balanceText(cost.balance));
  if (periodNamesBalance(cost.period)) {
    return inline(`${t("DetailCostBalance")}: ${money(cost.used, cost.formattedUsed)}`);
  }
  return inline(
    `${periodLabel(cost.period, t("CostPeriodThisMonth"), t)}: ${money(cost.used, cost.formattedUsed)}`,
  );
}
