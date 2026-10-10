import { useCallback, useEffect, useMemo, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { BootstrapState } from "../types/bridge";
import {
  beginFlyoutGesture,
  dismissTrayPanel,
  endFlyoutGesture,
  getAppInfo,
  getProviderDetail,
  getSystemAccentColor,
  openProviderDashboard,
  openProviderStatusPage,
  openSettingsWindow,
  quitApp as quitApplication,
  reorderProviders,
} from "../lib/tauri";
import { useProviders } from "./useProviders";
import { useSettings } from "./useSettings";
import { useUpdateState } from "./useUpdateState";
import { useLocale } from "./useLocale";
import { useProviderSwitcherKeys } from "./useProviderSwitcherKeys";
import { useSurfaceTarget } from "./useSurfaceMode";
import { useTrayPanelLayout } from "./useTrayPanelLayout";
import type { MenuFooterRow } from "../components/MenuSurface";
import { hasSuccessfulClaudeCliQuota } from "../lib/claudeAccountActions";
import { orderProviderSnapshots } from "../lib/providerOrder";
import { clampTrayScalePercent } from "../lib/trayScale";
import {
  hydrateProviderSlots,
  orderedEnabledProviderSlots,
} from "../lib/trayProviders";

const TRAY_INITIAL_REFRESH_DELAY_MS = 250;
const DENSE_OVERVIEW_THRESHOLD = 32;

/** Which of a provider's web pages the backend can open. */
interface ProviderLinks {
  providerId: string;
  dashboard: boolean;
  statusPage: boolean;
}

/**
 * Controller for the tray flyout surface — state, memos, effects, and
 * handlers. JSX stays in `TrayPanel`.
 */
export function useTrayPanelController(state: BootstrapState) {
  const { settings } = useSettings(state.settings);
  const {
    providers,
    isRefreshing,
    refreshingProviderIds,
    refresh,
    hasCachedData,
    hasLoadedCache,
  } = useProviders({
    initialRefreshDelayMs: TRAY_INITIAL_REFRESH_DELAY_MS,
    forceRefreshOnMount: settings.refreshAllProvidersOnMenuOpen,
  });
  const { updateState, checkNow, download, apply, dismiss, openRelease } =
    useUpdateState();

  const { t } = useLocale();
  const surfaceTarget = useSurfaceTarget("trayPanel");

  // Settings > Menu > Panel scale, applied as CSS zoom on the panel.
  const trayScale = clampTrayScalePercent(settings.trayScalePercent) / 100;

  const [appVersion, setAppVersion] = useState<string | null>(null);
  const [accentColor, setAccentColor] = useState<string | null>(null);
  useEffect(() => {
    let cancelled = false;
    void getAppInfo().then(
      (info) => {
        if (!cancelled) setAppVersion(info.version);
      },
      () => {},
    );
    void getSystemAccentColor().then(
      (color) => {
        if (!cancelled) setAccentColor(color);
      },
      () => {},
    );
    return () => {
      cancelled = true;
    };
  }, []);

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

  // What the switcher grid displays: the dense overview shows hydrated slots
  // (with placeholders); everything else shows the sorted providers.
  const gridProviders = expectsDenseOverview ? denseTrayProviders : sorted;

  useEffect(() => {
    setSelectedProviderId(initialProviderId);
  }, [initialProviderId]);

  // Cards to display based on mode
  // Overview: all providers in the grid — non-error first, then errors
  // Detail: only the selected provider's card (macOS shows single provider)
  const visibleProviders = useMemo(() => {
    if (selectedProviderId === null) {
      // Overview: show providers in the same Settings/catalog order as the grid.
      if (sorted.length + 1 > DENSE_OVERVIEW_THRESHOLD && !gridExpanded) {
        return denseTrayProviders.slice(0, 4);
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

  // The account and link rows act on the selected provider, or on the only
  // one: macOS shows them on a provider tab, and with a single provider there
  // is no overview tab.
  const actionProvider =
    selectedProviderId !== null
      ? (providersById.get(selectedProviderId) ?? null)
      : sorted.length === 1
        ? sorted[0]
        : null;
  const actionProviderId = actionProvider?.providerId ?? null;
  const [providerLinks, setProviderLinks] = useState<ProviderLinks | null>(null);
  useEffect(() => {
    if (actionProviderId === null) return;
    let cancelled = false;
    void getProviderDetail(actionProviderId)
      .then(
        (detail) => ({
          dashboard: Boolean(detail.dashboardUrl),
          statusPage: Boolean(detail.statusPageUrl),
        }),
        () => ({ dashboard: false, statusPage: false }),
      )
      .then((links) => {
        if (!cancelled) setProviderLinks({ providerId: actionProviderId, ...links });
      });
    return () => {
      cancelled = true;
    };
  }, [actionProviderId]);
  // Links resolved for a previously selected provider never show.
  const links =
    providerLinks !== null && providerLinks.providerId === actionProviderId
      ? providerLinks
      : null;

  const openSettings = useCallback(() => {
    void openSettingsWindow("general").finally(() => {
      void getCurrentWindow().close();
    });
  }, []);
  const openAbout = useCallback(() => {
    void openSettingsWindow("about").finally(() => {
      void getCurrentWindow().close();
    });
  }, []);
  const quitApp = useCallback(() => {
    void quitApplication();
  }, []);

  const actionRows: MenuFooterRow[] = [];
  if (actionProvider !== null && hasSuccessfulClaudeCliQuota(actionProvider)) {
    actionRows.push({
      id: "switchAccount",
      label: t("TrayMenuSwitchAccount"),
      onClick: () => void openSettingsWindow("providers").catch(() => {}),
    });
  }
  if (links?.dashboard) {
    const { providerId } = links;
    actionRows.push({
      id: "dashboard",
      label: t("TrayMenuUsageDashboard"),
      onClick: () => void openProviderDashboard(providerId).catch(() => {}),
    });
  }
  if (links?.statusPage) {
    const { providerId } = links;
    actionRows.push({
      id: "statusPage",
      label: t("TrayMenuStatusPage"),
      onClick: () => void openProviderStatusPage(providerId).catch(() => {}),
    });
  }
  const footerGroups: MenuFooterRow[][] = [
    actionRows,
    [
      { id: "refresh", label: t("ActionRefresh"), shortcut: "Ctrl+R", onClick: refresh },
      { id: "settings", label: t("MenuSettings"), shortcut: "Ctrl+,", onClick: openSettings },
      {
        id: "about",
        label: appVersion
          ? t("MenuAboutVersion").replace("{}", appVersion)
          : t("MenuAbout"),
        onClick: openAbout,
      },
      { id: "quit", label: t("MenuQuit"), shortcut: "Ctrl+Q", onClick: quitApp },
    ],
  ];
  const actionRowIds = actionRows.map((row) => row.id).join(",");

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
        actionRowIds,
        trayScale,
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
      actionRowIds,
      trayScale,
    ],
  );

  const { layoutReady, requestLayout } = useTrayPanelLayout({
    canMeasure: hasLoadedCache || sorted.length > 0,
    denseOverview: expectsDenseOverview,
    detailMode: selectedProviderId !== null,
    layoutKey,
    zoom: trayScale,
  });

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
  const gridProviderIds = useMemo(
    () => gridProviders.map((provider) => provider.providerId),
    [gridProviders],
  );
  useProviderSwitcherKeys({
    providerIds: gridProviderIds,
    selectedProviderId,
    onSelect: handleGridClick,
    shortcuts: settings.switcherShortcuts,
  });
  const handleReorder = useCallback((orderedIds: string[]) => {
    void reorderProviders(orderedIds).catch(() => {});
  }, []);
  const handleGestureStart = useCallback(() => {
    void beginFlyoutGesture().catch(() => {});
  }, []);
  const handleGestureEnd = useCallback(() => {
    void endFlyoutGesture().catch(() => {});
  }, []);

  const revealClassName = `tray-panel-reveal${layoutReady ? " tray-panel-reveal--ready" : ""}`;

  return {
    t,
    settings,
    isRefreshing,
    refreshingProviderIds,
    refresh,
    hasCachedData,
    trayScale,
    accentColor,
    sorted,
    gridProviders,
    selectedProviderId,
    gridExpanded,
    setGridExpanded,
    visibleProviders,
    layoutReady,
    requestLayout,
    footerGroups,
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
  };
}
