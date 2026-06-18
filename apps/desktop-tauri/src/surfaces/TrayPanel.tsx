import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { BootstrapState, ProviderUsageSnapshot } from "../types/bridge";
import {
  dismissTrayPanel,
  openSettingsWindow,
  quitApp as quitApplication,
  setSurfaceMode,
} from "../lib/tauri";
import { useProviders } from "../hooks/useProviders";
import { useSettings } from "../hooks/useSettings";
import { useUpdateState } from "../hooks/useUpdateState";
import { useLocale } from "../hooks/useLocale";
import { useSurfaceTarget } from "../hooks/useSurfaceMode";
import { useTrayPanelLayout } from "../hooks/useTrayPanelLayout";
import MenuCard from "../components/MenuCard";
import MenuSurface, {
  MenuEmpty,
  type MenuFooterRow,
} from "../components/MenuSurface";
import UpdateBanner from "../components/UpdateBanner";
import ProviderGrid, { prioritizeProviders } from "../components/ProviderGrid";
import { openProviderDashboard, openProviderStatusPage } from "../lib/tauri";
import { DEMO_ENABLED, DEMO_PROVIDERS } from "../lib/demoProviders";
import { orderProviderSnapshots } from "../lib/providerOrder";
import {
  hydrateProviderSlots,
  orderedEnabledProviderSlots,
} from "../lib/trayProviders";

/** Provider IDs that have a dashboard URL in the backend */
const HAS_DASHBOARD = new Set([
  "abacus", "alibaba", "alibabatokenplan", "amp", "augment",
  "azureopenai", "bedrock", "claude", "codex", "codebuff",
  "commandcode", "copilot", "crof", "cursor", "deepgram", "deepseek",
  "doubao", "elevenlabs", "factory", "gemini", "grok", "groq",
  "infini", "jetbrains", "kilo", "kimi", "kimik2", "kiro", "manus",
  "mimo", "minimax", "mistral", "nanogpt", "ollama", "openaiapi",
  "opencode", "opencodego", "openrouter", "perplexity", "stepfun",
  "synthetic", "t3chat", "venice", "vertexai", "warp", "windsurf",
  "zai",
]);
/** Provider IDs that have a status page URL in the backend */
const HAS_STATUS_PAGE = new Set([
  "alibabatokenplan", "amp", "augment", "azureopenai", "bedrock",
  "claude", "codex", "copilot", "deepgram", "deepseek", "elevenlabs",
  "gemini", "grok", "groq", "kiro", "mistral", "openaiapi",
  "openrouter", "vertexai", "windsurf",
]);

const TRAY_INITIAL_REFRESH_DELAY_MS = 250;
const DENSE_OVERVIEW_THRESHOLD = 32;

function getProviderStatus(
  p: ProviderUsageSnapshot,
): "ok" | "warning" | "exhausted" | "error" {
  if (p.error) return "error";
  if (p.primary.isExhausted) return "exhausted";
  if (p.primary.usedPercent > 80) return "warning";
  return "ok";
}
void getProviderStatus;

/**
 * Tray popover surface — two modes like macOS CodexBar:
 * 1. Overview (default): provider grid + all cards stacked
 * 2. Detail: click a provider in grid → show only that provider's card
 */
export default function TrayPanel({ state }: { state: BootstrapState }) {
  const {
    providers: realProviders,
    isRefreshing,
    refresh,
    hasCachedData,
    hasLoadedCache,
  } = useProviders({ initialRefreshDelayMs: TRAY_INITIAL_REFRESH_DELAY_MS });
  const providers = DEMO_ENABLED ? DEMO_PROVIDERS : realProviders;
  const { settings } = useSettings(state.settings);
  const { updateState, checkNow, download, apply, dismiss, openRelease } =
    useUpdateState();
  const { t } = useLocale();
  const surfaceTarget = useSurfaceTarget("trayPanel");

  const sorted = useMemo(
    () =>
      orderProviderSnapshots(
        providers,
        state.providers,
        settings.enabledProviders,
        settings.providerOrder,
      ),
    [providers, settings.enabledProviders, settings.providerOrder, state.providers],
  );
  const denseProviderSlots = useMemo(
    () =>
      orderedEnabledProviderSlots(
        state.providers,
        settings.enabledProviders,
        sorted,
        settings.providerOrder,
      ),
    [settings.enabledProviders, settings.providerOrder, sorted, state.providers],
  );
  const providersById = useMemo(
    () => new Map(sorted.map((provider) => [provider.providerId, provider])),
    [sorted],
  );
  const initialProviderId =
    surfaceTarget?.kind === "provider" ? surfaceTarget.providerId : null;

  // null = overview (all providers), string = single provider detail
  const [selectedProviderId, setSelectedProviderId] = useState<string | null>(
    initialProviderId,
  );
  const [gridExpanded, setGridExpanded] = useState(false);
  const expectsDenseOverview =
    selectedProviderId === null &&
    !gridExpanded &&
    settings.enabledProviders.length + 1 > DENSE_OVERVIEW_THRESHOLD;
  const denseTrayProviders = useMemo(() => {
    if (!expectsDenseOverview) return sorted;
    return hydrateProviderSlots(denseProviderSlots, providersById);
  }, [denseProviderSlots, expectsDenseOverview, providersById, sorted]);

  useEffect(() => {
    setSelectedProviderId(initialProviderId);
  }, [initialProviderId]);

  // Cards to display based on mode
  // Overview: all providers in the grid — non-error first, then errors
  // Detail: only the selected provider's card (macOS shows single provider)
  const visibleProviders = useMemo(() => {
    if (selectedProviderId === null) {
      if (DEMO_ENABLED) {
        return ["codex", "claude"]
          .map((id) => providers.find((p) => p.providerId === id))
          .filter((p): p is ProviderUsageSnapshot => p !== undefined);
      }
      // Overview: show providers in the same Settings/catalog order as the grid.
      if (sorted.length + 1 > DENSE_OVERVIEW_THRESHOLD && !gridExpanded) {
        return prioritizeProviders(denseTrayProviders, null).slice(0, 4);
      }
      return sorted;
    }
    // Detail: show ONLY the selected provider (macOS behavior — no appended errors)
    const match = sorted.find((p) => p.providerId === selectedProviderId);
    if (!match) {
      return sorted;
    }
    return [match];
  }, [denseTrayProviders, sorted, selectedProviderId, gridExpanded]);

  const layoutKey = useMemo(
    () =>
      [
        selectedProviderId ?? "overview",
        gridExpanded ? "expanded" : "collapsed",
        isRefreshing ? "refreshing" : "idle",
        updateState.status,
        updateState.version ?? "",
        updateState.error ?? "",
        expectsDenseOverview ? "dense" : "normal",
        hasLoadedCache ? "cache-ready" : "cache-pending",
        visibleProviders.map((provider) => provider.providerId).join(","),
      ].join("|"),
    [
      selectedProviderId,
      gridExpanded,
      isRefreshing,
      updateState.status,
      updateState.version,
      updateState.error,
      expectsDenseOverview,
      hasLoadedCache,
      visibleProviders,
    ],
  );

  const { layoutReady, requestLayout } = useTrayPanelLayout({
    canMeasure: hasLoadedCache || sorted.length > 0,
    denseOverview: expectsDenseOverview,
    detailMode: selectedProviderId !== null,
    layoutKey,
  });

  const openSettings = useCallback(() => {
    void openSettingsWindow("general").finally(() => {
      void getCurrentWindow().close();
    });
  }, []);
  const openPopOut = useCallback(() => {
    setSurfaceMode("popOut", { kind: "dashboard" });
  }, []);
  const openAbout = useCallback(() => {
    void openSettingsWindow("about").finally(() => {
      void getCurrentWindow().close();
    });
  }, []);
  const quitApp = useCallback(() => {
    void quitApplication();
  }, []);

  const headerActions = [
    { icon: "⧉", title: t("TooltipPopOut"), onClick: openPopOut },
  ];

  const footerRows: MenuFooterRow[] = [
    { icon: "↻", label: "Refresh", shortcut: "Ctrl+R", onClick: refresh },
    { icon: "⚙", label: "Settings\u2026", shortcut: "Ctrl+,", onClick: openSettings },
    { icon: "ⓘ", label: "About CodexBar", onClick: openAbout },
    { icon: "⌧", label: "Quit", shortcut: "Ctrl+Q", onClick: quitApp },
  ];

  // Keyboard shortcuts
  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if (
        e.key === "Escape" &&
        !e.ctrlKey &&
        !e.shiftKey &&
        !e.altKey &&
        !e.metaKey
      ) {
        e.preventDefault();
        void dismissTrayPanel().catch(() => {});
        return;
      }
      if (!e.ctrlKey || e.shiftKey || e.altKey || e.metaKey) return;
      switch (e.key.toLowerCase()) {
        case "r":
          e.preventDefault();
          refresh();
          break;
        case ",":
          e.preventDefault();
          openSettings();
          break;
        case "q":
          e.preventDefault();
          quitApp();
          break;
      }
    };
    window.addEventListener("keydown", handler);
    return () => window.removeEventListener("keydown", handler);
  }, [refresh, openSettings, quitApp]);

  const handleGridClick = useCallback(
    (providerId: string | null) => {
      setSelectedProviderId(providerId);
    },
    [],
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
  const revealClassName = `tray-panel-reveal${layoutReady ? " tray-panel-reveal--ready" : ""}${expectsDenseOverview ? " tray-panel-reveal--dense" : ""}`;

  if (sorted.length === 0) {
    return (
      <div className={revealClassName}>
        <MenuSurface
          variant="tray"
          onRefresh={refresh}
          isRefreshing={isRefreshing}
          actions={headerActions}
          banner={banner}
          footerRows={footerRows}
        >
          <MenuEmpty
            isLoading={isRefreshing && !hasCachedData}
            onSettings={openSettings}
          />
        </MenuSurface>
      </div>
    );
  }

  return (
    <div className={revealClassName}>
      <MenuSurface
        variant="tray"
        onRefresh={refresh}
        isRefreshing={isRefreshing}
        actions={headerActions}
        banner={banner}
        footerRows={footerRows}
      >
        <ProviderGrid
          providers={expectsDenseOverview ? denseTrayProviders : sorted}
          selectedProviderId={selectedProviderId}
          showAsUsed={settings.showAsUsed}
          showProviderIcons={settings.switcherShowsIcons}
          expanded={gridExpanded}
          onExpandedChange={setGridExpanded}
          onSelect={handleGridClick}
        />
        <div className="provider-grid__divider" />
        <div className="menu-stack">
          {visibleProviders.map((p, idx) => {
            const isSelected =
              selectedProviderId !== null && p.providerId === selectedProviderId;
            return (
              <Fragment key={p.providerId}>
                {idx > 0 && <div className="menu-stack__sep" />}
                <div
                  className={`menu-stack__item${isSelected ? " menu-stack__item--selected" : ""}`}
                  id={`card-${p.providerId}`}
                >
                  <MenuCard
                    provider={p}
                    hideEmail={settings.hidePersonalInfo}
                    resetTimeRelative={settings.resetTimeRelative}
                    showAsUsed={settings.showAsUsed}
                    compactMetrics={selectedProviderId === null}
                    onLayoutChange={requestLayout}
                  />
                </div>
              </Fragment>
            );
          })}
        </div>
        {/* Context actions — detail mode only, matches macOS actionsSection */}
        {selectedProviderId && (HAS_DASHBOARD.has(selectedProviderId) || HAS_STATUS_PAGE.has(selectedProviderId)) && (
          <div className="context-actions">
            <div className="context-actions__divider" />
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
                Usage Dashboard
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
                Status Page
              </button>
            )}
          </div>
        )}
      </MenuSurface>
    </div>
  );
}
