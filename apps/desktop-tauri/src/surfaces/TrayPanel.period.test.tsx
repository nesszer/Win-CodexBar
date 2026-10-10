import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getCachedProviders: vi.fn(),
  refreshProviders: vi.fn(),
  refreshProvidersIfStale: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  updateSettings: vi.fn(),
  getUpdateState: vi.fn(),
  checkForUpdates: vi.fn(),
  downloadUpdate: vi.fn(),
  applyUpdate: vi.fn(),
  dismissUpdate: vi.fn(),
  openReleasePage: vi.fn(),
  setSurfaceMode: vi.fn(),
  dismissTrayPanel: vi.fn(),
  beginFlyoutGesture: vi.fn().mockResolvedValue(undefined),
  endFlyoutGesture: vi.fn().mockResolvedValue(undefined),
  openSettingsWindow: vi.fn(),
  quitApp: vi.fn(),
  getWorkAreaRect: vi.fn(),
  reanchorTrayPanel: vi.fn(),
  revealTrayPanelWindow: vi.fn(),
  flyoutStoredSize: vi.fn().mockResolvedValue(null),
  setFlyoutSize: vi.fn().mockResolvedValue(undefined),
  openProviderDashboard: vi.fn(),
  openProviderStatusPage: vi.fn(),
  getProviderChartData: vi.fn(),
  getCurrentSurfaceState: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
  getDeepSeekPricingStatus: vi.fn().mockResolvedValue(null),
  getUsageSpendSummary: vi.fn(),
  claudeReconciliationState: vi.fn().mockResolvedValue(null),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

const windowMocks = vi.hoisted(() => ({
  getCurrentWindow: vi.fn(() => ({
    setSize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    scaleFactor: vi.fn().mockResolvedValue(1),
    onResized: vi.fn().mockResolvedValue(() => {}),
    innerSize: vi.fn().mockResolvedValue({ width: 328, height: 200 }),
  })),
  LogicalSize: vi.fn((width: number, height: number) => ({ width, height })),
  PhysicalSize: vi.fn((width: number, height: number) => ({ width, height })),
}));

vi.mock("../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/api/event", () => eventMocks);
vi.mock("@tauri-apps/api/window", () => windowMocks);

import TrayPanel from "./TrayPanel";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import type { ProviderUsageSnapshot, SettingsSnapshot } from "../types/bridge";
import { makeRateWindow, makeUsageSnapshot } from "../test/fixtures";

const codex: ProviderUsageSnapshot = makeUsageSnapshot("codex", {
  displayName: "Codex",
  primary: makeRateWindow(35),
  selectedMetric: makeRateWindow(35),
  primaryLabel: "Monthly",
  fetchDurationMs: null,
});

const settings = {
  enabledProviders: ["codex"],
  refreshIntervalSecs: 300,
  adaptiveRefresh: false,
  refreshAllProvidersOnMenuOpen: false,
  lowPowerMode: false,
  highUsageThreshold: 70,
  criticalUsageThreshold: 90,
  trayIconMode: "single",
  switcherShowsIcons: true,
  showAsUsed: true,
  resetTimeRelative: true,
  overviewLayout: "detailed",
  hidePersonalInfo: false,
  uiLanguage: "english",
  theme: "dark",
  windowScalePercent: 125,
  trayScalePercent: 100,
  trayPanelAlwaysOnTop: false,
  providerMetrics: {},
  costSummaryDisplayStyle: "compact",
  costReportingPeriod: "rolling:7",
  providerAccentColors: {},
} as unknown as SettingsSnapshot;

describe("TrayPanel History window", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getCachedProviders.mockResolvedValue([codex]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings);
    tauriMocks.updateSettings.mockResolvedValue(settings);
    tauriMocks.refreshProviders.mockResolvedValue(undefined);
    tauriMocks.refreshProvidersIfStale.mockResolvedValue(undefined);
    tauriMocks.dismissTrayPanel.mockResolvedValue(undefined);
    tauriMocks.reanchorTrayPanel.mockResolvedValue(undefined);
    tauriMocks.getWorkAreaRect.mockResolvedValue({ x: 0, y: 0, width: 1440, height: 900 });
    tauriMocks.getCurrentSurfaceState.mockResolvedValue({
      mode: "trayPanel",
      target: { kind: "summary" },
    });
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
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({}));
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("totals the Overview spend over the selected History window", async () => {
    tauriMocks.getUsageSpendSummary.mockResolvedValue({
      contract: {},
      reportingPeriod: "rolling:7",
      reportingDay: "2026-09-29",
      dashboardTimezone: "UTC",
      rows: [
        {
          providerId: "codex",
          displayName: "Codex",
          sevenDay: 9,
          thirtyDay: 2,
          periodCost: 9,
          periodTokens: 900,
          currency: "USD",
          source: "local",
          includedInOverview: true,
        },
      ],
    });

    render(
      <LocaleProvider>
        <TrayPanel state={{ contractVersion: "v1", providers: [], settings }} />
      </LocaleProvider>,
    );

    expect(await screen.findByText("OverviewSpendPeriodTitle")).toBeInTheDocument();
    expect(screen.getByText("$9.00")).toBeInTheDocument();
    expect(screen.queryByText("$2.00")).not.toBeInTheDocument();
    // The scan carries no explicit window: the backend resolves the saved one.
    expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalledWith();
  });
});
