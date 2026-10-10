import { Fragment, useEffect, useState, type CSSProperties } from "react";
import { getCurrentWindow, type Window } from "@tauri-apps/api/window";
import type { BootstrapState, ProviderUsageSnapshot, UsageSpendSummary } from "../types/bridge";
import type { LocaleKey } from "../i18n/keys";
import { costPeriodShortLabel } from "../lib/costPeriod";
import { useCurrency } from "../hooks/CurrencyProvider";
import { sumDisplayCurrencyAmounts } from "../lib/currency";
import {
  getUsageSpendSummary,
  openProviderDashboard,
  openProviderStatusPage,
  openSettingsWindow,
  resetFlyoutPosition,
} from "../lib/tauri";
import {
  TRAY_SCALE_MAX,
  TRAY_SCALE_MIN,
  TRAY_SCALE_STEP,
  useTrayPanelController,
} from "../hooks/useTrayPanelController";
import { useStayAwakeStatus } from "../hooks/useStayAwakeStatus";
import MenuCard from "../components/MenuCard";
import MenuSurface, { MenuEmpty } from "../components/MenuSurface";
import UpdateBanner from "../components/UpdateBanner";
import ProviderGrid from "../components/ProviderGrid";
import AgentSessions from "../components/AgentSessions";
import { hasSuccessfulClaudeCliQuota } from "../lib/claudeAccountActions";
import { filterUsageSpendSummaryForOverview } from "../lib/usageSpendSharing";

/** Provider IDs that have a dashboard URL in the backend */
const HAS_DASHBOARD = new Set([
  "abacus", "alibaba", "alibabatokenplan", "amp", "atlascloud", "augment",
  "azureopenai", "bedrock", "claude", "codex", "codebuff",
  "aiand", "aixy", "commandcode", "copilot", "crossmodel", "cursor", "deepgram", "deepinfra", "deepseek", "zenmux", "clinepass", "longcat", "neuralwatt", "zoommate",
  "doubao", "elevenlabs", "factory", "gemini", "grok", "groq",
  "infini", "jetbrains", "kilo", "kimi", "kimik2", "kiro", "manus", "replicate",
  "mimo", "minimax", "mistral", "nanogpt", "notion", "ollama", "openaiapi",
  "opencode", "opencodego", "openrouter", "perplexity", "qoder", "codebuddy", "sakana", "stepfun",
  "t3chat", "venice", "vertexai", "warp", "windsurf",
  "xai", "zai", "fireworks", "meta", "muse", "nous", "llmman", "devpass", "xkiro",
  "raycast", "vercel",
]);
/** Provider IDs that have a status page URL in the backend */
const HAS_STATUS_PAGE = new Set([
  "alibabatokenplan", "amp", "augment", "azureopenai", "bedrock",
  "claude", "codex", "copilot", "deepgram", "deepinfra", "deepseek", "zenmux", "clinepass", "longcat", "neuralwatt", "zoommate", "elevenlabs",
  "gemini", "grok", "groq", "kiro", "mistral", "openaiapi",
  "openrouter", "vertexai", "windsurf", "xai",
]);

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
    trayScaleDraft,
    trayScale,
    trayScaleFillPercent,
    handleTrayScaleChange,
    sorted,
    gridProviders,
    selectedProviderId,
    gridExpanded,
    setGridExpanded,
    visibleProviders,
    wideColumns,
    useWideColumns,
    requestLayout,
    footerRows,
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

  const zoomRow = (
    <div className="menu-surface__footer-row menu-surface__footer-zoom">
      <span>{t("PanelZoom")}</span>
      <input
        type="range"
        className="menu-surface__footer-zoom-slider"
        min={TRAY_SCALE_MIN}
        max={TRAY_SCALE_MAX}
        step={TRAY_SCALE_STEP}
        value={trayScaleDraft}
        aria-label={t("PanelZoom")}
        onChange={(e) => handleTrayScaleChange(Number(e.target.value))}
        style={{ "--zoom-fill": `${trayScaleFillPercent}%` } as CSSProperties}
      />
      <span className="menu-surface__footer-zoom-value">
        {trayScaleDraft}%
      </span>
    </div>
  );

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

  const renderProviderCard = (p: ProviderUsageSnapshot) => {
    const isSelected =
      selectedProviderId !== null && p.providerId === selectedProviderId;
    return (
      <div
        className={`menu-stack__item${isSelected ? " menu-stack__item--selected" : ""}`}
        id={`card-${p.providerId}`}
        key={p.providerId}
      >
        <MenuCard
          provider={p}
          isRefreshing={refreshingProviderIds.has(p.providerId)}
          display={{
            hideEmail: settings.hidePersonalInfo,
            resetTimeRelative: settings.resetTimeRelative,
            showResetWhenExhausted: settings.showResetWhenExhausted,
            showPace: settings.showPace ?? true,
            showAsUsed: settings.showAsUsed,
            compactOverview:
              selectedProviderId === null && settings.overviewLayout !== "detailed",
            costSummaryDisplayStyle: settings.costSummaryDisplayStyle,
            usageThresholds: settings,
            weeklyProgressWorkDays: settings.weeklyProgressWorkDays ?? null,
          }}
          accentColor={settings.providerAccentColors[p.providerId]}
          onLayoutChange={requestLayout}
        />
      </div>
    );
  };

  const selectedProvider = selectedProviderId
    ? sorted.find((provider) => provider.providerId === selectedProviderId) ?? null
    : null;
  const canSwitchClaudeAccount =
    selectedProvider !== null && hasSuccessfulClaudeCliQuota(selectedProvider);
  const stayAwakeStatus = stayAwakeHeld ? (
    <p className="menu-surface__hint" role="status">
      {t("TrayStayAwakeActive")}
    </p>
  ) : null;

  if (sorted.length === 0) {
    return (
      <div className={revealClassName}>
        <MenuSurface
          banner={banner}
          footerLead={zoomRow}
          footerRows={footerRows}
          style={{ zoom: trayScale }}
        >
          {stayAwakeStatus}
          {settings.agentSessionsEnabled && <AgentSessions />}
          <MenuEmpty
            isLoading={isRefreshing && !hasCachedData}
            onSettings={openSettings}
          />
        </MenuSurface>
        <TrayWindowHandles moveHint={t("TrayMoveHandleHint")} />
      </div>
    );
  }

  return (
    <div className={revealClassName}>
      <MenuSurface
        banner={banner}
        footerLead={zoomRow}
        footerRows={footerRows}
        style={{ zoom: trayScale }}
      >
        {stayAwakeStatus}
        {settings.agentSessionsEnabled && <AgentSessions />}
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
            t={t}
          />
        )}
        <div className="menu-stack">
          {useWideColumns
            ? wideColumns.map((column) => (
                <div
                  className="menu-stack__column"
                  key={column.map((p) => p.providerId).join("|") || "empty"}
                >
                  {column.map(renderProviderCard)}
                </div>
              ))
            : visibleProviders.map((p, idx) => (
                <Fragment key={p.providerId}>
                  {idx > 0 && <div className="menu-stack__sep" />}
                  {renderProviderCard(p)}
                </Fragment>
              ))}
        </div>
        {/* Context actions — detail mode only, matches macOS actionsSection */}
        {selectedProviderId &&
          (HAS_DASHBOARD.has(selectedProviderId) ||
            HAS_STATUS_PAGE.has(selectedProviderId) ||
            canSwitchClaudeAccount) && (
          <div className="context-actions">
            <div className="context-actions__divider" />
            {canSwitchClaudeAccount && (
              <button
                type="button"
                className="context-actions__btn"
                onClick={() => openSettingsWindow("providers")}
              >
                <span className="context-actions__icon" aria-hidden>
                  ⇄
                </span>
                {t("ActionSwitchAccount")}
              </button>
            )}
            {HAS_DASHBOARD.has(selectedProviderId) && (
              <button
                type="button"
                className="context-actions__btn"
                onClick={() => void openProviderDashboard(selectedProviderId)}
              >
                <span className="context-actions__icon" aria-hidden>
                  <svg width="13" height="13" viewBox="0 0 16 16" fill="none" xmlns="http://www.w3.org/2000/svg">
                    <rect x="2" y="9" width="2.5" height="5" rx="0.6" fill="currentColor" />
                    <rect x="6.75" y="6" width="2.5" height="8" rx="0.6" fill="currentColor" />
                    <rect x="11.5" y="3" width="2.5" height="11" rx="0.6" fill="currentColor" />
                  </svg>
                </span>
                {t("ActionUsageDashboard")}
              </button>
            )}
            {HAS_STATUS_PAGE.has(selectedProviderId) && (
              <button
                type="button"
                className="context-actions__btn"
                onClick={() => void openProviderStatusPage(selectedProviderId)}
              >
                <span className="context-actions__icon" aria-hidden>
                  <svg width="14" height="13" viewBox="0 0 18 14" fill="none" xmlns="http://www.w3.org/2000/svg">
                    <path d="M1 7H4L5.5 3L8 11L10.5 5L12 7H17" stroke="currentColor" strokeWidth="1.4" strokeLinecap="round" strokeLinejoin="round" fill="none" />
                  </svg>
                </span>
                {t("ActionStatusPage")}
              </button>
            )}
          </div>
        )}
      </MenuSurface>
      <TrayWindowHandles moveHint={t("TrayMoveHandleHint")} />
    </div>
  );
}

type ResizeDirection = Parameters<Window["startResizeDragging"]>[0];

/** Every edge and corner, so a flyout the user moved away from the tray can
 *  be resized from whichever side faces open screen. */
const RESIZE_GRIPS: ReadonlyArray<{ edge: string; direction: ResizeDirection }> = [
  { edge: "top", direction: "North" },
  { edge: "bottom", direction: "South" },
  { edge: "left", direction: "West" },
  { edge: "right", direction: "East" },
  { edge: "topleft", direction: "NorthWest" },
  { edge: "topright", direction: "NorthEast" },
  { edge: "bottomleft", direction: "SouthWest" },
  { edge: "bottomright", direction: "SouthEast" },
];

/**
 * Window handles for the borderless flyout: a move strip along the top and
 * invisible resize grips on every edge and corner. The native frame left
 * around the borderless WebView2 is only a few pixels wide, so both are driven
 * explicitly with `startDragging` / `startResizeDragging`. Those calls enter a
 * Win32 modal move/size loop which transiently steals focus from the WebView2
 * child — Windows fires a spurious `Focused(false)` the instant the press
 * starts. The backend keeps the flyout open on a blur while a mouse button is
 * held on it, so no gesture guard is armed here; arming one would also keep
 * the next genuine outside click from dismissing the panel for up to 15s.
 *
 * Dragging the strip moves the flyout away from the tray and the backend
 * remembers the spot; double-clicking it anchors the flyout to the tray again.
 */
function TrayWindowHandles({ moveHint }: { moveHint: string }) {
  return (
    <>
      <div
        className="tray-move-handle"
        aria-hidden
        title={moveHint}
        onMouseDown={(e) => {
          if (e.button !== 0) return;
          e.preventDefault();
          if (e.detail === 2) {
            void resetFlyoutPosition().catch((err) =>
              console.error("[tray-move] resetFlyoutPosition failed:", err),
            );
            return;
          }
          void getCurrentWindow()
            .startDragging()
            .catch((err) => console.error("[tray-move] startDragging failed:", err));
        }}
      >
        <span className="tray-move-handle__grip" />
      </div>
      {RESIZE_GRIPS.map(({ edge, direction }) => (
        <div
          key={edge}
          className={`tray-resize tray-resize--${edge}`}
          aria-hidden
          onMouseDown={(e) => {
            if (e.button !== 0) return;
            e.preventDefault();
            void getCurrentWindow()
              .startResizeDragging(direction)
              .catch((err) => console.error("[tray-resize] startResizeDragging failed:", err));
          }}
        />
      ))}
    </>
  );
}

function OverviewSpendSummary({
  providerIds,
  period,
  t,
}: {
  providerIds: string[];
  /** Saved History window; a change rescans. Omitted means the backend default. */
  period?: string;
  t: (key: LocaleKey) => string;
}) {
  const { preferredCode, rates } = useCurrency();
  const [summary, setSummary] = useState<UsageSpendSummary | null>(null);

  useEffect(() => {
    let cancelled = false;
    // No explicit period: the backend resolves the saved History window.
    void getUsageSpendSummary()
      .then((value) => { if (!cancelled) setSummary(value); })
      .catch(() => { if (!cancelled) setSummary(null); });
    return () => { cancelled = true; };
  }, [providerIds.join("|"), period]);

  // Overview consumes the same backend spend catalog as Usage & Spend. Do not
  // restrict accounting to whichever cards happen to be rendered in this tray.
  const overviewSummary = summary ? filterUsageSpendSummaryForOverview(summary) : null;

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

  return (
    <div className="provider-detail-section" style={{ margin: "8px 8px 10px", padding: "10px 12px" }}>
      <div style={{ display: "flex", justifyContent: "space-between", gap: 12, alignItems: "baseline" }}>
        <strong>{title}</strong>
        <strong>
          {aggregate.total == null
            ? "—"
            : `${partial ? "~" : ""}${new Intl.NumberFormat(undefined, { style: "currency", currency: target === "AUTO" ? "USD" : target, maximumFractionDigits: 2 }).format(aggregate.total)}`}
        </strong>
      </div>
      <div className="settings-section__caption" style={{ marginTop: 4 }}>
        {t("OverviewSpendProviderCoverage")
          .replace("{}", String(aggregate.included))
          .replace("{}", String(aggregate.considered))}{" "}
        · {t("OverviewSpendEstimate")}
      </div>
    </div>
  );
}
