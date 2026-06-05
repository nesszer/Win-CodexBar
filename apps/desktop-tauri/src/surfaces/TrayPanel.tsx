import { Fragment, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import type { BootstrapState, ProviderCatalogEntry, ProviderUsageSnapshot } from "../types/bridge";
import { setSurfaceMode, openSettingsWindow, quitApp as quitApplication } from "../lib/tauri";
import { getWorkAreaRect, reanchorTrayPanel, revealTrayPanelWindow } from "../lib/tauri";
import { useProviders } from "../hooks/useProviders";
import { useSettings } from "../hooks/useSettings";
import { useUpdateState } from "../hooks/useUpdateState";
import { useLocale } from "../hooks/useLocale";
import { useSurfaceTarget } from "../hooks/useSurfaceMode";
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
const TRAY_WIDTH = 328;
const TRAY_MAX_MEASURE_HEIGHT = 920;
const TRAY_OVERVIEW_MIN_HEIGHT = 200;
const TRAY_DETAIL_MIN_HEIGHT = 420;
const TRAY_DENSE_OVERVIEW_HEIGHT = 776;
const DENSE_OVERVIEW_THRESHOLD = 32;

function emptyRateWindow() {
  return {
    usedPercent: 0,
    remainingPercent: 100,
    windowMinutes: null,
    resetsAt: null,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
  };
}

function providerPlaceholder(providerId: string, displayName: string): ProviderUsageSnapshot {
  return {
    providerId,
    displayName,
    primary: emptyRateWindow(),
    primaryLabel: "Usage",
    secondary: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "pending",
    updatedAt: new Date(0).toISOString(),
    error: "Loading provider data...",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    fetchDurationMs: null,
  };
}

function orderedEnabledProviderSlots(
  catalog: ProviderCatalogEntry[],
  enabledProviderIds: string[],
  snapshots: ProviderUsageSnapshot[],
): Array<{ id: string; displayName: string }> {
  const enabled = new Set(enabledProviderIds);
  const snapshotNames = new Map(
    snapshots.map((provider) => [provider.providerId, provider.displayName]),
  );
  const slots: Array<{ id: string; displayName: string }> = [];
  const seen = new Set<string>();

  for (const provider of catalog) {
    if (!enabled.has(provider.id)) continue;
    seen.add(provider.id);
    slots.push({ id: provider.id, displayName: provider.displayName });
  }

  for (const providerId of enabledProviderIds) {
    if (seen.has(providerId)) continue;
    slots.push({
      id: providerId,
      displayName: snapshotNames.get(providerId) ?? providerId,
    });
  }

  return slots;
}

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
    lastRefresh,
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
    () => orderProviderSnapshots(providers, state.providers, settings.enabledProviders),
    [providers, settings.enabledProviders, state.providers],
  );
  const denseProviderSlots = useMemo(
    () => orderedEnabledProviderSlots(state.providers, settings.enabledProviders, sorted),
    [settings.enabledProviders, sorted, state.providers],
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
    return denseProviderSlots.map((slot) =>
      providersById.get(slot.id) ?? providerPlaceholder(slot.id, slot.displayName),
    );
  }, [denseProviderSlots, expectsDenseOverview, providersById, sorted]);
  const denseTopProviderIds = useMemo(
    () => denseProviderSlots.slice(0, 4).map((slot) => slot.id),
    [denseProviderSlots],
  );
  const denseOverviewReady =
    !expectsDenseOverview ||
    denseTopProviderIds.every((providerId) => providersById.has(providerId)) ||
    lastRefresh !== null;

  useEffect(() => {
    setSelectedProviderId(initialProviderId);
  }, [initialProviderId]);

  // Hide panel during the initial resize+reposition dance so the user
  // doesn't see the window jump around.  Revealed after first layout pass.
  const [layoutReady, setLayoutReady] = useState(false);
  const [layoutRevision, setLayoutRevision] = useState(0);
  const layoutReadyRef = useRef(false);
  const resizeRunRef = useRef(0);
  const layoutTimerRef = useRef<number | undefined>(undefined);
  const lastSizeRef = useRef<{ width: number; height: number } | null>(null);

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

  const handleMenuCardLayoutChange = useCallback(() => {
    if (layoutTimerRef.current !== undefined) {
      window.clearTimeout(layoutTimerRef.current);
    }
    layoutTimerRef.current = window.setTimeout(() => {
      setLayoutRevision((current) => current + 1);
    }, layoutReadyRef.current ? 100 : 16);
  }, []);

  const layoutContentKey = useMemo(
    () => [
      selectedProviderId ?? "overview",
      gridExpanded ? "expanded" : "collapsed",
      isRefreshing ? "refreshing" : "idle",
      updateState.status,
      updateState.version ?? "",
      updateState.error ?? "",
      expectsDenseOverview ? "dense" : "normal",
      hasLoadedCache ? "cache-ready" : "cache-pending",
      denseOverviewReady ? "dense-ready" : "dense-pending",
      visibleProviders.map((provider) => provider.providerId).join(","),
    ].join("|"),
    [
      gridExpanded,
      isRefreshing,
      selectedProviderId,
      updateState.error,
      updateState.status,
      updateState.version,
      expectsDenseOverview,
      denseOverviewReady,
      hasLoadedCache,
      visibleProviders,
    ],
  );

  useEffect(() => {
    handleMenuCardLayoutChange();
  }, [handleMenuCardLayoutChange, layoutContentKey]);

  useEffect(() => {
    const surface = document.querySelector<HTMLElement>(".menu-surface--tray");
    if (!surface || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => handleMenuCardLayoutChange());
    observer.observe(surface);
    return () => observer.disconnect();
  }, [handleMenuCardLayoutChange, sorted.length === 0]);

  useEffect(() => {
    return () => {
      if (layoutTimerRef.current !== undefined) {
        window.clearTimeout(layoutTimerRef.current);
      }
    };
  }, []);

  // Dynamically size the Tauri window to fit content, capped at the work area.
  // Measurements are debounced and skip no-op size changes to avoid the
  // visible bounce caused by resizing/reanchoring on every provider update.
  useEffect(() => {
    if (!hasLoadedCache && sorted.length === 0) {
      return;
    }

    const minHeight =
      selectedProviderId !== null
        ? TRAY_DETAIL_MIN_HEIGHT
        : expectsDenseOverview
          ? TRAY_DENSE_OVERVIEW_HEIGHT
          : TRAY_OVERVIEW_MIN_HEIGHT;

    const resize = async () => {
      const run = ++resizeRunRef.current;
      const win = getCurrentWindow();
      const surface = document.querySelector<HTMLElement>(".menu-surface--tray");
      if (!surface) return;
      const html = document.documentElement;
      const pageBody = document.body;
      const workArea = await getWorkAreaRect().catch(() => null);
      const maxHeight = Math.max(
        minHeight,
        Math.min(TRAY_MAX_MEASURE_HEIGHT, (workArea?.height ?? TRAY_MAX_MEASURE_HEIGHT) - 16),
      );

      const body = surface.querySelector<HTMLElement>(".menu-surface__body");
      const stack = surface.querySelector<HTMLElement>(".menu-stack");

      const previous = {
        htmlOverflow: html.style.overflow,
        bodyOverflow: pageBody.style.overflow,
        bodyMinHeight: pageBody.style.minHeight,
        surfaceMinHeight: surface.style.minHeight,
        surfaceHeight: surface.style.height,
        surfaceMaxHeight: surface.style.maxHeight,
        surfaceOverflow: surface.style.overflow,
        bodyInnerOverflow: body?.style.overflow,
        bodyFlex: body?.style.flex,
        stackOverflow: stack?.style.overflow,
      };
      let committedHeight = false;

      html.style.overflow = "visible";
      pageBody.style.overflow = "visible";
      pageBody.style.minHeight = "0";
      surface.style.minHeight = "0";
      surface.style.height = "auto";
      surface.style.maxHeight = "none";
      surface.style.overflow = "visible";
      if (body) { body.style.overflow = "visible"; body.style.flex = "0 0 auto"; }
      if (stack) { stack.style.overflow = "visible"; }

      const revealPanel = async () => {
      if (run !== resizeRunRef.current || !denseOverviewReady) return;
      layoutReadyRef.current = true;
      setLayoutReady(true);
      await new Promise<void>((r) => requestAnimationFrame(() => r()));
      if (run === resizeRunRef.current) {
        await Promise.resolve(revealTrayPanelWindow()).catch(() => {});
      }
      };

      try {
        if (!layoutReadyRef.current) {
          await win.setSize(new LogicalSize(TRAY_WIDTH, minHeight));
          lastSizeRef.current = { width: TRAY_WIDTH, height: minHeight };
        }

        await new Promise<void>((r) => requestAnimationFrame(() => r()));
        await new Promise<void>((r) => requestAnimationFrame(() => r()));

        if (run !== resizeRunRef.current) return;

        const surfaceRect = surface.getBoundingClientRect();
        let contentHeight = Math.max(
          surface.scrollHeight,
          Math.ceil(surfaceRect.height),
        );
        let maxBottom = surfaceRect.top + contentHeight;
        const bodyRect = body?.getBoundingClientRect();
        if (bodyRect && bodyRect.height > 0 && bodyRect.bottom > maxBottom) {
          maxBottom = bodyRect.bottom;
        }
        const footer = surface.querySelector<HTMLElement>(".menu-surface__footer");
        const footerRect = footer?.getBoundingClientRect();
        if (footerRect && footerRect.height > 0 && footerRect.bottom > maxBottom) {
          maxBottom = footerRect.bottom;
        }
        contentHeight = Math.ceil(maxBottom - surfaceRect.top) + 4;

        const height = Math.min(Math.max(contentHeight, minHeight), maxHeight);

        // Lock surface to measured content height.
        surface.style.maxHeight = `${height}px`;
        committedHeight = true;

        const previousSize = lastSizeRef.current;
        const shouldResize =
          previousSize === null ||
          previousSize.width !== TRAY_WIDTH ||
          Math.abs(previousSize.height - height) > 2;
        if (shouldResize) {
          await win.setSize(new LogicalSize(TRAY_WIDTH, height));
          lastSizeRef.current = { width: TRAY_WIDTH, height };
          await Promise.resolve(reanchorTrayPanel()).catch(() => {});
        }

        // First layout pass complete — reveal the panel.
        await revealPanel();
      } catch (error) {
        console.warn("CodexBar tray panel resize failed", error);
        // If Windows refuses a transient resize/reanchor request, prefer a
        // visible slightly-imperfect panel over an unusable invisible one.
        void revealPanel();
      } finally {
        if (!committedHeight) {
          surface.style.maxHeight = previous.surfaceMaxHeight;
        }
        surface.style.minHeight = previous.surfaceMinHeight;
        surface.style.height = previous.surfaceHeight;
        surface.style.overflow = previous.surfaceOverflow;
        html.style.overflow = previous.htmlOverflow;
        pageBody.style.overflow = previous.bodyOverflow;
        pageBody.style.minHeight = previous.bodyMinHeight;
        if (body) {
          body.style.overflow = previous.bodyInnerOverflow ?? "";
          body.style.flex = previous.bodyFlex ?? "";
        }
        if (stack) {
          stack.style.overflow = previous.stackOverflow ?? "";
        }
      }
    };

    const t0 = setTimeout(() => void resize(), layoutReadyRef.current ? 25 : 0);

    return () => {
      clearTimeout(t0);
      resizeRunRef.current += 1;
    };
  }, [
    expectsDenseOverview,
    denseOverviewReady,
    hasLoadedCache,
    layoutRevision,
    selectedProviderId,
    sorted.length,
  ]);

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

  if (sorted.length === 0) {
    return (
      <div className={`tray-panel-reveal${layoutReady && denseOverviewReady ? " tray-panel-reveal--ready" : ""}${expectsDenseOverview ? " tray-panel-reveal--dense" : ""}`}>
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
    <div className={`tray-panel-reveal${layoutReady && denseOverviewReady ? " tray-panel-reveal--ready" : ""}${expectsDenseOverview ? " tray-panel-reveal--dense" : ""}`}>
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
                  onLayoutChange={handleMenuCardLayoutChange}
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
