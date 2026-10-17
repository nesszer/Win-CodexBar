import { type CSSProperties, useEffect, useState } from "react";
import type {
  CostSummaryDisplayStyle,
  ProviderChartData,
  ProviderUsageSnapshot,
  WorkdayTickStyle,
} from "../types/bridge";
import { getProviderChartData } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import { providerSupportsChartData } from "../lib/providerCharts";
import { providerAllowsPace } from "../lib/providerPace";
import { hideOpenAiApiProjectId } from "../lib/openAiApiIdentity";
import {
  claudePlanDateNote,
  isUsageItemVisible,
  projectClaudeSections,
} from "../lib/usageItemVisibility";
import { localizeWindowLabel } from "../lib/windowLabels";
import { inlineDashboardModel } from "../lib/inlineDashboard";
import { useMonthlyLimitBlockNow } from "../hooks/useMonthlyLimitBlockNow";
import type { UsageThresholdSettings } from "../lib/metricRowModel";
import { providerBarAccent } from "./providers/providerIcons";
import CardHeader, { displayPlanName } from "./card/CardHeader";
import CardMetricRow, { type MetricEntry } from "./card/CardMetricRow";
import DetailSections from "./card/DetailSections";
import InlineDashboard from "./card/InlineDashboard";
import ExtraUsageBlock from "./card/ExtraUsageBlock";
import WayfinderBlock from "./card/WayfinderBlock";
import { OpenAITokensPerMinuteChart } from "./OpenAITokensPerMinuteChart";
import { describeCard } from "./card/describeCard";
import { hideEmailsInText } from "../lib/personalInfo";

export interface MenuCardDisplayOptions {
  hideEmail: boolean;
  resetTimeRelative: boolean;
  showResetWhenExhausted?: boolean;
  showPace?: boolean;
  showAsUsed?: boolean;
  /**
   * Compact Overview layout: the header and the quota rows only (plus the
   * Wayfinder telemetry, which is that provider's quota).
   */
  compactOverview?: boolean;
  costSummaryDisplayStyle?: CostSummaryDisplayStyle;
  usageThresholds?: UsageThresholdSettings | null;
  quotaWarningMarkersVisible?: boolean;
  weeklyProgressWorkDays?: number | null;
  workdayTickStyle?: WorkdayTickStyle;
  /** Settings "Track costs"; a change re-reads the chart so local estimates follow it. */
  costTrackingEnabled?: boolean;
}

interface MenuCardProps {
  provider: ProviderUsageSnapshot;
  display: MenuCardDisplayOptions;
  isRefreshing?: boolean;
  /** Per-provider accent color override (hex); defaults to the brand color. */
  accentColor?: string;
  onLayoutChange?: () => void;
}

/**
 * Upstream `MenuCardView.email(from:)`: the provider-reported email, else the
 * active token-account label. Hide Personal Info shows neither, as upstream
 * `PersonalInfoRedactor.redactEmail` / `redactAccountLabel` blank them
 * (`PersonalInfoRedactor.swift:18`, `emailPlaceholder = ""`).
 */
export function headerAccount(
  provider: Pick<ProviderUsageSnapshot, "accountEmail" | "accountLabel">,
  hideEmail: boolean,
): string | null {
  if (hideEmail) return null;
  return provider.accountEmail?.trim() || provider.accountLabel?.trim() || null;
}

function cardMetrics(
  provider: ProviderUsageSnapshot,
  t: ReturnType<typeof useLocale>["t"],
  language: ReturnType<typeof useLocale>["language"],
): MetricEntry[] {
  const metrics: MetricEntry[] = [];
  if (provider.providerId !== "wayfinder") {
    metrics.push({
      id: "primary",
      label:
        localizeWindowLabel(provider.primaryLabel, t, language, provider.primary.windowMinutes) ||
        t("DetailWindowPrimary"),
      snap: provider.primary,
      lane: "primary",
    });
  }
  if (provider.secondary) {
    metrics.push({
      id: "secondary",
      label: provider.secondaryLabelKey
        ? t(provider.secondaryLabelKey)
        : localizeWindowLabel(provider.secondaryLabel, t) || t("DetailWindowSecondary"),
      snap: provider.secondary,
      lane: "secondary",
    });
  }
  if (provider.modelSpecific) {
    metrics.push({
      id: "model-specific",
      label: t("DetailWindowModelSpecific"),
      snap: provider.modelSpecific,
      lane: "tertiary",
    });
  }
  if (provider.tertiary) {
    metrics.push({
      id: "tertiary",
      // Upstream 0.48.0: the cadence label (e.g. "Monthly") when the provider names one.
      label: localizeWindowLabel(provider.tertiaryLabel, t) || t("DetailWindowTertiary"),
      snap: provider.tertiary,
      lane: "tertiary",
    });
  }
  for (const extra of provider.extraRateWindows ?? []) {
    metrics.push({
      id: `extra-${extra.id}`,
      label:
        localizeWindowLabel(extra.title, t, language, extra.window.windowMinutes, extra.id) ||
        extra.title,
      snap: extra.window,
      lane: "extra",
      resetFormatMode: extra.id === "reset-credits" ? "expires" : "reset",
    });
  }
  // A window the provider keeps for the tray and float bar only (upstream LithosAI has no card bar).
  return metrics.filter(
    (metric) =>
      !metric.snap.hiddenOnCard && isUsageItemVisible(provider.hiddenUsageItemIds, metric.id),
  );
}

/**
 * Provider card, following upstream `UsageMenuCardView` (0.70.0), top to bottom:
 * header, divider, rate-window rows, key-value sections, the inline usage
 * dashboard, then the extra usage block. Account switching, pace budgets,
 * forecasts, burndown and pricing details live in Settings > Providers.
 */
export default function MenuCard({
  provider: reportedProvider,
  display,
  isRefreshing = false,
  accentColor,
  onLayoutChange,
}: MenuCardProps) {
  const {
    hideEmail,
    resetTimeRelative,
    showResetWhenExhausted = false,
    showPace = true,
    showAsUsed = false,
    compactOverview = false,
    costSummaryDisplayStyle,
    usageThresholds = null,
    quotaWarningMarkersVisible = true,
    weeklyProgressWorkDays = null,
    workdayTickStyle = "subtle",
    costTrackingEnabled = true,
  } = display;
  const { t, language } = useLocale();
  const provider = projectClaudeSections(reportedProvider.providerId, reportedProvider);
  const [chartData, setChartData] = useState<ProviderChartData | null>(null);

  // Another provider or account must not show the previous card's chart.
  useEffect(() => {
    setChartData(null);
  }, [provider.providerId, provider.accountEmail]);

  // Re-read the local-usage chart when the local calendar day changes (seen on
  // the next render, e.g. a provider update), so "Today" does not freeze at the
  // first mount without a full transcript walk on every refresh.
  const localDay = new Date().toLocaleDateString("sv");
  useEffect(() => {
    if (!providerSupportsChartData(provider.providerId)) return;
    let cancelled = false;
    let frame: number | undefined;
    getProviderChartData(provider.providerId, provider.accountEmail ?? undefined)
      .then((data) => {
        if (!cancelled) {
          setChartData(data);
          frame = requestAnimationFrame(() => onLayoutChange?.());
        }
      })
      .catch(() => {
        /* chart data is best-effort */
      });
    return () => {
      cancelled = true;
      if (frame !== undefined) cancelAnimationFrame(frame);
    };
    // "Track costs" flips local estimates on or off; the backend answers for the new value.
  }, [provider.providerId, provider.accountEmail, localDay, costTrackingEnabled, onLayoutChange]);

  const isWayfinder = provider.providerId === "wayfinder";
  const email = isWayfinder ? null : headerAccount(provider, hideEmail);
  const plan = isWayfinder
    ? null
    : hideOpenAiApiProjectId(
        displayPlanName(provider.planName, t),
        provider.providerId === "openaiapi" && hideEmail,
      );

  const metrics = cardMetrics(provider, t, language);
  // Snapshots are cached across failed refreshes, so monthly-limit blocks are
  // re-checked against the clock and lift when the pool's reset passes.
  const monthlyLimitBlockNow = useMonthlyLimitBlockNow(
    metrics.map((metric) => metric.snap.monthlyLimitBlock),
  );
  const presence = describeCard(
    provider,
    chartData,
    metrics,
    costSummaryDisplayStyle,
    compactOverview,
    claudePlanDateNote(provider.providerId, provider.subscription, t),
  );
  const dashboard =
    !compactOverview && presence.localUsage
      ? inlineDashboardModel({
          providerId: provider.providerId,
          summary: presence.localUsage,
          costHistory: chartData?.costHistory ?? [],
          tokensHistory: chartData?.tokensHistory ?? [],
          quotaHistory: chartData?.quotaWindowHistory?.windows,
          t,
        })
      : null;
  const showDetailSections = presence.showsDetailSections;
  const showCost = !compactOverview && presence.hasCost && !!provider.cost;
  const metricDisplay = {
    resetTimeRelative,
    showResetWhenExhausted,
    showPace: showPace && providerAllowsPace(provider.providerId, provider.sourceLabel),
    showAsUsed,
    compactOverview,
    monthlyLimitBlockNow,
    usageThresholds,
    quotaWarningMarkersVisible,
    weeklyProgressWorkDays,
    workdayTickStyle,
  };

  const cardClassName = [
    "menu-card",
    provider.error ? "menu-card--error" : null,
    isRefreshing ? "menu-card--refreshing" : null,
    presence.hasDetails ? "menu-card--with-details" : "menu-card--header-only",
  ]
    .filter(Boolean)
    .join(" ");
  const accent = providerBarAccent(provider.providerId, accentColor);

  return (
    <article
      className={cardClassName}
      aria-busy={isRefreshing}
      style={{ "--provider-accent": accent } as CSSProperties}
    >
      <CardHeader
        name={provider.displayName}
        email={email}
        plan={plan}
        error={hideEmail ? hideEmailsInText(provider.error) : provider.error}
        updatedAt={provider.updatedAt}
        isRefreshing={isRefreshing}
      />

      {presence.hasDetails && <div className="menu-card__divider" />}

      {presence.hasDetails && (
        <div className="menu-card__content">
          {presence.hasMetrics && (
            <section className="menu-card__metrics">
              {metrics.map((metric) => (
                <CardMetricRow
                  key={metric.id}
                  title={metric.label}
                  snap={metric.snap}
                  lane={metric.lane}
                  providerId={provider.providerId}
                  display={metricDisplay}
                  resetFormatMode={metric.resetFormatMode}
                />
              ))}
            </section>
          )}
          {presence.planDateNote && <p className="card-notes">{presence.planDateNote}</p>}
          {showDetailSections && (
            <DetailSections
              details={provider.displayDetails ?? []}
              inventory={provider.inventory ?? []}
              hiddenUsageItemIds={provider.hiddenUsageItemIds}
              resetTimeRelative={resetTimeRelative}
            />
          )}
          {presence.wayfinderUsage && <WayfinderBlock usage={presence.wayfinderUsage} />}
          {presence.openAiThroughput && (
            <OpenAITokensPerMinuteChart throughput={presence.openAiThroughput} t={t} />
          )}
          {dashboard && <InlineDashboard model={dashboard} />}
          {showCost && provider.cost && (
            <>
              {(presence.hasMetrics ||
                presence.planDateNote ||
                showDetailSections ||
                presence.openAiThroughput ||
                dashboard) && (
                <div className="menu-card__separator" />
              )}
              <ExtraUsageBlock
                providerId={provider.providerId}
                cost={provider.cost}
                style={costSummaryDisplayStyle}
                resetTimeRelative={resetTimeRelative}
              />
            </>
          )}
        </div>
      )}
    </article>
  );
}
