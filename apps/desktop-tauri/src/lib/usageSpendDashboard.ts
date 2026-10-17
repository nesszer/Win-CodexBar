import type { SpendContract, UsageSpendRow, UsageSpendSummary } from "../types/bridge";

/**
 * Settings > Usage & Spend dashboard helpers. Hidden sources are a view
 * filter: the backend still returns every row, and the pane, Copy JSON,
 * Export JSON and Share Stats all read the summary this module filters.
 */

/** Period segments in the order the macOS pane shows them. */
export const SPEND_DASHBOARD_PERIODS = ["rolling:7", "rolling:30", "rolling:90", "month-to-date", "all"] as const;

/** Mirrors `normalize_spend_dashboard_hidden_source_ids`: trimmed, lowercase, non-empty, sorted, unique. */
export function normalizeHiddenSourceIds(ids: Iterable<string>): string[] {
  const normalized = new Set<string>();
  for (const id of ids) {
    const value = id.trim().toLowerCase();
    if (value) normalized.add(value);
  }
  return [...normalized].sort();
}

/** The hidden list after showing (`visible`) or hiding one source. */
export function toggleHiddenSource(hidden: readonly string[], sourceId: string, visible: boolean): string[] {
  const id = sourceId.trim().toLowerCase();
  const rest = hidden.filter((value) => value.trim().toLowerCase() !== id);
  return normalizeHiddenSourceIds(visible ? rest : [...rest, id]);
}

export interface SpendSource {
  id: string;
  displayName: string;
}

/**
 * Every source the Sources filter lists: each row the backend returned, plus
 * hidden ids that have no row this scan, so a hidden source can always be
 * shown again.
 */
export function spendSources(summary: UsageSpendSummary | null, hidden: readonly string[]): SpendSource[] {
  const sources = new Map<string, SpendSource>();
  for (const row of summary?.rows ?? []) {
    const id = row.providerId.trim().toLowerCase();
    if (id && !sources.has(id)) sources.set(id, { id, displayName: row.displayName });
  }
  for (const id of normalizeHiddenSourceIds(hidden)) {
    if (!sources.has(id)) sources.set(id, { id, displayName: id });
  }
  return [...sources.values()].sort((a, b) => a.displayName.localeCompare(b.displayName));
}

/** TS mirror of `SpendContract::empty`: nothing scanned for this provider and window. */
export function emptySpendContract(providerId: string, contract: SpendContract): SpendContract {
  return {
    providerId,
    historyDays: contract.historyDays,
    reportingPeriod: contract.reportingPeriod,
    knownCostUsd: null,
    knownZero: false,
    provenance: "unknown",
    priceCoverage: { priced: 0, unpriced: 0, unmetered: 0, estimated: 0 },
    priceCoverageRatio: null,
    historyCoverageEstablished: false,
    tokenMix: {
      inputTokens: null,
      outputTokens: null,
      cacheReadTokens: null,
      cacheCreationTokens: null,
      reasoningTokens: null,
    },
    conversationCount: 0,
    models: [],
    projects: [],
    conversations: [],
    daily: [],
    hourlyActivity: [],
    projectSourceStatus: null,
    customPricingActive: false,
    imports: [],
  };
}

/** True when the detail contract's provider is one of the hidden sources. */
export function isContractHidden(summary: UsageSpendSummary, hidden: readonly string[]): boolean {
  const ids = new Set(normalizeHiddenSourceIds(hidden));
  return ids.has(summary.contract.providerId.trim().toLowerCase());
}

/**
 * The summary the pane shows and exports: hidden rows are dropped, and when
 * the detail contract belongs to a hidden source it is replaced by an empty
 * one so its tokens, models, projects and sessions do not leak.
 */
export function visibleUsageSpendSummary(summary: UsageSpendSummary, hidden: readonly string[]): UsageSpendSummary {
  const ids = new Set(normalizeHiddenSourceIds(hidden));
  if (ids.size === 0) return summary;
  const rows = summary.rows.filter((row) => !ids.has(row.providerId.trim().toLowerCase()));
  const contract = isContractHidden(summary, hidden)
    ? emptySpendContract(summary.contract.providerId, summary.contract)
    : summary.contract;
  const shown = (sourceId: string) => !ids.has(sourceId.trim().toLowerCase());
  const trend = summary.trend && {
    ...summary.trend,
    sources: summary.trend.sources.filter((source) => shown(source.id)),
    daily: summary.trend.daily.filter((point) => shown(point.sourceId)),
    hourly: summary.trend.hourly.filter((point) => shown(point.sourceId)),
  };
  return { ...summary, rows, contract, trend };
}

export interface CurrencyTotals {
  currency: string;
  /** Sum of the known period costs; null when no row in this currency has one. */
  cost: number | null;
  /** Sum of the known period tokens; null when no row in this currency has a count. */
  tokens: number | null;
  /** Rows (sources) reporting in this currency. */
  sources: number;
}

/** Stat-strip totals over the selected window, one entry per currency (USD first). */
export function spendTotalsByCurrency(rows: readonly UsageSpendRow[]): CurrencyTotals[] {
  const byCurrency = new Map<string, CurrencyTotals>();
  for (const row of rows) {
    const currency = (row.currency || "USD").toUpperCase();
    const totals = byCurrency.get(currency) ?? { currency, cost: null, tokens: null, sources: 0 };
    totals.sources += 1;
    if (row.periodCost != null && Number.isFinite(row.periodCost)) {
      totals.cost = (totals.cost ?? 0) + row.periodCost;
    }
    if (row.periodTokens != null && Number.isFinite(row.periodTokens) && row.periodTokens >= 0) {
      totals.tokens = (totals.tokens ?? 0) + row.periodTokens;
    }
    byCurrency.set(currency, totals);
  }
  return [...byCurrency.values()].sort((a, b) => {
    if (a.currency === "USD") return -1;
    if (b.currency === "USD") return 1;
    return a.currency.localeCompare(b.currency);
  });
}

const compactFormatter = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

/** "20M", "106K", "—" for unknown. Matches the macOS pane's compact token figures. */
export function formatCompactTokens(value: number | null | undefined): string {
  if (value == null || !Number.isFinite(value) || value < 0) return "—";
  return compactFormatter.format(value);
}
