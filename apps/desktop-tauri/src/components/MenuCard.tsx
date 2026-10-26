import { Fragment, type CSSProperties, type ReactNode, useEffect, useState } from "react";
import type {
  CostSummaryDisplayStyle,
  ProviderChartData,
  ProviderUsageSnapshot,
  TrayPanelStyle,
  UsageSpendSummary,
  WorkdayTickStyle,
} from "../types/bridge";
import { getProviderChartData, getUsageSpendSummary } from "../lib/tauri";
import { useLocale } from "../hooks/useLocale";
import { providerListsConversations, providerSupportsChartData } from "../lib/providerCharts";
import { cardConversationsModel } from "../lib/cardConversations";
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
import CardConversations from "./card/CardConversations";
import WayfinderBlock from "./card/WayfinderBlock";
import { OpenAILiveSpendChart } from "./OpenAILiveSpendChart";
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
  /** Saved History window (raw); a change re-reads the Conversations list. */
  costReportingPeriod?: string;
  /**
   * One account's row in an Overview that lists accounts separately. Local-log
   * charts and the Conversations list are per provider, not per account, so
   * they are not read for such a row.
   */
  accountScoped?: boolean;
  /** Settings > Menu > Panel look; defaults to modern. */
  panelStyle?: TrayPanelStyle;
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
 * Provider card, top to bottom: header, divider, rate-window rows, key-value
 * sections, the extra usage block, the collapsible Cost dashboard and the local
 * Conversations list, each section set off by a hairline. Account switching, pace budgets,
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
    costReportingPeriod,
    accountScoped = false,
    panelStyle = "modern",
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
    if (!providerSupportsChartData(provider.providerId) || accountScoped) return;
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
  }, [provider.providerId, provider.accountEmail, localDay, costTrackingEnabled, accountScoped, onLayoutChange]);

  // The Conversations list reads the same backend-cached summary as Usage & Spend,
  // which masks titles itself; a summary fetched under the other Hide Personal
  // Info value is never shown.
  const listsConversations =
    !compactOverview &&
    !accountScoped &&
    costTrackingEnabled &&
    providerListsConversations(provider.providerId);
  const [spend, setSpend] = useState<{ summary: UsageSpendSummary; hidden: boolean } | null>(null);
  useEffect(() => {
    if (!listsConversations) {
      setSpend(null);
      return;
    }
    let cancelled = false;
    let frame: number | undefined;
    getUsageSpendSummary()
      .then((summary) => {
        if (cancelled) return;
        setSpend({ summary, hidden: hideEmail });
        frame = requestAnimationFrame(() => onLayoutChange?.());
      })
      .catch(() => {
        if (!cancelled) setSpend(null);
      });
    return () => {
      cancelled = true;
      if (frame !== undefined) cancelAnimationFrame(frame);
    };
  }, [listsConversations, hideEmail, costReportingPeriod, localDay, onLayoutChange]);
  const conversations =
    listsConversations && spend && spend.hidden === hideEmail
      ? cardConversationsModel(provider.providerId, spend.summary, t)
      : null;

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
    conversations,
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
    showPace:
      showPace &&
      providerAllowsPace(provider.providerId, provider.sourceLabel, provider.paceAuthoritative),
    showAsUsed,
    compactOverview,
    monthlyLimitBlockNow,
    usageThresholds,
    quotaWarningMarkersVisible,
    weeklyProgressWorkDays,
    workdayTickStyle,
    panelStyle,
  };

  // Card sections in screen order. Modern sets every neighbouring pair off with a
  // hairline and puts the extra usage block before Cost; classic keeps the extra
  // usage block last, set off by a single hairline.
  const modern = panelStyle !== "classic";
  const sections: { key: string; node: ReactNode }[] = [];
  if (presence.hasMetrics || presence.planDateNote) {
    sections.push({
      key: "metrics",
      node: (
        <>
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
        </>
      ),
    });
  }
  if (showDetailSections) {
    sections.push({
      key: "details",
      node: (
        <DetailSections
          details={provider.displayDetails ?? []}
          inventory={provider.inventory ?? []}
          hiddenUsageItemIds={provider.hiddenUsageItemIds}
          resetTimeRelative={resetTimeRelative}
        />
      ),
    });
  }
  if (presence.wayfinderUsage) {
    sections.push({ key: "wayfinder", node: <WayfinderBlock usage={presence.wayfinderUsage} /> });
  }
  if (presence.openAiThroughput) {
    sections.push({
      key: "openai-throughput",
      node: (
        <OpenAILiveSpendChart
          key={provider.providerId}
          providerId={provider.providerId}
          throughput={presence.openAiThroughput}
          usage={provider.openAiApiUsage}
          t={t}
        />
      ),
    });
  }
  if (showCost && provider.cost) {
    sections.push({
      key: "extra-usage",
      node: (
        <ExtraUsageBlock
          providerId={provider.providerId}
          cost={provider.cost}
          style={costSummaryDisplayStyle}
          resetTimeRelative={resetTimeRelative}
        />
      ),
    });
  }
  if (dashboard) {
    sections.push({
      key: "dashboard",
      node: (
        <InlineDashboard
          model={dashboard}
          onToggle={() => requestAnimationFrame(() => onLayoutChange?.())}
          collapsible={modern}
        />
      ),
    });
  }
  if (presence.conversations) {
    sections.push({
      key: "conversations",
      node: (
        <CardConversations
          model={presence.conversations}
          onToggle={() => requestAnimationFrame(() => onLayoutChange?.())}
        />
      ),
    });
  }

  const ordered = modern
    ? sections
    : [
        ...sections.filter((section) => section.key !== "extra-usage"),
        ...sections.filter((section) => section.key === "extra-usage"),
      ];

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
          {ordered.map((section, index) => (
            <Fragment key={section.key}>
              {(modern
                ? index > 0
                : section.key === "extra-usage" &&
                  ordered.slice(0, index).some((earlier) => earlier.key !== "wayfinder")) && (
                <div className="menu-card__separator" />
              )}
              {section.node}
            </Fragment>
          ))}
        </div>
      )}
    </article>
  );
}
