import { afterEach, beforeEach, vi } from "vitest";
import { cleanup, render } from "@testing-library/react";
import TrayPanel from "../surfaces/TrayPanel";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "./localeHarness";
import type {
  BootstrapState,
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsSnapshot,
} from "../types/bridge";
import { makeRateWindow, makeSettings, makeUsageSnapshot } from "./fixtures";
import { tauriMocks, eventMocks, windowMocks } from "./trayPanelMocks";

export function provider(id: string, displayName: string, used = 20): ProviderUsageSnapshot {
  return makeUsageSnapshot(id, {
    displayName,
    primary: makeRateWindow(used),
    selectedMetric: makeRateWindow(used),
    primaryLabel: "Monthly",
    fetchDurationMs: null,
  });
}

export function providerWithThreeQuotaWindows(
  id: string,
  displayName: string,
): ProviderUsageSnapshot {
  const snapshot = provider(id, displayName);
  snapshot.secondary = makeRateWindow(35);
  snapshot.secondaryLabel = "Weekly";
  snapshot.tertiary = makeRateWindow(50);
  snapshot.tertiaryLabel = "Monthly";
  return snapshot;
}


export function bootstrap(
  settingsOverrides: Partial<SettingsSnapshot> = {},
  catalog: ProviderCatalogEntry[] = [],
): BootstrapState {
  return {
    contractVersion: "v1",
    providers: catalog,
    settings: makeSettings(settingsOverrides),
  };
}

export function renderTrayPanel(
  providers: ProviderUsageSnapshot[],
  settingsOverrides: Partial<SettingsSnapshot> = {},
  catalog: ProviderCatalogEntry[] = [],
) {
  tauriMocks.getCachedProviders.mockResolvedValue(providers);
  const snapshot = makeSettings(settingsOverrides);
  tauriMocks.getSettingsSnapshot.mockResolvedValue(snapshot);
  return render(
    <LocaleProvider>
      <TrayPanel
        state={{
          ...bootstrap(settingsOverrides, catalog),
          settings: snapshot,
        }}
      />
    </LocaleProvider>,
  );
}

export function emitEvent(event: string, payload: unknown) {
  for (const listener of eventMocks.listeners.get(event) ?? []) {
    listener({ payload });
  }
}

export function setupTrayPanelTests() {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.listeners.clear();
    tauriMocks.claudeReconciliationState.mockResolvedValue(null);
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue(null);
    tauriMocks.getUsageSpendSummary.mockResolvedValue({ rows: [], models: [] });
    tauriMocks.flyoutStoredSize.mockResolvedValue(null);
    tauriMocks.beginFlyoutGesture.mockResolvedValue(undefined);
    tauriMocks.resetFlyoutPosition.mockResolvedValue(undefined);
    windowMocks.startDragging.mockResolvedValue(undefined);
    windowMocks.startResizeDragging.mockResolvedValue(undefined);
    tauriMocks.refreshProviders.mockResolvedValue(undefined);
    tauriMocks.refreshProvidersIfStale.mockResolvedValue(undefined);
    tauriMocks.dismissTrayPanel.mockResolvedValue(undefined);
    tauriMocks.reanchorTrayPanel.mockResolvedValue(undefined);
    tauriMocks.getWorkAreaRect.mockResolvedValue({
      x: 0,
      y: 0,
      width: 1440,
      height: 900,
    });
    tauriMocks.getCurrentSurfaceState.mockResolvedValue({
      mode: "trayPanel",
      target: { kind: "summary" },
    });
    tauriMocks.getSettingsSnapshot.mockResolvedValue(makeSettings());
    tauriMocks.updateSettings.mockResolvedValue(makeSettings());
    tauriMocks.getUpdateState.mockResolvedValue({
      status: "idle",
      version: null,
      error: null,
      progress: null,
      releaseUrl: null,
      canDownload: false,
      canApply: false,
      lastCheckedAt: null,
    });
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "codex",
      costHistory: [],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: null,
    });
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        ActionRefresh: "Refresh",
        OverviewSpendProviderCoverage: "{} of {} providers have spend",
        MenuAbout: "About CodexBar",
        MenuQuit: "Quit",
        MenuSettings: "Settings...",
        ActionUsageDashboard: "Usage Dashboard",
        ActionStatusPage: "Status Page",
        PanelAllProviders: "All providers",
        PanelAllProvidersShort: "All",
        PanelLeftSuffix: "left",
        PanelShowAllProviders: "Show all providers",
        PanelShowFewerProviders: "Show fewer providers",
        PanelUsedSuffix: "used",
        PanelZoom: "Zoom",
      }),
    );
    eventMocks.listen.mockImplementation(
      (event: string, handler: (event: { payload: unknown }) => void) => {
        const listeners = eventMocks.listeners.get(event) ?? [];
        listeners.push(handler);
        eventMocks.listeners.set(event, listeners);
        return Promise.resolve(() => {});
      },
    );
  });
  afterEach(() => {
    // Unmount before restoring mocks: a still-mounted panel can re-run an
    // effect against a restored (undefined-returning) mock.
    cleanup();
    vi.restoreAllMocks();
  });
}
