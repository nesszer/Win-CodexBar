import type { LocaleKey } from "../i18n/keys";
import type {
  DailyCostPoint,
  DailyTokenPoint,
  ProviderLocalUsageSummary,
  QuotaWindowHistoryPoint,
} from "../types/bridge";
import { parseCostPeriod, periodCostLabel, periodTokensLabel } from "./costPeriod";
import { formatCostAmount } from "./cardCost";

type Translate = (key: LocaleKey) => string;

/**
 * The card's inline usage dashboard, following upstream
 * `InlineUsageDashboardContent.swift`: a KPI grid, a 58 px cost histogram,
 * the recent quota windows, and short detail lines.
 */
export interface DashboardKpi {
  title: string;
  value: string;
  emphasis: boolean;
}

export interface DashboardPoint {
  date: string;
  value: number | null;
  hasUnpricedUsage: boolean;
  title: string;
}

export interface DashboardWindowRow {
  id: string;
  title: string;
  range: string;
  value: string;
  note: string | null;
}

export interface InlineDashboardModel {
  kpis: DashboardKpi[];
  points: DashboardPoint[];
  maxValue: number;
  scaleLabel: string | null;
  windows: DashboardWindowRow[];
  detailLines: string[];
}

export interface InlineDashboardInput {
  providerId: string;
  summary: ProviderLocalUsageSummary | null | undefined;
  costHistory: DailyCostPoint[];
  tokensHistory: DailyTokenPoint[];
  quotaHistory: QuotaWindowHistoryPoint[] | null | undefined;
  t: Translate;
  /** Formatting overrides, for deterministic tests. */
  locale?: string;
  timeZone?: string;
  now?: Date;
}

/** Upstream shows at most four quota windows (`weekCount: 4`). */
const MAX_WINDOWS = 4;
/** The histogram stays a fixed 30 days, whatever the History window is. */
const CHART_DAYS = 30;

const compactCount0 = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 0 });
const compactCount1 = new Intl.NumberFormat("en-US", { notation: "compact", maximumFractionDigits: 1 });

function compactCount(value: number | null | undefined): string {
  if (value == null || value <= 0) return "—";
  return (value >= 1_000_000 ? compactCount1 : compactCount0).format(value);
}

function compactCurrency(value: number): string {
  try {
    return new Intl.NumberFormat("en-US", {
      style: "currency",
      currency: "USD",
      notation: "compact",
      minimumFractionDigits: 0,
      maximumFractionDigits: 1,
    }).format(value);
  } catch {
    return formatCostAmount(value, "USD");
  }
}

function fill(template: string, value: string): string {
  return template.replace(/\{[^}]*\}/, value);
}

/** Upstream `quotaMetricValue`: an incomplete subtotal reads as a lower bound. */
function boundValue(value: string | null, complete: boolean): string {
  if (value == null) return "—";
  return complete ? value : `≥ ${value}`;
}

export function shortModelName(name: string): string {
  const trimmed = name.trim();
  return trimmed.length > 26 ? `${trimmed.slice(0, 25)}…` : trimmed;
}

/** Day count of the History window; month-to-date counts the days so far. */
function historyDays(reportingPeriod: string, now: Date): number {
  const parsed = parseCostPeriod(reportingPeriod) ?? { kind: "rolling", days: 30 };
  switch (parsed.kind) {
    case "rolling":
      return parsed.days;
    case "monthToDate":
      return now.getDate();
    case "all":
      return Number.POSITIVE_INFINITY;
  }
}

function rangeLabel(
  start: string,
  end: string,
  locale: string | undefined,
  timeZone: string | undefined,
): string {
  const startDate = new Date(start);
  const endDate = new Date(end);
  if (Number.isNaN(startDate.getTime()) || Number.isNaN(endDate.getTime())) {
    return `${start} – ${end}`;
  }
  const dayKey = new Intl.DateTimeFormat("en-CA", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  });
  const sameDay = dayKey.format(startDate) === dayKey.format(endDate);
  const sameYear = dayKey.format(startDate).slice(0, 4) === dayKey.format(endDate).slice(0, 4);
  const time: Intl.DateTimeFormatOptions = { timeZone, hour: "numeric", minute: "2-digit" };
  const dated = new Intl.DateTimeFormat(locale, {
    ...time,
    month: "short",
    day: "numeric",
    year: sameYear ? undefined : "numeric",
  });
  const endText = sameDay
    ? new Intl.DateTimeFormat(locale, time).format(endDate)
    : dated.format(endDate);
  return `${dated.format(startDate)} – ${endText}`;
}

function windowTitle(offset: number, t: Translate): string {
  if (offset === 0) return t("PanelCurrentWindow");
  if (offset === 1) return t("PanelPreviousWindow");
  return fill(t("PanelWindowsAgo"), String(offset));
}

function windowRow(
  week: QuotaWindowHistoryPoint,
  input: InlineDashboardInput,
): DashboardWindowRow {
  const { t } = input;
  const cost = boundValue(
    week.totalCostUsd == null ? null : formatCostAmount(week.totalCostUsd, "USD"),
    week.costIsComplete,
  );
  const value =
    week.totalTokens != null
      ? `${cost} · ${boundValue(compactCount(week.totalTokens), week.tokensAreComplete)}`
      : cost;
  const range = rangeLabel(week.start, week.end, input.locale, input.timeZone);
  const partial =
    (!week.costIsComplete && week.totalCostUsd != null) ||
    (!week.tokensAreComplete && week.totalTokens != null);
  return {
    id: `${week.offset}-${week.start}`,
    title: windowTitle(week.offset, t),
    range: week.boundariesAreEstimated ? fill(t("PanelEstimatedValue"), range) : range,
    value,
    note: partial ? t("PanelPartialEstimate") : null,
  };
}

function chartPoints(input: InlineDashboardInput): DashboardPoint[] {
  const tokensByDate = new Map(input.tokensHistory.map((point) => [point.date, point.tokens]));
  return input.costHistory.slice(-CHART_DAYS).map((point) => {
    const tokens = tokensByDate.get(point.date);
    const hasUnpricedUsage = point.value == null && tokens != null && tokens > 0;
    const cost = hasUnpricedUsage ? input.t("UsageSpendUnpriced")
      : point.value == null ? "—" : formatCostAmount(point.value, "USD");
    const title =
      point.value == null && tokens == null
        ? point.date
        : `${point.date}: ${cost} · ${
            tokens == null ? "—" : `${compactCount(tokens)} ${input.t("UsageSpendTokens")}`
          }`;
    return { date: point.date, value: point.value, hasUnpricedUsage, title };
  });
}

export function inlineDashboardModel(input: InlineDashboardInput): InlineDashboardModel | null {
  const { summary, t } = input;
  if (!summary) return null;

  const incompleteCount = summary.incompleteRequestCount ?? 0;
  const incompleteSuffix = incompleteCount > 0 ? ` · ${t("IncompleteRequestsLabel")}` : "";
  const money = (value: number | null) => (value == null ? "—" : formatCostAmount(value, "USD"));
  const period = summary.reportingPeriod;
  const periodTokens = {
    title: periodTokensLabel(period, t),
    value:
      compactCount(summary.periodTokens) + (summary.periodTokens != null ? incompleteSuffix : ""),
    emphasis: false,
  };

  const detailLines: string[] = [];
  if (incompleteCount > 0) {
    detailLines.push(
      `${t("IncompleteRequestsLabel")} · ${fill(t("IncompleteRequestsDetail"), String(incompleteCount))}`,
    );
  }
  if (summary.topModel) {
    detailLines.push(`${t("PanelTopModelPrefix")}: ${shortModelName(summary.topModel)}`);
  }
  if (summary.estimateNote) {
    detailLines.push(
      summary.estimateNote === "Estimated from local logs"
        ? t("PanelEstimatedFromLocalLogs")
        : summary.estimateNote,
    );
  }

  const points = chartPoints(input);
  const maxValue = Math.max(0, ...points.flatMap((point) => (point.value == null ? [] : [point.value])));
  const scaleLabel = points.length > 0 && maxValue > 0 ? compactCurrency(maxValue) : null;

  // Tokens-only providers (Muse) have no cost; upstream `tokenHistoryInlineDashboard`.
  if (input.providerId === "muse") {
    return {
      kpis: [
        {
          title: t("PanelLatest"),
          value:
            summary.latestTokens != null && summary.latestTokens > 0
              ? fill(t("PanelPeriodTokens"), compactCount(summary.latestTokens))
              : "—",
          emphasis: true,
        },
        periodTokens,
      ],
      points: [],
      maxValue: 0,
      scaleLabel: null,
      windows: [],
      detailLines,
    };
  }

  const windows = (input.quotaHistory ?? [])
    .filter((week) => week.offset < MAX_WINDOWS)
    .filter((week) => week.offset === 0 || week.entryCount > 0)
    .sort((a, b) => a.offset - b.offset)
    .map((week) => windowRow(week, input));

  const today: DashboardKpi = { title: t("PanelToday"), value: money(summary.todayCost), emphasis: true };
  const periodCost: DashboardKpi = {
    title: periodCostLabel(period, t),
    value: money(summary.periodCost) + (summary.periodCost != null ? incompleteSuffix : ""),
    emphasis: false,
  };
  const latestTokens: DashboardKpi = {
    title: t("PanelLatestTokens"),
    value: compactCount(summary.latestTokens),
    emphasis: false,
  };

  const current = input.quotaHistory?.find((week) => week.offset === 0);
  const days = historyDays(period, input.now ?? new Date());
  if (current && days >= 7) {
    const estimated = current.boundariesAreEstimated;
    const titled = (text: string) => (estimated ? fill(t("PanelEstimatedValue"), text) : text);
    const windowCost: DashboardKpi = {
      title: titled(t("PanelCurrentWindow")),
      value: boundValue(
        current.totalCostUsd == null ? null : formatCostAmount(current.totalCostUsd, "USD"),
        current.costIsComplete,
      ),
      emphasis: true,
    };
    const windowTokens: DashboardKpi = {
      title: titled(t("PanelCurrentWindowTokens")),
      value: boundValue(
        current.totalTokens == null ? null : compactCount(current.totalTokens),
        current.tokensAreComplete,
      ),
      emphasis: false,
    };
    // A 7-day history is the window itself: relabel instead of repeating it.
    const kpis =
      days > 7
        ? [today, windowCost, latestTokens, windowTokens, periodCost, periodTokens]
        : [today, windowCost, latestTokens, windowTokens];
    return { kpis, points, maxValue, scaleLabel, windows, detailLines };
  }

  return {
    kpis: [today, periodCost, latestTokens, periodTokens],
    points,
    maxValue,
    scaleLabel,
    windows,
    detailLines,
  };
}
