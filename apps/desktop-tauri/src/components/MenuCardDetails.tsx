import { useEffect, useState } from "react";
import type {
  CostSummaryDisplayStyle,
  DailyCostPoint,
  DailyTokenPoint,
  PaceSnapshot,
  ProviderInventoryItem,
  OpenAiApiUsageSnapshot,
  ProviderChartData,
  ProviderLocalUsageSummary,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
  SessionEquivalentForecastSnapshot,
} from "../types/bridge";
import { useLocale } from "../hooks/useLocale";
import { providerAllowsPace } from "../lib/providerPace";
import { providerShowsDailyApiUsage } from "../lib/providerCharts";
import {
  useFormattedResetTime,
  type ResetTimeFormatMode,
} from "../hooks/useFormattedResetTime";
import { formatEta } from "../lib/formatEta";
import type { LocaleKey } from "../i18n/keys";
import { paceCategory } from "../surfaces/tray/paceCategory";
import { SimpleBarChart, StackedBarChart } from "./MiniBarChart";
import { InventoryItemRow } from "./InventoryRows";
import {
  groupProviderDisplayDetails,
  ProviderDisplayRow,
} from "./ProviderDisplayRow";
import { OpenAIApiUsageChart } from "./OpenAIApiUsageChart";
import { QuotaWindowHistory } from "./QuotaWindowHistory";
import QuotaBurndownChart from "./QuotaBurndownChart";
import { getPaceBudget, type PaceBudget } from "../lib/paceBudget";
import { isMonthlyLimitBlockActive } from "../lib/monthlyLimitBlock";
import { periodCostLabel, periodTokensLabel } from "../lib/costPeriod";
import { providerCostPeriodTitle } from "../lib/providerLabels";
import { windowDetailText } from "../lib/usageWindows";
import { localizeProviderText } from "../lib/providerText";
import { localizeProviderLabel } from "../lib/windowLabels";
import { isDetailSectionVisible } from "../lib/usageItemVisibility";
import PaceDetailsChart from "./PaceDetailsChart";
import UsageProgressBar from "./UsageProgressBar";
import {
  metricResetText,
  metricRowPresentation,
  type MetricLane,
  type UsageThresholdSettings,
} from "../lib/metricRowModel";

/** Upstream session-quota estimate: "Estimated: {n} session quota(s) left". */
function formatSessionEquivalentEstimate(
  forecast: SessionEquivalentForecastSnapshot | null | undefined,
): string | null {
  if (!forecast) return null;
  const raw = forecast.estimatedWindowsToExhaustWeekly;
  if (!Number.isFinite(raw)) return null;
  const rounded = Math.round(Math.min(Math.max(raw, 0), 1_000_000) * 10) / 10;
  const display =
    Number.isInteger(rounded) || Math.abs(rounded - Math.round(rounded)) < 1e-9
      ? String(Math.round(rounded))
      : rounded.toFixed(1);
  const unit =
    rounded > 0 && rounded <= 1 ? "session quota" : "session quotas";
  return `Estimated: ${display} ${unit} left`;
}

const currencyFormatters = new Map<string, Intl.NumberFormat>();
const compactCountFormat0 = new Intl.NumberFormat("en-US", {
  notation: "compact",
  maximumFractionDigits: 0,
});
const compactCountFormat1 = new Intl.NumberFormat("en-US", {
  notation: "compact",
  maximumFractionDigits: 1,
});

function formatCurrency(amount: number, code: string): string {
  try {
    let formatter = currencyFormatters.get(code);
    if (!formatter) {
      formatter = new Intl.NumberFormat("en-US", {
        style: "currency",
        currency: code,
      });
      currencyFormatters.set(code, formatter);
    }
    return formatter.format(amount);
  } catch {
    return `${code} ${amount.toFixed(2)}`;
  }
}

function formatCompactCount(value: number | null): string {
  if (value == null || value <= 0) return "—";
  return (value >= 1_000_000 ? compactCountFormat1 : compactCountFormat0).format(
    value,
  );
}

function formatLocalUsagePointTitle(
  point: DailyCostPoint,
  tokenCount: number | undefined,
  tokenLabel: string,
): string {
  if (point.value == null && tokenCount == null) return point.date;
  const cost = point.value == null ? "—" : formatCurrency(point.value, "USD");
  const tokens = tokenCount == null ? "—" : `${formatCompactCount(tokenCount)} ${tokenLabel}`;
  return `${point.date}: ${cost} · ${tokens}`;
}

function formatBudget(value: number): string {
  return value < 10
    ? value.toFixed(1).replace(/\.0$/, "")
    : Math.round(value).toString();
}

function LocalUsageBlock({
  providerId,
  summary,
  costHistory,
  tokensHistory,
}: {
  providerId: string;
  summary: ProviderLocalUsageSummary;
  costHistory: DailyCostPoint[];
  tokensHistory: DailyTokenPoint[];
}) {
  const { t } = useLocale();
  // The selected History window; the histogram below stays a fixed 30 days.
  const { reportingPeriod, periodCost, periodTokens } = summary;

  const isCodex = providerId === "codex";
  const isMuse = providerId === "muse";
  const visibleHistory = costHistory.slice(-30);
  const maxCost = Math.max(
    ...visibleHistory.flatMap((point) => (point.value == null ? [] : [point.value])),
    0,
  );
  const tokensByDate = new Map(tokensHistory.map((point) => [point.date, point.tokens]));

  return (
    <section className="menu-card__group menu-card__local-usage">
      <div className="menu-card__local-grid">
        <div>
          <span className="menu-card__local-label">{t("PanelToday")}</span>
          <strong>
            {isMuse
              ? (summary.latestTokens != null
                ? formatCompactCount(summary.latestTokens)
                : "—")
              : summary.todayCost != null
              ? formatCurrency(summary.todayCost, "USD")
              : "—"}
          </strong>
        </div>
        {!isMuse && (
          <div>
            <span className="menu-card__local-label">
              {periodCostLabel(reportingPeriod, t)}
            </span>
            <strong>
              {periodCost != null ? formatCurrency(periodCost, "USD") : "—"}
            </strong>
          </div>
        )}
        <div>
          <span className="menu-card__local-label">
            {periodTokensLabel(reportingPeriod, t)}
          </span>
          <strong>{formatCompactCount(periodTokens)}</strong>
        </div>
        {!isMuse && (
          <div>
            <span className="menu-card__local-label">{t("PanelLatestTokens")}</span>
            <strong>{formatCompactCount(summary.latestTokens)}</strong>
          </div>
        )}
      </div>

      {isCodex && visibleHistory.length > 0 && (
        <div className="menu-card__local-chart" aria-label={t("PanelThirtyDayCostHistogram")}>
          {visibleHistory.map((point, index) => (
            <span
              key={`${point.date}-${index}`}
              style={{
                height: `${point.value == null || maxCost <= 0 ? 1 : Math.max(4, Math.round((point.value / maxCost) * 64))}px`,
                opacity: point.value == null ? 0 : undefined,
              }}
              title={formatLocalUsagePointTitle(
                point,
                tokensByDate.get(point.date),
                t("UsageSpendTokens"),
              )}
            />
          ))}
        </div>
      )}

      {summary.incompleteRequestCount != null && summary.incompleteRequestCount > 0 && (
        <div className="menu-card__local-note">
          <strong>{t("IncompleteRequestsLabel")}</strong>
          <span>
            {t("IncompleteRequestsDetail").replace("{}", String(summary.incompleteRequestCount))}
          </span>
        </div>
      )}

      <div className="menu-card__local-note">
        {summary.topModel && <strong>{t("PanelTopModelPrefix")}: {summary.topModel}</strong>}
        <span>
          {summary.estimateNote === "Estimated from local logs"
            ? t("PanelEstimatedFromLocalLogs")
            : summary.estimateNote}
        </span>
      </div>
    </section>
  );
}

function WayfinderUsageBlock({
  usage,
}: {
  usage: NonNullable<ProviderUsageSnapshot["wayfinderUsage"]>;
}) {
  const { t } = useLocale();
  const formatAmount = (value: number) =>
    usage.priced ? `${value.toFixed(4)} ${usage.unit.toUpperCase()}` : "—";

  return (
    <section className="menu-card__group">
      <div className="menu-card__local-grid">
        <div>
          <span className="menu-card__local-label">{t("WayfinderGatewayStatus")}</span>
          <strong>{usage.gatewayStatus}</strong>
        </div>
        <div>
          <span className="menu-card__local-label">{t("WayfinderModels")}</span>
          <strong>{usage.modelCount}</strong>
        </div>
        <div>
          <span className="menu-card__local-label">{t("WayfinderRequests")}</span>
          <strong>{formatCompactCount(usage.requests)}</strong>
        </div>
        <div>
          <span className="menu-card__local-label">{t("WayfinderTokens")}</span>
          <strong>{formatCompactCount(usage.tokens)}</strong>
        </div>
      </div>
      <div className="menu-card__cost-line">
        {t("WayfinderSaved")}: {formatAmount(usage.saved)} ({usage.savedPercent.toFixed(1)}%)
      </div>
      {(usage.offline || usage.dryRun || usage.missingKeys.length > 0) && (
        <div className="menu-card__local-note">
          {usage.offline && <span>{t("WayfinderOffline")}</span>}
          {usage.dryRun && <span>{t("WayfinderDryRun")}</span>}
          {usage.missingKeys.length > 0 && (
            <span>{t("WayfinderMissingKeys")}: {usage.missingKeys.join(", ")}</span>
          )}
        </div>
      )}
    </section>
  );
}

function paceStageKey(stage: PaceSnapshot["stage"]): LocaleKey {
  switch (stage) {
    case "on_track":
      return "DetailPaceOnTrack";
    case "slightly_ahead":
      return "DetailPaceSlightlyAhead";
    case "ahead":
      return "DetailPaceAhead";
    case "far_ahead":
      return "DetailPaceFarAhead";
    case "slightly_behind":
      return "DetailPaceSlightlyBehind";
    case "behind":
      return "DetailPaceBehind";
    case "far_behind":
      return "DetailPaceFarBehind";
    default:
      return "DetailPaceOnTrack";
  }
}

const WEEKLY_WINDOW_MINUTES = 7 * 24 * 60;

export interface MetricEntry {
  id: string;
  label: string;
  snap: RateWindowSnapshot;
  lane: MetricLane;
  resetFormatMode?: ResetTimeFormatMode;
  sessionEquivalentForecast?: SessionEquivalentForecastSnapshot | null;
}

function metricPaceBudget(snap: RateWindowSnapshot): PaceBudget | null {
  if (snap.isExhausted) return null;
  const isWeeklyWindow =
    snap.windowMinutes != null && snap.windowMinutes >= WEEKLY_WINDOW_MINUTES;
  return isWeeklyWindow ? getPaceBudget(snap) : null;
}

type MetricRowDisplay = {
  resetTimeRelative: boolean;
  showResetWhenExhausted?: boolean;
  showPace?: boolean;
  showAsUsed?: boolean;
  compactOverview?: boolean;
  costSummaryDisplayStyle?: CostSummaryDisplayStyle;
  monthlyLimitBlockNow?: number;
  usageThresholds?: UsageThresholdSettings | null;
  weeklyProgressWorkDays?: number | null;
};

function useCountdownNow(ticking: boolean): number {
  const [, setTick] = useState(0);
  useEffect(() => {
    if (!ticking) return;
    const id = window.setInterval(() => setTick((tick) => tick + 1), 30_000);
    return () => window.clearInterval(id);
  }, [ticking]);
  return Date.now();
}

function MetricRow({
  title,
  snap,
  lane,
  providerId,
  exhaustedLabel,
  display,
  expanded,
  onToggleExpanded,
  resetFormatMode,
  sessionEquivalentForecast,
}: {
  title: string;
  snap: RateWindowSnapshot;
  lane: MetricLane;
  providerId: string;
  exhaustedLabel: string;
  display: MetricRowDisplay;
  expanded: boolean;
  onToggleExpanded: () => void;
  resetFormatMode?: ResetTimeFormatMode;
  sessionEquivalentForecast?: SessionEquivalentForecastSnapshot | null;
}) {
  const { t } = useLocale();
  const {
    resetTimeRelative,
    showResetWhenExhausted = false,
    showPace = true,
    showAsUsed = false,
    compactOverview = false,
    monthlyLimitBlockNow,
    usageThresholds = null,
    weeklyProgressWorkDays = null,
  } = display;
  // Upstream 0.69.0 #4091: a longer exhausted pool (Kimi's monthly membership)
  // blocks this window until the pool resets. Raw percentages stay untouched.
  const blocked = isMonthlyLimitBlockActive(
    snap.monthlyLimitBlock,
    monthlyLimitBlockNow ?? Date.now(),
  );
  const isInformational = snap.isInformational === true;
  const informationalReset = useFormattedResetTime(
    isInformational && !blocked ? snap.resetsAt : null,
    null,
    resetTimeRelative,
    resetFormatMode ?? "reset",
  );
  const now = useCountdownNow(!isInformational && !blocked && snap.resetsAt != null);
  const detailText = localizeProviderText(windowDetailText(snap), t) || null;
  if (blocked) {
    // Upstream `MetricRow` status layout: title plus one secondary status line,
    // no bar, percent, reset, pace, or forecast. The pool's own row keeps its
    // reset; the shorter resets cannot restore access.
    return (
      <div className="menu-metric menu-metric--blocked">
        <span className="menu-metric__title">{title}</span>
        <span className="menu-metric__status">{t("PanelBlockedByMonthlyLimit")}</span>
      </div>
    );
  }
  if (isInformational) {
    const infoPrimary =
      localizeProviderText(snap.resetDescription?.trim(), t) || informationalReset || "—";
    return (
      <div className="menu-metric">
        <span className="menu-metric__title">{title}</span>
        <div className="menu-metric__row">
          <span className="menu-metric__pct">{infoPrimary}</span>
          {!compactOverview &&
            snap.resetDescription?.trim() &&
            informationalReset &&
            informationalReset !== infoPrimary && (
              <span className="menu-metric__reset">{informationalReset}</span>
            )}
        </div>
        {!compactOverview && detailText && (
          <div className="menu-metric__detail">{detailText}</div>
        )}
      </div>
    );
  }
  const row = metricRowPresentation(
    {
      snap,
      lane,
      providerId,
      resetText: metricResetText(snap, resetTimeRelative, now, t),
      compact: compactOverview,
      showAsUsed,
      showResetWhenExhausted,
      paceEnabled: showPace,
      usageThresholds,
      weeklyProgressWorkDays,
      now,
    },
    t,
  );
  const budget = showPace && !compactOverview ? metricPaceBudget(snap) : null;
  const forecastText = formatSessionEquivalentEstimate(sessionEquivalentForecast);
  return (
    <div className="menu-metric">
      <div className="menu-metric__head">
        <span className="menu-metric__title">
          {title}
          {row.percentText && (
            <>
              {" "}
              <span className="menu-metric__percent">{row.percentText}</span>
            </>
          )}
        </span>
        {row.resetText && <span className="menu-metric__reset">{row.resetText}</span>}
      </div>
      <UsageProgressBar bar={row.bar} label={title} />
      {row.metaText && <div className="menu-metric__meta">{row.metaText}</div>}
      {!compactOverview && detailText && (
        <div className="menu-metric__detail">{detailText}</div>
      )}
      {!compactOverview && snap.isExhausted && (
        <div className="menu-metric__exhausted">{exhaustedLabel}</div>
      )}
      {budget && (
        <div className="menu-metric__budget">
          <button
            type="button"
            className="menu-metric__budget-header"
            onClick={onToggleExpanded}
            aria-expanded={expanded}
          >
            <span>{t("PanelOnPaceBudget")}</span>
          </button>
          {expanded && <div className="menu-metric__budget-pills">
            {[
              [t("PanelNow"), budget.now],
              [t("PanelOneHour"), budget.nextHour],
              [t("PanelFiveHours"), budget.nextFiveHours],
              [t("PanelTodayBudget"), budget.today],
            ].map(([label, value]) => (
              <span className="menu-metric__budget-pill" key={String(label)}>
                {label} {formatBudget(Number(value))}%
              </span>
            ))}
          </div>}
          {expanded && <PaceDetailsChart snap={snap} t={t} />}
        </div>
      )}
      {!compactOverview && showPace && forecastText && (
        <div className="menu-metric__row menu-metric__forecast">
          <span className="menu-metric__pct">{forecastText}</span>
        </div>
      )}
    </div>
  );
}

export interface MenuCardPresence {
  hasMetrics: boolean;
  hasInventory: boolean;
  hasDisplayDetails: boolean;
  hasCost: boolean;
  hasPace: boolean;
  hasCharts: boolean;
  hasCostHistory: boolean;
  hasCreditsHistory: boolean;
  hasUsageBreakdown: boolean;
  hasQuotaWindowHistory: boolean;
  hasBurndown: boolean;
  localUsage: ProviderChartData["localUsage"] | null;
  wayfinderUsage: ProviderUsageSnapshot["wayfinderUsage"] | null;
  /** Per-day API usage history (OpenAI Admin API, Groq console); only with data. */
  openAiApiUsage: OpenAiApiUsageSnapshot | null;
  hasDetails: boolean;
}

export interface MenuCardDetailsProps {
  provider: ProviderUsageSnapshot;
  display: MetricRowDisplay;
  metrics: MetricEntry[];
  chartData: ProviderChartData | null;
  presence: MenuCardPresence;
  onLayoutChange?: () => void;
}

/**
 * Single source of truth for "does this card have a body" and which sections
 * are present. Pure; computed once in `MenuCard` and threaded into
 * `MenuCardDetails` so the two files never diverge on the predicate suite.
 */
export function describeCard(
  provider: ProviderUsageSnapshot,
  chartData: ProviderChartData | null,
  visibleMetrics: MetricEntry[],
  costSummaryDisplayStyle: CostSummaryDisplayStyle = "detailed",
  showPace = true,
  compactOverview = false,
  monthlyLimitBlockNow: number = Date.now(),
): MenuCardPresence {
  const hasCostHistory =
    chartData !== null && chartData.costHistory.some((point) => point.value != null);
  const hasCreditsHistory =
    chartData !== null && chartData.creditsHistory.length > 0;
  const hasUsageBreakdown =
    chartData !== null && chartData.usageBreakdown.length > 0;
  const hasQuotaWindowHistory =
    chartData !== null && (chartData.quotaWindowHistory?.windows.length ?? 0) > 0;
  const hasBurndown = provider.quotaBurndown != null;
  const hasCharts =
    hasCostHistory || hasCreditsHistory || hasUsageBreakdown || hasQuotaWindowHistory;
  const isWayfinder = provider.providerId === "wayfinder";
  const localUsage = provider.error ? null : chartData?.localUsage ?? null;
  const wayfinderUsage = isWayfinder ? provider.wayfinderUsage : null;
  const openAiApiUsage =
    providerShowsDailyApiUsage(provider.providerId) &&
    !provider.error &&
    (provider.openAiApiUsage?.daily.length ?? 0) > 0
      ? (provider.openAiApiUsage ?? null)
      : null;
  const hasMetrics = visibleMetrics.length > 0;
  const hasInventory = !provider.error && (provider.inventory?.length ?? 0) > 0;
  const hasDisplayDetails = !provider.error && (provider.displayDetails?.length ?? 0) > 0;
  const hasCost =
    !!provider.cost &&
    (costSummaryDisplayStyle !== "hidden" || provider.cost.alwaysVisible === true);
  const hasPace =
    showPace &&
    providerAllowsPace(provider.providerId, provider.sourceLabel) &&
    !!provider.pace &&
    !isMonthlyLimitBlockActive(provider.pace.monthlyLimitBlock, monthlyLimitBlockNow);
  const hasDetails =
    !provider.error &&
    (hasMetrics ||
      hasInventory ||
      hasDisplayDetails ||
      hasCost ||
      hasPace ||
      hasCharts ||
      !!localUsage ||
      !!wayfinderUsage ||
      !!openAiApiUsage) &&
    // Compact Overview suppresses supplemental sections entirely; a card
    // whose only content would be suppressed renders header-only so no empty
    // divider or details container appears.
    (!compactOverview || hasMetrics || !!wayfinderUsage || hasPace);
  return {
    hasMetrics,
    hasInventory,
    hasDisplayDetails,
    hasCost,
    hasPace,
    hasCharts,
    hasCostHistory,
    hasCreditsHistory,
    hasUsageBreakdown,
    hasQuotaWindowHistory,
    hasBurndown,
    localUsage,
    wayfinderUsage,
    openAiApiUsage,
    hasDetails,
  };
}

/** Metrics / cost / pace / charts body of a provider MenuCard. */
export default function MenuCardDetails({
  provider,
  display,
  metrics,
  chartData,
  presence,
  onLayoutChange,
}: MenuCardDetailsProps) {
  const { t } = useLocale();
  const paceEnabled =
    display.showPace !== false &&
    providerAllowsPace(provider.providerId, provider.sourceLabel);
  const metricDisplay = paceEnabled ? display : { ...display, showPace: false };
  const compactOverview = display.compactOverview === true;
  const [expandedPaceWindow, setExpandedPaceWindow] = useState<string | null>(null);
  const formattedCostReset = useFormattedResetTime(
    provider.cost?.resetsAt ?? null,
    null,
    display.resetTimeRelative,
  );
  const localCostHistory = chartData?.costHistory ?? [];
  const localTokensHistory = chartData?.tokensHistory ?? [];
  const costStyle = display.costSummaryDisplayStyle ?? "detailed";
  const displayDetailGroups = groupProviderDisplayDetails(
    provider.displayDetails ?? [],
  );
  const costPeriod = providerCostPeriodTitle(
    provider.providerId,
    provider.cost?.period ?? "",
    t,
  );

  const {
    hasMetrics,
    hasInventory,
    hasDisplayDetails,
    hasCost,
    hasPace,
    hasCharts,
    hasCostHistory,
    hasCreditsHistory,
    hasUsageBreakdown,
    hasQuotaWindowHistory,
    hasBurndown,
    localUsage,
    wayfinderUsage,
    openAiApiUsage,
  } = presence;

  return (
    <div className="menu-card__content">
      {!provider.error && hasMetrics && (
        <section className="menu-card__group menu-card__metrics">
          {metrics.map((m) => (
            <MetricRow
              key={m.id}
              title={m.label}
              snap={m.snap}
              lane={m.lane}
              providerId={provider.providerId}
              exhaustedLabel={t("DetailWindowExhausted")}
              display={metricDisplay}
              expanded={expandedPaceWindow === m.id}
              resetFormatMode={m.resetFormatMode}
              sessionEquivalentForecast={m.sessionEquivalentForecast}
              onToggleExpanded={() => {
                setExpandedPaceWindow((current) =>
                  current === m.id ? null : m.id,
                );
                requestAnimationFrame(() => onLayoutChange?.());
              }}
            />
          ))}
        </section>
      )}

      {!provider.error && hasInventory && (
        <section className="menu-card__group menu-card__inventory">
          {provider.inventory?.map((item) => (
            <InventoryItemRow
              key={item.id}
              item={item}
              resetTimeRelative={display.resetTimeRelative}
              lineClassName="menu-card__cost-line"
              expiryClassName="menu-card__cost-line--muted"
            />
          ))}
        </section>
      )}
      {!provider.error && hasDisplayDetails && (
        displayDetailGroups
          .map((group) => ({
            ...group,
            rows: group.rows.filter((detail) =>
              isDetailSectionVisible(provider.hiddenUsageItemIds, detail.title),
            ),
          }))
          .filter((group) => group.rows.length > 0)
          .map((group) => (
            <section
              className="menu-card__group menu-card__provider-details"
              key={group.id}
            >
              {group.title && (
                <div className="menu-card__group-title" role="heading" aria-level={4}>
                  {localizeProviderLabel(group.title, t)}
                </div>
              )}
              {group.rows.map((detail, index) => (
                <ProviderDisplayRow
                  key={`${detail.id}-${index}`}
                  detail={detail}
                  lineClassName="menu-card__cost-line"
                  secondaryClassName="menu-card__cost-line--muted"
                  trackClassName="menu-metric__bar"
                  fillClassName="menu-metric__bar-fill"
                />
              ))}
            </section>
          ))
      )}

      {wayfinderUsage && !compactOverview && <WayfinderUsageBlock usage={wayfinderUsage} />}

      {!compactOverview && hasMetrics && hasCost && <div className="menu-card__divider" />}

      {!compactOverview && hasCost && provider.cost && (
        <section className="menu-card__group menu-card__cost">
          <div className="menu-card__group-title">
            {provider.cost.alwaysVisible === true && (provider.cost.limit ?? 0) <= 0
              ? t("ApiSpendTitle")
              : provider.cost.balance != null && provider.cost.limit == null
                ? provider.cost.period || t("CreditsLabel")
              : `${t("DetailCostTitle")} — ${provider.cost.period}`}
          </div>
          {provider.cost.balance != null && provider.cost.limit == null ? (
            <div className="menu-card__cost-line">
              {provider.cost.formattedBalance ||
                formatCurrency(
                  provider.cost.balance,
                  provider.cost.currencyCode,
                )}
            </div>
          ) : (
            <>
              <div className="menu-card__cost-line">
                {t("DetailCostUsed")}:{" "}
                {provider.cost.formattedUsed ||
                  formatCurrency(
                    provider.cost.used,
                    provider.cost.currencyCode,
                  )}
                {provider.cost.limit != null && (
                  <>
                    {" / "}
                    {provider.cost.formattedLimit ||
                      formatCurrency(
                        provider.cost.limit,
                        provider.cost.currencyCode,
                      )}
                  </>
                )}
              </div>
              {costStyle === "detailed" && provider.cost.balance != null && (
                <div className="menu-card__cost-line menu-card__cost-line--muted">
                  {t("DetailCostBalance")}:{" "}
                  {provider.cost.formattedBalance ||
                    formatCurrency(
                      provider.cost.balance,
                      provider.cost.currencyCode,
                    )}
                </div>
              )}
              {costStyle === "detailed" && provider.cost.remaining != null && (
                <div className="menu-card__cost-line menu-card__cost-line--muted">
                  {t("DetailCostRemaining")}:{" "}
                  {formatCurrency(
                    provider.cost.remaining,
                    provider.cost.currencyCode,
                  )}
                </div>
              )}
              {costStyle === "detailed" && formattedCostReset && (
                <div className="menu-card__cost-line menu-card__cost-line--muted">
                  {t("DetailCostResets")}: {formattedCostReset}
                </div>
              )}
            </>
          )}
          {provider.providerId === "mistral" && provider.cost && (
            <div className="menu-card__cost-line menu-card__monthly-spend">
              {t("MistralMonthlySpend")}:{" "}
              {provider.cost.currencySymbol
                ? `${provider.cost.currencySymbol}${provider.cost.used.toFixed(2)}`
                : provider.cost.formattedUsed}
            </div>
          )}
        </section>
      )}

      {!compactOverview && openAiApiUsage && (
        <details className="menu-card__more menu-card__daily-usage" onToggle={onLayoutChange}>
          <summary>{t("OpenAIChartTitle")}</summary>
          <div className="menu-card__more-content">
            {/* Tray cards open often; skip the bar entrance animation there. */}
            <OpenAIApiUsageChart
              usage={openAiApiUsage}
              animations={false}
              t={t}
              onLayoutChange={onLayoutChange}
            />
          </div>
        </details>
      )}

      {!compactOverview && (localUsage || hasPace || hasCharts) && (
        <details className="menu-card__more" onToggle={onLayoutChange}>
          <summary>{t("PanelUsageDetails")}</summary>
          <div className="menu-card__more-content">
            {localUsage && (
              <LocalUsageBlock
                providerId={provider.providerId}
                summary={localUsage}
                costHistory={localCostHistory}
                tokensHistory={localTokensHistory}
              />
            )}

            {paceEnabled && hasPace && provider.pace && (
              <section className="menu-card__group menu-card__pace">
                <div className="menu-card__pace-header">
                  <span className="menu-card__group-title">{t("DetailPaceTitle")}</span>
                  <span
                    className="menu-card__pace-label"
                    data-pace={paceCategory(provider.pace.stage)}
                  >
                    {t(paceStageKey(provider.pace.stage))} (
                    {provider.pace.deltaPercent >= 0 ? "+" : ""}
                    {provider.pace.deltaPercent.toFixed(1)}%)
                  </span>
                </div>
                <div className="menu-card__pace-bars">
                  <div className="menu-card__pace-track" title={t("PanelExpected")}>
                    <div
                      className="menu-card__pace-fill menu-card__pace-fill--expected"
                      style={{ width: `${provider.pace.expectedUsedPercent.toFixed(1)}%` }}
                    />
                  </div>
                  <div className="menu-card__pace-track" title={t("PanelActual")}>
                    <div
                      className="menu-card__pace-fill"
                      data-pace={paceCategory(provider.pace.stage)}
                      style={{ width: `${provider.pace.actualUsedPercent.toFixed(1)}%` }}
                    />
                  </div>
                </div>
                {provider.pace.etaSeconds != null && !provider.pace.willLastToReset && (
                  <div className="menu-card__pace-eta">
                    ⚠{" "}
                    {t("DetailPaceRunsOutIn")} {formatEta(provider.pace.etaSeconds)}
                  </div>
                )}
                {provider.pace.willLastToReset && (
                  <div className="menu-card__pace-ok">
                    ✓ {t("DetailPaceWillLastToReset")}
                  </div>
                )}
              </section>
            )}

            {(hasMetrics || hasCost || hasPace) && hasCharts && (
              <div className="menu-card__divider" />
            )}

            {hasCharts && (
              <section className="menu-card__group menu-card__charts">
                {hasCostHistory && (
                  <SimpleBarChart
                    points={chartData!.costHistory}
                    label={t("DetailChartCost")}
                    color="var(--provider-accent, var(--accent))"
                    formatValue={(v) => `$${v.toFixed(2)}`}
                    t={t}
                  />
                )}
                {hasCreditsHistory && (
                  <SimpleBarChart
                    points={chartData!.creditsHistory}
                    label={t("DetailChartCredits")}
                    color="var(--provider-status-ok)"
                    formatValue={(v) => v.toFixed(1)}
                    t={t}
                  />
                )}
                {hasUsageBreakdown && (
                  <StackedBarChart
                    points={chartData!.usageBreakdown}
                    label={t("DetailChartUsageBreakdown")}
                    height={56}
                    t={t}
                  />
                )}
                {hasQuotaWindowHistory && (
                  <QuotaWindowHistory history={chartData!.quotaWindowHistory} t={t} />
                )}
                {hasBurndown && provider.quotaBurndown && (
                  <QuotaBurndownChart burndown={provider.quotaBurndown} t={t} />
                )}
              </section>
            )}
          </div>
        </details>
      )}
    </div>
  );
}
