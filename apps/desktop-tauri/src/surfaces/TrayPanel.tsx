import { Fragment, useEffect, useState, type CSSProperties } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { BootstrapState, ProviderUsageSnapshot, UsageSpendSummary } from "../types/bridge";
import type { LocaleKey } from "../i18n/keys";
import { costPeriodShortLabel } from "../lib/costPeriod";
import { useCurrency } from "../hooks/CurrencyProvider";
import { sumDisplayCurrencyAmounts } from "../lib/currency";
import { getUsageSpendSummary } from "../lib/tauri";
import { useTrayPanelController } from "../hooks/useTrayPanelController";
import { useStayAwakeStatus } from "../hooks/useStayAwakeStatus";
import MenuCard from "../components/MenuCard";
import MenuSurface, { MenuEmpty } from "../components/MenuSurface";
import UpdateBanner from "../components/UpdateBanner";
import ProviderGrid from "../components/ProviderGrid";
import AgentSessions from "../components/AgentSessions";
import { accentSelectionStyle } from "../lib/menuSelection";
import { filterUsageSpendSummaryForOverview } from "../lib/usageSpendSharing";
import { expandOverviewAccountRows, type OverviewEntry } from "../lib/overviewAccountRows";
import { useCodexAccountsState } from "../hooks/useCodexAccountsState";
import { useClaudeTokenOverviewAccounts } from "../hooks/useClaudeTokenOverviewAccounts";
import { useClaudeSwapOverviewAccounts } from "../hooks/useClaudeSwapOverviewAccounts";

/**
 * Tray popover surface — two modes like macOS CodexBar:
 * 1. Overview (default): provider grid + all cards stacked
 * 2. Detail: click a provider in grid → show only that provider's card
 */
export default function TrayPanel({ state }: { state: BootstrapState }) {
  const stayAwakeHeld = useStayAwakeStatus();
  const {
    t,
    settings,
    isRefreshing,
    refreshingProviderIds,
    hasCachedData,
    trayScale,
    accentColor,
    sorted,
    gridProviders,
    selectedProviderId,
    gridExpanded,
    setGridExpanded,
    visibleProviders,
    requestLayout,
    footerGroups,
    statusLine,
    updateState,
    checkNow,
    download,
    apply,
    dismiss,
    openRelease,
    openSettings,
    handleGridClick,
    handleReorder,
    handleGestureStart,
    handleGestureEnd,
    revealClassName,
  } = useTrayPanelController(state);
  // Read the cached Codex account store only while "Show all accounts in Overview" is on.
  const codexAccounts = useCodexAccountsState({
    enabled: settings.mergedOverviewAllAccounts === true,
  });
  // Saved Claude token accounts and their cached usage, under the same setting.
  const claudeTokenAccounts = useClaudeTokenOverviewAccounts({
    enabled: settings.mergedOverviewAllAccounts === true,
  });
  // claude-swap/tcr pool accounts and their cached usage, under the same setting.
  const claudeSwapAccounts = useClaudeSwapOverviewAccounts({
    enabled: settings.mergedOverviewAllAccounts === true,
  });

  const surfaceStyle = {
    zoom: trayScale,
    ...accentSelectionStyle(accentColor),
  } as CSSProperties;
  const hoverHighlight = settings.highlightMenuCardsOnHover ?? true;

  const banner = (
    <UpdateBanner
      updateState={updateState}
      onCheck={checkNow}
      onDownload={download}
      onApply={apply}
      onDismiss={dismiss}
      onOpenRelease={openRelease}
    />
  );

  const renderProviderCard = (p: ProviderUsageSnapshot, entry: OverviewEntry | null = null) => {
    const isSelected =
      selectedProviderId !== null && p.providerId === selectedProviderId;
    const rowKey = entry?.key ?? p.providerId;
    // An account row's label is already privacy-safe (generic when hidden), so
    // it stays visible; a plain card keeps the account email hidden as before.
    const hideEmail = entry?.accountId != null ? false : settings.hidePersonalInfo;
    return (
      <div
        className={`menu-stack__item${isSelected ? " menu-stack__item--selected" : ""}`}
        id={`card-${rowKey}`}
        key={rowKey}
      >
        <MenuCard
          provider={p}
          isRefreshing={refreshingProviderIds.has(p.providerId)}
          display={{
            hideEmail,
            resetTimeRelative: settings.resetTimeRelative,
            showResetWhenExhausted: settings.showResetWhenExhausted,
            showPace: settings.showPace ?? true,
            showAsUsed: settings.showAsUsed,
            compactOverview:
              selectedProviderId === null && settings.overviewLayout !== "detailed",
            costSummaryDisplayStyle: settings.costSummaryDisplayStyle,
            usageThresholds: settings,
            quotaWarningMarkersVisible: settings.quotaWarningMarkersVisible ?? true,
            weeklyProgressWorkDays: settings.weeklyProgressWorkDays ?? null,
            workdayTickStyle: settings.workdayTickStyle ?? "subtle",
            costTrackingEnabled: settings.costUsageEnabled ?? true,
            costReportingPeriod: settings.costReportingPeriod,
            accountScoped: entry?.accountId != null,
          }}
          accentColor={settings.providerAccentColors[p.providerId]}
          onLayoutChange={requestLayout}
        />
      </div>
    );
  };

  // Overview: each selected provider, expanded to one row per cached account
  // (Codex managed accounts, claude-swap/tcr pool accounts, Claude token accounts)
  // when the setting is on. Rows never take a provider slot; the cap applies first.
  const cardEntries: OverviewEntry[] =
    selectedProviderId === null && settings.mergedOverviewAllAccounts === true
      ? expandOverviewAccountRows(
          visibleProviders,
          codexAccounts.loaded ? codexAccounts : null,
          {
            hidePersonalInfo: settings.hidePersonalInfo,
            accountWord: t("Account"),
            unavailableText: t("CodexAccountsUsageUnavailable"),
          },
          claudeTokenAccounts.loaded ? claudeTokenAccounts : null,
          claudeSwapAccounts.loaded ? claudeSwapAccounts : null,
        )
      : visibleProviders.map((provider) => ({
          key: provider.providerId,
          provider,
          accountId: null,
        }));

  const stayAwakeStatus = stayAwakeHeld ? (
    <p className="menu-surface__hint" role="status">
      {t("TrayStayAwakeActive")}
    </p>
  ) : null;

  if (sorted.length === 0) {
    return (
      <div className={revealClassName} data-theme="light">
        <MenuSurface banner={banner} footerGroups={footerGroups}
          statusLine={statusLine} style={surfaceStyle} hoverHighlight={hoverHighlight}>
          {stayAwakeStatus}
          {settings.agentSessionsEnabled && <AgentSessions key={settings.agentSessionsLocalOnly ? "local-only" : "all-hosts"} />}
          <MenuEmpty
            isLoading={isRefreshing && !hasCachedData}
            onSettings={openSettings}
          />
        </MenuSurface>
      </div>
    );
  }

  return (
    <div className={revealClassName} data-theme="light">
      <MenuSurface banner={banner} footerGroups={footerGroups}
        statusLine={statusLine} style={surfaceStyle} hoverHighlight={hoverHighlight}>
        {stayAwakeStatus}
        {settings.agentSessionsEnabled && <AgentSessions key={settings.agentSessionsLocalOnly ? "local-only" : "all-hosts"} />}
        <ProviderGrid
          providers={gridProviders}
          selectedProviderId={selectedProviderId}
          showAsUsed={settings.showAsUsed}
          showProviderIcons={settings.switcherShowsIcons}
          expanded={gridExpanded}
          onExpandedChange={setGridExpanded}
          onSelect={handleGridClick}
          onReorder={handleReorder}
          onGestureStart={handleGestureStart}
          onGestureEnd={handleGestureEnd}
        />
        <div className="provider-grid__divider" />
        {selectedProviderId === null && (
          <OverviewSpendSummary
            providerIds={sorted.map((provider) => provider.providerId)}
            period={settings.costReportingPeriod}
            costTrackingEnabled={settings.costUsageEnabled ?? true}
            t={t}
          />
        )}
        <div className="menu-stack">
          {cardEntries.map((entry, idx) => (
            <Fragment key={entry.key}>
              {idx > 0 && <div className="menu-stack__sep" />}
              {renderProviderCard(entry.provider, entry)}
            </Fragment>
          ))}
        </div>
      </MenuSurface>
    </div>
  );
}

/**
 * Splits "Usage & Spend · {}" into the semibold heading and the secondary
 * "· range" tail, whatever the locale puts around the placeholder.
 */
export function splitSpendTitle(
  template: string,
  range: string,
): { heading: string; detail: string | null } {
  const at = template.indexOf("{}");
  if (at < 0) return { heading: template, detail: null };
  const before = template.slice(0, at);
  const after = template.slice(at + 2);
  const dot = before.lastIndexOf("·");
  const cut = dot < 0 ? before.length : dot;
  const heading = before.slice(0, cut).trim();
  const detail = `${before.slice(cut).trim()} ${range}${after}`.trim();
  return heading ? { heading, detail } : { heading: detail, detail: null };
}

function OverviewSpendSummary({
  providerIds,
  period,
  costTrackingEnabled = true,
  t,
}: {
  providerIds: string[];
  /** Saved History window; a change rescans. Omitted means the backend default. */
  period?: string;
  /** Settings "Track costs"; a change re-reads the summary (off hides it). */
  costTrackingEnabled?: boolean;
  t: (key: LocaleKey) => string;
}) {
  const { preferredCode, rates } = useCurrency();
  const [summary, setSummary] = useState<UsageSpendSummary | null>(null);

  useEffect(() => {
    let cancelled = false;
    if (!costTrackingEnabled) {
      setSummary(null);
      return () => { cancelled = true; };
    }
    let request = 0;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let unlisten: UnlistenFn | null = null;
    const load = () => {
      const current = ++request;
      // No explicit period: the backend resolves the saved History window.
      void getUsageSpendSummary()
        .then((value) => { if (!cancelled && current === request) setSummary(value); })
        .catch(() => { if (!cancelled && current === request) setSummary(null); });
    };
    load();
    void listen("provider-updated", () => {
      if (timer) clearTimeout(timer);
      timer = setTimeout(load, 200);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    }).catch(() => {});
    return () => {
      cancelled = true;
      if (timer) clearTimeout(timer);
      unlisten?.();
    };
  }, [providerIds.join("|"), period, costTrackingEnabled]);

  // Overview consumes the same backend spend catalog as Usage & Spend. Do not
  // restrict accounting to whichever cards happen to be rendered in this tray.
  // Track costs off: no local estimates anywhere, so no Overview spend either.
  const overviewSummary = summary && summary.costTrackingEnabled !== false
    ? filterUsageSpendSummaryForOverview(summary)
    : null;

  if (!overviewSummary) return null;
  // The summary names the History window its period columns cover.
  const summaryPeriod = overviewSummary.reportingPeriod;
  const title = t("OverviewSpendPeriodTitle").replace("{}", costPeriodShortLabel(summaryPeriod, t));

  const rows = overviewSummary.rows;
  const target = preferredCode.trim().toUpperCase() || "AUTO";
  const aggregate = sumDisplayCurrencyAmounts(
    rows.map((row) => ({ amount: row.periodCost, currency: row.currency || "USD" })),
    target,
    rates,
  );
  if (aggregate.total == null && aggregate.considered === 0) return null;
  const partial = aggregate.included < aggregate.considered;

  const { heading, detail } = splitSpendTitle(
    t("OverviewSpendPeriodTitle"),
    costPeriodShortLabel(summaryPeriod, t),
  );
  const total =
    aggregate.total == null
      ? "—"
      : `${partial ? "~" : ""}${new Intl.NumberFormat(undefined, { style: "currency", currency: target === "AUTO" ? "USD" : target, maximumFractionDigits: 2 }).format(aggregate.total)}`;

  // Upstream `OverviewSpendSummaryView`: headline, the title2 total, then captions.
  return (
    <section className="overview-spend" aria-label={title}>
      <div className="overview-spend__headline">
        <span className="overview-spend__heading">{heading}</span>
        {detail && (
          <>
            {" "}
            <span className="overview-spend__range">{detail}</span>
          </>
        )}
      </div>
      <strong className="overview-spend__total">{total}</strong>
      <div className="overview-spend__caption">
        {t("OverviewSpendProviderCoverage")
          .replace("{}", String(aggregate.included))
          .replace("{}", String(aggregate.considered))}{" "}
        · {t("OverviewSpendEstimate")}
      </div>
    </section>
  );
}
