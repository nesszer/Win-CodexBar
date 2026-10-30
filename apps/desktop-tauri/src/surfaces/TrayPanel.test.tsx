import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getCachedProviders: vi.fn(),
  refreshProviders: vi.fn(),
  refreshProvidersIfStale: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  getStayAwakeStatus: vi.fn().mockResolvedValue(false),
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
  getAppInfo: vi.fn(),
  getSystemAccentColor: vi.fn(),
  getProviderDetail: vi.fn(),
  openProviderDashboard: vi.fn(),
  openProviderStatusPage: vi.fn(),
  getProviderStatuses: vi.fn(),
  getProviderChartData: vi.fn(),
  getCurrentSurfaceState: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
  getDeepSeekPricingStatus: vi.fn().mockResolvedValue(null),
  getUsageSpendSummary: vi.fn(),
  getCodexAccountsState: vi.fn(),
  getTokenAccounts: vi.fn(),
  getProviderAccountUsage: vi.fn(),
  claudeReconciliationState: vi.fn().mockResolvedValue(null),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listeners: new Map<string, Array<(event: { payload: unknown }) => void>>(),
}));

const windowMocks = vi.hoisted(() => ({
  // Loosely typed so tests can swap in windows with only the methods they use.
  getCurrentWindow: vi.fn((): Record<string, unknown> => ({
    setSize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
    innerSize: vi.fn().mockResolvedValue({ width: 310, height: 200 }),
  })),
  LogicalSize: vi.fn((width: number, height: number) => ({ width, height })),
  PhysicalSize: vi.fn((width: number, height: number) => ({ width, height })),
}));

vi.mock("../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/api/event", () => eventMocks);
vi.mock("@tauri-apps/api/window", () => windowMocks);

import TrayPanel from "./TrayPanel";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { TEST_PROVIDER_CATALOG } from "../test/providerCatalog";
import { buildBundle } from "../test/localeHarness";
import type {
  BootstrapState,
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsSnapshot,
} from "../types/bridge";

function rateWindow(used: number) {
  return {
    usedPercent: used,
    remainingPercent: 100 - used,
    windowMinutes: null,
    resetsAt: null,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
  };
}

function provider(id: string, displayName: string, used = 20): ProviderUsageSnapshot {
  return {
    providerId: id,
    displayName,
    primary: rateWindow(used),
    selectedMetric: rateWindow(used),
    primaryLabel: "Monthly",
    secondary: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "auto",
    updatedAt: "2026-05-24T00:00:00Z",
    error: null,
    errorState: "ready",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    fetchDurationMs: null,
  };
}

function providerWithThreeQuotaWindows(
  id: string,
  displayName: string,
): ProviderUsageSnapshot {
  const snapshot = provider(id, displayName);
  snapshot.secondary = rateWindow(35);
  snapshot.secondaryLabel = "Weekly";
  snapshot.tertiary = rateWindow(50);
  snapshot.tertiaryLabel = "Monthly";
  return snapshot;
}

function settings(overrides: Partial<SettingsSnapshot> = {}): SettingsSnapshot {
  return {
    enabledProviders: ["codex", "claude"],
    refreshIntervalSecs: 300,
    adaptiveRefresh: false,
    refreshAllProvidersOnMenuOpen: false,
    statusChecksEnabled: true,
  lowPowerMode: false,
    startAtLogin: false,
    startMinimized: false,
    showNotifications: true,
    soundEnabled: true,
    notificationSoundTheme: "windows",
    notificationSoundPaths: {
      predictiveWarning: null,
      highUsage: null,
      criticalUsage: null,
      exhausted: null,
      statusIssue: null,
      sessionDepleted: null,
      sessionRestored: null,
    },
    highUsageThreshold: 70,
    criticalUsageThreshold: 90,
    predictivePaceWarningEnabled: false,
    predictivePaceWarningLeadHours: 0,
    credentialExpiryNotificationsEnabled: false,
    trayIconMode: "single",
    switcherShowsIcons: true,
    menuBarShowsHighestUsage: false,
    menuBarShowsPercent: false,
    menuBarIconStyle: "bars",
    mergedOverviewSelectedProviders: [],
    mergedOverviewAllAccounts: false,
    quotaWarningMarkersVisible: true,
    workdayTickStyle: "subtle",
    menuBarColorPace: false,
    menuBarColorByProvider: false,
    menuBarPaceReservePositive: false,
    providersSortedAlphabetically: false,
    showAsUsed: true,
    showAllTokenAccountsInMenu: false,
    enableAnimations: true,
    resetTimeRelative: true,
    showResetWhenExhausted: false,
    menuBarDisplayMode: "detailed",
    overviewLayout: "detailed",
    hidePersonalInfo: false,
    updateChannel: "stable",
    autoDownloadUpdates: false,
    installUpdatesOnQuit: false,
    globalShortcut: "Ctrl+Shift+U",
    switcherShortcuts: {},
    codexCustomSessionsDirs: [],
    antigravityAdditionalProfileHomes: [],
    uiLanguage: "english",
    uiLanguageFollowsSystem: false,
    theme: "dark",
    windowScalePercent: 125,
    trayScalePercent: 100,
    trayPanelAlwaysOnTop: false,
    powertoysStatusPipeEnabled: false,
    shareUsageWithCli: false,
    claudeAvoidKeychainPrompts: false,
    codexSparkUsageVisible: true,
    disableKeychainAccess: false,
    providerMetrics: {},
    floatBarEnabled: false,
    floatBarOpacity: 80,
    floatBarScale: 100,
    floatBarOrientation: "horizontal",
    floatBarStyle: "floating",
    floatBarClickThrough: false,
    floatBarProviderIds: [],
    floatBarDarkText: false,
    floatBarShowResetInline: false,
    floatBarShowCost: false,
    claudeDailyRoutinesUsageVisible: true,
    claudeAllowReadingClaudeCodeCredentials: false,
    alibabaTokenPlanRegion: "cn",
    weeklyProgressWorkDays: null,
    costSummaryDisplayStyle: "compact",
    providerAccentColors: {},
    ...overrides,
  };
}

function bootstrap(
  settingsOverrides: Partial<SettingsSnapshot> = {},
  catalog: ProviderCatalogEntry[] = [],
): BootstrapState {
  return {
    contractVersion: "v1",
    providers: catalog,
    settings: settings(settingsOverrides),
  };
}

/** The first `count` test providers, enabled and ordered as in the catalog. */
function overviewOrder(count: number): Partial<SettingsSnapshot> {
  const ids = TEST_PROVIDER_CATALOG.slice(0, count).map(([id]) => id);
  return { enabledProviders: ids, providerOrder: ids };
}

function overviewCatalog(count: number): ProviderCatalogEntry[] {
  return TEST_PROVIDER_CATALOG.slice(0, count).map(([id, displayName]) => ({
    id,
    displayName,
    cookieDomain: null,
  }));
}

function renderTrayPanel(
  providers: ProviderUsageSnapshot[],
  settingsOverrides: Partial<SettingsSnapshot> = {},
  catalog: ProviderCatalogEntry[] = [],
) {
  tauriMocks.getCachedProviders.mockResolvedValue(providers);
  const snapshot = settings(settingsOverrides);
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

function providerLinks(id: string) {
  return {
    dashboardUrl: `https://${id}.example/usage`,
    statusPageUrl: `https://status.${id}.example`,
  };
}

function emitEvent(event: string, payload: unknown) {
  for (const listener of eventMocks.listeners.get(event) ?? []) {
    listener({ payload });
  }
}

describe("TrayPanel provider grid", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.listeners.clear();
    tauriMocks.claudeReconciliationState.mockResolvedValue(null);
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue(null);
    tauriMocks.getUsageSpendSummary.mockResolvedValue({ rows: [], models: [] });
    tauriMocks.beginFlyoutGesture.mockResolvedValue(undefined);
    tauriMocks.getAppInfo.mockResolvedValue({
      name: "CodexBar",
      version: "0.70.0",
      buildNumber: "1",
      updateChannel: "stable",
      tagline: "",
    });
    tauriMocks.getSystemAccentColor.mockResolvedValue(null);
    tauriMocks.getProviderDetail.mockResolvedValue({
      dashboardUrl: null,
      statusPageUrl: null,
    });
    tauriMocks.openProviderDashboard.mockResolvedValue(undefined);
    tauriMocks.openProviderStatusPage.mockResolvedValue(undefined);
    tauriMocks.getProviderStatuses.mockResolvedValue([]);
    tauriMocks.openSettingsWindow.mockResolvedValue(undefined);
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
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());
    tauriMocks.updateSettings.mockResolvedValue(settings());
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
        MenuAboutVersion: "About CodexBar (v{})",
        MenuQuit: "Quit",
        MenuSettings: "Settings...",
        PanelMenu: "Menu",
        TrayMenuSwitchAccount: "Switch Account...",
        AddAccountEllipsis: "Add Account...",
        TrayMenuUsageDashboard: "Usage Dashboard",
        TrayMenuStatusPage: "Status Page",
        PanelAllProviders: "All providers",
        PanelAllProvidersShort: "All",
        PanelLeftSuffix: "left",
        PanelShowAllProviders: "Show all providers",
        PanelShowFewerProviders: "Show fewer providers",
        PanelUsedSuffix: "used",
        ServiceStatusPartialOutage: "Partial outage",
        UpdatedMinutesAgo: "{} minutes ago",
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

  it("reveals regardless of the shared surface-mode snapshot (TrayPanel now runs in its own dedicated window)", async () => {
    tauriMocks.getCurrentSurfaceState.mockResolvedValue({
      mode: "settings",
      target: { kind: "settings", tab: "general" },
    });

    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
  });

  it("shows only included overview spend rows without an export button", async () => {
    tauriMocks.getUsageSpendSummary.mockResolvedValue({
      contract: {},
      reportingPeriod: "rolling:30",
      reportingDay: "2026-09-19",
      dashboardTimezone: "UTC",
      rows: [
        {
          providerId: "codex",
          displayName: "Codex",
          sevenDay: 1,
          thirtyDay: 2,
          periodCost: 2,
          periodTokens: null,
          currency: "USD",
          source: "local",
          includedInOverview: true,
        },
        {
          providerId: "claude",
          displayName: "Claude",
          sevenDay: 3,
          thirtyDay: 4,
          periodCost: 4,
          periodTokens: null,
          currency: "USD",
          source: "hidden",
          includedInOverview: false,
        },
        // A known subtotal is a partial estimate: counted for coverage, not in the total.
        {
          providerId: "antigravity",
          displayName: "Antigravity",
          sevenDay: null,
          thirtyDay: null,
          periodCost: null,
          periodTokens: null,
          thirtyDayEstimate: {
            knownSubtotalUsd: 9,
            coverage: { priced: 0, unpriced: 1, unmetered: 0, estimated: 1 },
          },
          currency: "USD",
          source: "known subtotal",
          includedInOverview: true,
        },
      ],
    });

    renderTrayPanel([provider("codex", "Codex", 35)]);

    const expectedTotal = `~${new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: 2,
    }).format(2)}`;
    expect(await screen.findByText((_, element) =>
      element?.tagName === "STRONG" && element.textContent === expectedTotal,
    )).toBeInTheDocument();
    expect(screen.getByText(/1 of 2 providers have spend/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "UsageSpendShare" })).not.toBeInTheDocument();
  });

  it("hides overview spend when Track costs is off", async () => {
    tauriMocks.getUsageSpendSummary.mockResolvedValue({
      contract: {},
      reportingPeriod: "rolling:30",
      reportingDay: "2026-09-19",
      dashboardTimezone: "UTC",
      costTrackingEnabled: false,
      rows: [
        {
          providerId: "codex",
          displayName: "Codex",
          sevenDay: 1,
          thirtyDay: 2,
          periodCost: 2,
          periodTokens: null,
          currency: "USD",
          source: "local",
          includedInOverview: true,
        },
      ],
    });

    const { container } = renderTrayPanel([provider("codex", "Codex", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    await waitFor(() => expect(tauriMocks.getUsageSpendSummary).toHaveBeenCalled());
    await Promise.resolve();
    expect(screen.queryByText(/providers have spend/)).not.toBeInTheDocument();
    expect(container.textContent).not.toMatch(/\$2\.00/);
  });

  it("follows Track costs live: off hides Overview spend and re-reads the cards, on restores them", async () => {
    const onSummary = {
      contract: {},
      reportingPeriod: "rolling:30",
      reportingDay: "2026-09-19",
      dashboardTimezone: "UTC",
      costTrackingEnabled: true,
      rows: [
        {
          providerId: "codex",
          displayName: "Codex",
          sevenDay: 1,
          thirtyDay: 2,
          periodCost: 2,
          periodTokens: null,
          currency: "USD",
          source: "local",
          includedInOverview: true,
        },
      ],
    };
    tauriMocks.getUsageSpendSummary.mockResolvedValue(onSummary);
    const { container } = renderTrayPanel([provider("codex", "Codex", 35)], { costUsageEnabled: true });
    const total = new Intl.NumberFormat(undefined, {
      style: "currency",
      currency: "USD",
      maximumFractionDigits: 2,
    }).format(2);
    const shown = () =>
      /providers have spend/.test(container.textContent ?? "")
      && [...container.querySelectorAll("strong")].some((e) => e.textContent?.includes(total));

    await waitFor(() => expect(shown()).toBe(true));
    await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(1));
    const summaryCalls = tauriMocks.getUsageSpendSummary.mock.calls.length;

    // Track costs turned off in the Settings window: the flyout only hears settings-changed.
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings({ costUsageEnabled: false }));
    await act(async () => emitEvent("settings-changed", null));
    await waitFor(() => expect(shown()).toBe(false));
    await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(2));
    expect(tauriMocks.getUsageSpendSummary.mock.calls.length).toBe(summaryCalls);

    // Turned on again: the summary and the card chart are read again.
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings({ costUsageEnabled: true }));
    await act(async () => emitEvent("settings-changed", null));
    await waitFor(() => expect(shown()).toBe(true));
    await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(3));
    // Once for Overview spend, once for the Codex card's Conversations list.
    await waitFor(() =>
      expect(tauriMocks.getUsageSpendSummary.mock.calls.length).toBe(summaryCalls + 2),
    );
  });

  it("removes a disabled provider's card as soon as settings change", async () => {
    const { container } = renderTrayPanel([
      provider("codex", "Codex", 35),
      provider("claude", "Claude", 20),
    ]);
    await waitFor(() => expect(container.querySelector("#card-codex")).not.toBeNull());
    expect(container.querySelector("#card-claude")).not.toBeNull();

    // The shell prunes the disabled provider from its cache before broadcasting.
    tauriMocks.getCachedProviders.mockResolvedValue([provider("claude", "Claude", 20)]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings({ enabledProviders: ["claude"] }));
    await act(async () => emitEvent("settings-changed", null));

    await waitFor(() => expect(container.querySelector("#card-codex")).toBeNull());
    expect(container.querySelector("#card-claude")).not.toBeNull();
  });

  it("dismisses the tray panel on unmodified Escape", async () => {
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });

    fireEvent.keyDown(window, { key: "Escape" });

    await waitFor(() => {
      expect(tauriMocks.dismissTrayPanel).toHaveBeenCalledTimes(1);
    });
  });

  it("does not dismiss the tray panel on modified Escape", async () => {
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });

    fireEvent.keyDown(window, { key: "Escape", ctrlKey: true });
    fireEvent.keyDown(window, { key: "Escape", shiftKey: true });
    fireEvent.keyDown(window, { key: "Escape", altKey: true });
    fireEvent.keyDown(window, { key: "Escape", metaKey: true });

    expect(tauriMocks.dismissTrayPanel).not.toHaveBeenCalled();
  });

  it("keeps the existing Ctrl+R tray shortcut", async () => {
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    tauriMocks.refreshProviders.mockClear();

    fireEvent.keyDown(window, { key: "r", ctrlKey: true });

    await waitFor(() => {
      expect(tauriMocks.refreshProviders).toHaveBeenCalledTimes(1);
    });
  });

  it("offers the selected provider's Usage Dashboard and Status Page rows", async () => {
    tauriMocks.getProviderDetail.mockImplementation((id: string) =>
      Promise.resolve(providerLinks(id)),
    );
    const { container } = renderTrayPanel([
      provider("claude", "Claude", 35),
      provider("codex", "Codex", 45),
    ]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    expect(screen.queryByRole("button", { name: "Usage Dashboard" })).toBeNull();
    expect(container.querySelectorAll(".menu-surface__footer-sep")).toHaveLength(1);

    fireEvent.click(screen.getByRole("button", { name: /^Claude$/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Usage Dashboard" }));
    expect(tauriMocks.openProviderDashboard).toHaveBeenLastCalledWith("claude");
    fireEvent.click(screen.getByRole("button", { name: "Status Page" }));
    expect(tauriMocks.openProviderStatusPage).toHaveBeenLastCalledWith("claude");
    expect(container.querySelectorAll(".menu-surface__footer-sep")).toHaveLength(2);

    fireEvent.click(screen.getByRole("button", { name: /^Codex$/ }));
    fireEvent.click(await screen.findByRole("button", { name: "Usage Dashboard" }));
    expect(tauriMocks.openProviderDashboard).toHaveBeenLastCalledWith("codex");
    fireEvent.click(screen.getByRole("button", { name: "Status Page" }));
    expect(tauriMocks.openProviderStatusPage).toHaveBeenLastCalledWith("codex");
  });

  it("shows the target provider's incident as a status line above the app rows", async () => {
    const { container } = renderTrayPanel([
      provider("claude", "Claude", 35),
      provider("codex", "Codex", 45),
    ]);
    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    const note = () => container.querySelector(".menu-surface__footer-note");
    expect(note()).toBeNull();

    act(() => {
      emitEvent("provider-statuses-updated", [
        {
          providerId: "codex",
          status: {
            indicator: "major",
            description: "  Elevated error rates ",
            updatedAt: Date.now() - 5 * 60_000 - 1_000,
          },
        },
        { providerId: "claude", status: { indicator: "none", description: null, updatedAt: null } },
      ]);
    });
    // Overview: the first provider in order (Codex) is the target, as on macOS.
    expect(note()).toHaveTextContent("Elevated error rates — 5 minutes ago");
    const footer = container.querySelector(".menu-surface__footer")!;
    expect(Array.from(footer.children, (el) => el.className)).toEqual([
      "menu-surface__footer-sep",
      "menu-surface__footer-note",
      "menu-surface__footer-sep",
      "menu-surface__footer-row menu-surface__footer-row--refresh",
      "menu-surface__footer-row",
      "menu-surface__footer-row",
      "menu-surface__footer-row",
    ]);

    fireEvent.click(screen.getByRole("button", { name: /^Claude$/ }));
    expect(note()).toBeNull();

    act(() => {
      emitEvent("provider-statuses-updated", [
        { providerId: "claude", status: { indicator: "minor", description: null, updatedAt: null } },
      ]);
    });
    expect(note()).toHaveTextContent(/^Partial outage$/);

    act(() => {
      emitEvent("provider-statuses-updated", []);
    });
    expect(note()).toBeNull();
  });

  it("targets the first enabled provider for the overview status line, as on macOS", async () => {
    const codexIncident = [
      {
        providerId: "codex",
        status: { indicator: "major", description: "Codex is down", updatedAt: null },
      },
    ];
    // Overview: macOS targets the first enabled provider (Claude here), not
    // the first provider that already has a snapshot.
    const overview = renderTrayPanel([provider("codex", "Codex", 45)], {
      enabledProviders: ["claude", "codex"],
      providerOrder: ["claude", "codex"],
    });
    await waitFor(() => {
      expect(overview.container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    act(() => {
      emitEvent("provider-statuses-updated", codexIncident);
    });
    expect(overview.container.querySelector(".menu-surface__footer-note")).toBeNull();

    act(() => {
      emitEvent("provider-statuses-updated", [
        ...codexIncident,
        { providerId: "claude", status: { indicator: "minor", description: null, updatedAt: null } },
      ]);
    });
    expect(overview.container.querySelector(".menu-surface__footer-note")).toHaveTextContent(
      /^Partial outage$/,
    );
  });

  it("offers the only provider's rows without selecting its tab", async () => {
    tauriMocks.getProviderDetail.mockImplementation((id: string) =>
      Promise.resolve(providerLinks(id)),
    );
    renderTrayPanel([provider("codex", "Codex", 45)]);

    fireEvent.click(await screen.findByRole("button", { name: "Usage Dashboard" }));

    expect(tauriMocks.openProviderDashboard).toHaveBeenCalledWith("codex");
  });

  it("leaves out a link the provider does not have", async () => {
    tauriMocks.getProviderDetail.mockResolvedValue({
      dashboardUrl: "https://claude.example/usage",
      statusPageUrl: null,
    });
    renderTrayPanel([provider("claude", "Claude", 35)]);

    expect(
      await screen.findByRole("button", { name: "Usage Dashboard" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Status Page" })).toBeNull();
  });

  it("never shows the previous provider's links while the next one loads", async () => {
    let resolveCodex: (detail: ReturnType<typeof providerLinks>) => void = () => {};
    tauriMocks.getProviderDetail.mockImplementation((id: string) =>
      id === "codex"
        ? new Promise((resolve) => {
            resolveCodex = resolve;
          })
        : Promise.resolve(providerLinks(id)),
    );
    renderTrayPanel([provider("claude", "Claude", 35), provider("codex", "Codex", 45)]);

    fireEvent.click(await screen.findByRole("button", { name: /^Claude$/ }));
    expect(
      await screen.findByRole("button", { name: "Usage Dashboard" }),
    ).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /^Codex$/ }));
    expect(screen.queryByRole("button", { name: "Usage Dashboard" })).toBeNull();

    await act(async () => {
      resolveCodex(providerLinks("codex"));
    });
    fireEvent.click(screen.getByRole("button", { name: "Usage Dashboard" }));
    expect(tauriMocks.openProviderDashboard).toHaveBeenLastCalledWith("codex");
  });

  it("offers Switch Account for a Claude account with CLI quota", async () => {
    renderTrayPanel([
      { ...provider("claude", "Claude", 35), hasSuccessfulClaudeCliQuota: true },
    ]);

    fireEvent.click(await screen.findByRole("button", { name: "Switch Account..." }));

    expect(tauriMocks.openSettingsWindow).toHaveBeenCalledWith("providers:claude");
  });

  it("offers Add Account... for a Codex card with no account", async () => {
    renderTrayPanel([provider("codex", "Codex", 35)]);

    fireEvent.click(await screen.findByRole("button", { name: "Add Account..." }));

    expect(tauriMocks.openSettingsWindow).toHaveBeenCalledWith("providers:codex");
  });

  it("localizes static tray panel labels in Japanese", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle(
        {
          ActionRefresh: "更新",
          MenuAbout: "CodexBar について",
          MenuQuit: "終了",
          MenuSettings: "設定...",
          PanelAllProviders: "すべてのプロバイダー",
          PanelAllProvidersShort: "すべて",
          PanelLatestTokens: "最新トークン",
          CostPeriodShortDays: "{}日",
          PanelPeriodCost: "{}間のコスト",
          PanelPeriodTokens: "{}間のトークン",
          PanelTopModelPrefix: "トップモデル",
          PanelEstimatedFromLocalLogs: "ローカルログから推定",
          MenuAboutVersion: "CodexBar について (v{})",
          UpdatedDaysAgo: "{}日前",
        },
        "japanese",
      ),
    );
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "codex",
      costHistory: [{ date: "2026-05-24", value: 1.23 }],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: {
        todayCost: null,
        thirtyDayCost: 1.23,
        thirtyDayTokens: 584_000,
        periodCost: 1.23,
        periodTokens: 584_000,
        reportingPeriod: "rolling:30",
        latestTokens: 1200,
        topModel: "gpt-5.5",
        estimateNote: "Estimated from local logs",
        tokenCostUpdatedAtMs: 1234,
      },
    });

    const { container } = renderTrayPanel([provider("codex", "Codex", 35)]);

    await waitFor(() => {
      expect(
        container.querySelector('.provider-grid__item[aria-label="すべてのプロバイダー"]'),
      ).not.toBeNull();
    });
    expect(container.querySelector(".provider-grid__item")?.textContent).toContain("すべて");
    expect(screen.getByText("更新")).toBeInTheDocument();
    expect(screen.getByText("設定...")).toBeInTheDocument();
    expect(await screen.findByText("CodexBar について (v0.70.0)")).toBeInTheDocument();
    expect(screen.getByText("終了")).toBeInTheDocument();
    expect(await screen.findByText("30日間のコスト")).toBeInTheDocument();
    expect(container.querySelector(".card-header__subtitle")?.textContent).toContain("日前");
    expect(screen.getByText("最新トークン")).toBeInTheDocument();
    expect(screen.getByText("トップモデル: gpt-5.5")).toBeInTheDocument();
    expect(screen.getByText("ローカルログから推定")).toBeInTheDocument();
  });

  it("localizes the expanded dense grid collapse label in Japanese", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle(
        {
          PanelAllProviders: "すべてのプロバイダー",
          PanelAllProvidersShort: "すべて",
          PanelShowAllProviders: "すべてのプロバイダーを表示",
          PanelShowFewerProviders: "表示を減らす",
        },
        "japanese",
      ),
    );
    const providers = TEST_PROVIDER_CATALOG.map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );

    const { container } = renderTrayPanel(providers);

    await waitFor(() => {
      expect(container.querySelector(".provider-grid--compact")).not.toBeNull();
    });

    fireEvent.click(
      container.querySelector<HTMLButtonElement>(
        '.provider-grid__item--more[aria-label="すべてのプロバイダーを表示"]',
      )!,
    );

    expect(await screen.findByText("表示を減らす")).toBeInTheDocument();
  });

  it.each([
    [1, true],
    [2, true],
    [5, true],
    [6, false],
    [12, false],
  ])("uses expected density for %i providers plus overview", async (providerCount, shouldBeSparse) => {
      const providers = [
        provider("codex", "Codex"),
        provider("claude", "Claude"),
        provider("copilot", "GitHub Copilot"),
        provider("cursor", "Cursor"),
        provider("gemini", "Gemini"),
        provider("kiro", "Kiro"),
        provider("zai", "z.ai"),
        provider("minimax", "MiniMax"),
        provider("vertexai", "Vertex AI"),
        provider("augment", "Augment"),
        provider("opencode", "OpenCode"),
        provider("kimi", "Kimi"),
      ].slice(0, providerCount);

      const { container } = renderTrayPanel(providers);

      await waitFor(() => {
        expect(container.querySelector(".provider-grid")).not.toBeNull();
      });

      const grid = container.querySelector(".provider-grid");
      expect(grid?.classList.contains("provider-grid--sparse")).toBe(
        shouldBeSparse,
      );
    },
  );

  it("only requests chart data for providers that can render charts", async () => {
    renderTrayPanel([
      provider("codex", "Codex"),
      provider("claude", "Claude"),
      provider("copilot", "GitHub Copilot"),
      provider("cursor", "Cursor"),
      provider("deepseek", "DeepSeek"),
    ]);

    await waitFor(() => {
      expect(tauriMocks.getProviderChartData).toHaveBeenCalledTimes(2);
    });

    expect(tauriMocks.getProviderChartData).toHaveBeenCalledWith("codex", undefined);
    expect(tauriMocks.getProviderChartData).toHaveBeenCalledWith("claude", undefined);
  });

  it("renders providers in settings catalog order instead of fetch completion order", async () => {
    const catalog: ProviderCatalogEntry[] = [
      { id: "codex", displayName: "Codex", cookieDomain: null },
      { id: "claude", displayName: "Claude", cookieDomain: null },
      { id: "cursor", displayName: "Cursor", cookieDomain: null },
      { id: "factory", displayName: "Factory", cookieDomain: null },
      { id: "gemini", displayName: "Gemini", cookieDomain: null },
    ];
    const providers = [
      provider("gemini", "Gemini", 10),
      provider("cursor", "Cursor", 20),
      { ...provider("codex", "Codex", 80), error: "Authentication required" },
      provider("factory", "Factory", 30),
      { ...provider("claude", "Claude", 40), error: "Claude sign-in missing" },
    ];

    const { container } = renderTrayPanel(
      providers,
      { enabledProviders: catalog.map((entry) => entry.id) },
      catalog,
    );

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(6);
    });

    const labels = Array.from(container.querySelectorAll(".provider-grid__item"))
      .map((node) => node.getAttribute("aria-label"));
    expect(labels).toEqual([
      "All providers",
      "Codex",
      "Claude",
      "Cursor",
      "Factory",
      "Gemini",
    ]);
    expect(
      Array.from(container.querySelectorAll(".card-header__name")).map(
        (node) => node.textContent,
      ),
    ).toEqual(["Codex", "Claude", "Cursor", "Factory", "Gemini"]);
  });

  it("shows all quota rows in compact Overview when explicitly selected (0.62.0 #2616)", async () => {
    const { container } = renderTrayPanel(
      [providerWithThreeQuotaWindows("codex", "Codex")],
      { overviewLayout: "compact" },
    );

    await waitFor(() => {
      expect(container.querySelector(".menu-stack__item")).not.toBeNull();
    });

    expect(container.querySelectorAll(".menu-metric")).toHaveLength(3);
  });

  it("shows the first six providers on Overview when none are chosen", async () => {
    const providers = TEST_PROVIDER_CATALOG.slice(0, 8).map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );
    const { container } = renderTrayPanel(providers, overviewOrder(8), overviewCatalog(8));

    await waitFor(() => {
      expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(6);
    });
    expect(
      Array.from(container.querySelectorAll(".card-header__name"), (node) => node.textContent),
    ).toEqual(TEST_PROVIDER_CATALOG.slice(0, 6).map(([, displayName]) => displayName));
  });

  it("shows only the chosen Overview providers, in provider order", async () => {
    const providers = TEST_PROVIDER_CATALOG.slice(0, 8).map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );
    const [first, , third, , , , seventh] = TEST_PROVIDER_CATALOG;
    const { container } = renderTrayPanel(
      providers,
      {
        ...overviewOrder(8),
        mergedOverviewSelectedProviders: [seventh[0], first[0], third[0], "not-present"],
      },
      overviewCatalog(8),
    );

    await waitFor(() => {
      expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(3);
    });
    expect(
      Array.from(container.querySelectorAll(".card-header__name"), (node) => node.textContent),
    ).toEqual([first[1], third[1], seventh[1]]);
  });

  it("collapses and expands the full provider catalog in the dense tray grid", async () => {
    const providers = TEST_PROVIDER_CATALOG.map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );

    const { container } = renderTrayPanel(providers);

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(
        20,
      );
    });

    const grid = container.querySelector(".provider-grid");
    expect(grid?.classList.contains("provider-grid--sparse")).toBe(false);
    expect(grid?.classList.contains("provider-grid--compact")).toBe(true);
    expect(grid?.getAttribute("data-expanded")).toBe("false");
    expect(grid?.getAttribute("data-provider-count")).toBe(
      String(providers.length + 1),
    );
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(4);

    const expand = container.querySelector<HTMLButtonElement>(
      '.provider-grid__item--more[aria-label="Show all providers"]',
    );
    expect(expand).not.toBeNull();
    expect(expand?.textContent).toContain(`+${providers.length - 18}`);

    fireEvent.click(expand!);

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(
        providers.length + 2,
      );
    });
    expect(grid?.getAttribute("data-expanded")).toBe("true");
    // The expanded grid lists every provider; Overview still shows the
    // Settings > Menu Bar > Overview providers default, the first six.
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(6);
    for (const [id, displayName] of TEST_PROVIDER_CATALOG) {
      expect(
        container.querySelector(`.provider-grid__item[aria-label="${displayName}"]`),
        id,
      ).not.toBeNull();
    }
  });

  it("uses compact provider labels for huge catalogs without losing full accessible labels", async () => {
    const providers = TEST_PROVIDER_CATALOG.slice(0, 36).map(
      ([id, displayName], index) => provider(id, displayName, (index * 7) % 100),
    );

    const { container } = renderTrayPanel(providers);

    await waitFor(() => {
      expect(container.querySelector(".provider-grid--compact")).not.toBeNull();
    });

    const expand = container.querySelector<HTMLButtonElement>(
      '.provider-grid__item--more[aria-label="Show all providers"]',
    );
    expect(expand).not.toBeNull();

    fireEvent.click(expand!);

    await waitFor(() => {
      expect(
        container.querySelector('.provider-grid__item[aria-label="Copilot"]'),
      ).not.toBeNull();
    });

    const copilot = container.querySelector(
      '.provider-grid__item[aria-label="Copilot"]',
    );
    expect(copilot).not.toBeNull();
    expect(copilot?.getAttribute("aria-label")).toBe("Copilot");
    expect(copilot?.querySelector(".provider-grid__label")?.textContent).toBe(
      "Copi",
    );
  });

  it("provider grid indicator follows the show-as-used setting", async () => {
    const { container, rerender } = renderTrayPanel(
      [provider("claude", "Claude", 35)],
      { showAsUsed: true },
    );

    await waitFor(() => {
      const track = container.querySelector<HTMLElement>(
        ".provider-grid__weekly-track",
      );
      expect(track?.style.getPropertyValue("--weekly-pct")).toBe("35%");
    });

    tauriMocks.getCachedProviders.mockResolvedValue([
      provider("claude", "Claude", 35),
    ]);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings({ showAsUsed: false }));
    rerender(
      <LocaleProvider>
        <TrayPanel state={bootstrap({ showAsUsed: false })} />
      </LocaleProvider>,
    );

    await waitFor(() => {
      const track = container.querySelector<HTMLElement>(
        ".provider-grid__weekly-track",
      );
      expect(track?.style.getPropertyValue("--weekly-pct")).toBe("65%");
    });
  });

  it("draws quota warning markers at the notification thresholds", async () => {
    const { container } = renderTrayPanel([provider("claude", "Claude", 20)], {
      showAsUsed: false,
      highUsageThreshold: 75,
      criticalUsageThreshold: 90,
      providerQuotaWarnings: { claude: { session: { critical: 95 } } },
    });

    await waitFor(() => {
      expect(
        Array.from(container.querySelectorAll(".menu-metric__progress-warning"), (node) =>
          node.getAttribute("x"),
        ),
      ).toEqual(["5%", "25%"]);
    });
  });

  it("hides provider grid icons when the display setting is disabled", async () => {
    const { container } = renderTrayPanel(
      [provider("codex", "Codex"), provider("claude", "Claude")],
      { switcherShowsIcons: false },
    );

    await waitFor(() => {
      expect(container.querySelector(".provider-grid")).not.toBeNull();
    });

    const grid = container.querySelector(".provider-grid");
    expect(grid?.getAttribute("data-show-icons")).toBe("false");
    expect(grid?.classList.contains("provider-grid--no-icons")).toBe(true);
    expect(container.querySelector(".provider-icon")).toBeNull();
    expect(container.querySelector(".provider-grid__icon-overview")).toBeNull();
  });

  it("renders the default tray panel layout with no legacy window chrome", async () => {
    const { container } = renderTrayPanel([
      provider("claude", "Claude", 35),
      provider("codex", "Codex", 20),
    ]);

    const menu = await screen.findByRole("navigation", { name: "Menu" });
    expect(
      await within(menu).findByRole("button", { name: "About CodexBar (v0.70.0)" }),
    ).toBeInTheDocument();

    const surface = container.querySelector(".menu-surface");
    expect(surface?.classList.contains("menu-surface--tray")).toBe(true);
    expect(container.querySelector(".menu-surface--popout")).toBeNull();
    expect(container.querySelector(".popout-titlebar")).toBeNull();
    expect(container.querySelector(".popout-scale-shell")).toBeNull();
    expect(container.querySelector(".provider-grid")).not.toBeNull();
    expect(
      within(menu)
        .getAllByRole("button")
        .map((row) => [row.textContent, row.getAttribute("aria-keyshortcuts")]),
    ).toEqual([
      ["RefreshCtrl+R", "Control+R"],
      ["Settings...Ctrl+,", "Control+,"],
      ["About CodexBar (v0.70.0)", null],
      ["QuitCtrl+Q", "Control+Q"],
    ]);
    expect(tauriMocks.setSurfaceMode).not.toHaveBeenCalled();
  });

  it("applies the saved Panel scale and the Windows accent to the panel", async () => {
    tauriMocks.getSystemAccentColor.mockResolvedValue("#c42b1c");
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)], {
      trayScalePercent: 150,
    });

    await waitFor(() => {
      expect(
        container
          .querySelector<HTMLElement>(".menu-surface--tray")
          ?.style.getPropertyValue("--mac-selection-bg"),
      ).toBe("#c42b1c");
    });
    const surface = container.querySelector<HTMLElement>(".menu-surface--tray")!;
    expect(surface.style.getPropertyValue("--mac-selection-text")).toBe("#fff");
    expect(surface.style.zoom).toBe("1.5");
  });

  it("follows a new Windows accent the next time the panel takes focus", async () => {
    tauriMocks.getSystemAccentColor.mockResolvedValue("#0078d4");
    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);
    const selectionBg = () =>
      container
        .querySelector<HTMLElement>(".menu-surface--tray")
        ?.style.getPropertyValue("--mac-selection-bg");
    await waitFor(() => expect(selectionBg()).toBe("#0078d4"));

    tauriMocks.getSystemAccentColor.mockResolvedValue("#ffb900");
    act(() => {
      window.dispatchEvent(new Event("focus"));
    });

    await waitFor(() => expect(selectionBg()).toBe("#ffb900"));
    expect(
      container
        .querySelector<HTMLElement>(".menu-surface--tray")!
        .style.getPropertyValue("--mac-selection-text"),
    ).toBe("#000");
  });

  it("reveals the tray panel if the native resize pass fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize: vi.fn().mockRejectedValue(new Error("resize failed")),
      close: vi.fn().mockResolvedValue(undefined),
      innerSize: vi.fn().mockResolvedValue({ width: 310, height: 200 }),
    });

    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    expect(warn).toHaveBeenCalledWith("CodexBar tray panel resize failed", expect.any(Error));

    warn.mockRestore();
  });

  it("does not resize the native tray window for usage-only provider updates", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      innerSize: vi.fn().mockResolvedValue({ width: 310, height: 200 }),
    });

    const { container } = renderTrayPanel([provider("claude", "Claude", 35)]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    setSize.mockClear();
    tauriMocks.reanchorTrayPanel.mockClear();

    act(() => {
      emitEvent("provider-updated", provider("claude", "Claude", 52));
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 200));
    });

    expect(setSize).not.toHaveBeenCalled();
    expect(tauriMocks.reanchorTrayPanel).not.toHaveBeenCalled();
  });

  it("reserves dense all-provider height on first layout", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      innerSize: vi.fn().mockResolvedValue({ width: 310, height: 200 }),
    });
    const denseProviders = TEST_PROVIDER_CATALOG.slice(0, 36).map(([id, displayName]) =>
      provider(id, displayName),
    );

    renderTrayPanel(denseProviders, {
      enabledProviders: denseProviders.map((snapshot) => snapshot.providerId),
    });

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 310, height: 776 }),
      );
    });
  });

  it("keeps provider detail mode tall enough for the menu rows and footer", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      innerSize: vi.fn().mockResolvedValue({ width: 310, height: 200 }),
    });
    const errorProvider = {
      ...provider("abacus", "Abacus AI", 0),
      error: "Source mode `Cli` not supported for this provider",
    };

    const { container } = renderTrayPanel([errorProvider]);

    await waitFor(() => {
      expect(container.querySelector(".tray-panel-reveal--ready")).not.toBeNull();
    });
    setSize.mockClear();

    fireEvent.click(
      container.querySelector<HTMLButtonElement>(
        '.provider-grid__item[aria-label="Abacus AI"]',
      )!,
    );

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 310, height: 420 }),
      );
    });
  });

  it("fits the window to the zoomed panel at 150% Panel scale (#265)", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
      innerSize: vi.fn().mockResolvedValue({ width: 465, height: 200 }),
    });
    // jsdom has no layout engine (scrollHeight always reads 0), so pin it
    // globally to a deterministic PRE-zoom content height: TrayPanel applies
    // `zoom: trayScale` via CSS and the hook must size the window in POST-zoom
    // px or tall cards clip below the fold (#265).
    const scrollHeight = vi
      .spyOn(Element.prototype, "scrollHeight", "get")
      .mockReturnValue(505);

    const first = renderTrayPanel([provider("codex", "Codex", 61)], {
      trayScalePercent: 150,
    });

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 465, height: 759 }),
      );
    });
    first.unmount();
    setSize.mockClear();

    scrollHeight.mockReturnValue(700);
    renderTrayPanel([provider("codex", "Codex", 61)], {
      trayScalePercent: 150,
    });

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 465, height: 884 }),
      );
    });
  });

  it("keeps the Refresh row hover highlight unless the Menu setting turns it off", async () => {
    const on = renderTrayPanel([provider("codex", "Codex", 61)]);
    await screen.findByRole("button", { name: "Refresh" });
    const surface = () => on.container.querySelector(".menu-surface")!;
    expect(surface()).not.toHaveClass("menu-surface--hover-highlight-off");
    on.unmount();

    const off = renderTrayPanel([provider("codex", "Codex", 61)], {
      highlightMenuCardsOnHover: false,
    });
    await screen.findByRole("button", { name: "Refresh" });
    expect(off.container.querySelector(".menu-surface")).toHaveClass(
      "menu-surface--hover-highlight-off",
    );
  });
});

function codexAccountRecord(id: string, emailHint: string) {
  return {
    id,
    nickname: null,
    emailHint,
    authSubject: null,
    providerAccountId: null,
    codexHomePath: `/example/${id}`,
    source: "managedByApp" as const,
    createdAt: "2026-05-01T00:00:00Z",
    updatedAt: "2026-05-01T00:00:00Z",
    lastAuthenticatedAt: null,
  };
}

function codexAccountUsage(usedPercent: number) {
  return {
    email: null,
    providerAccountId: null,
    plan: "plus",
    allowed: true,
    limitReached: false,
    primaryWindow: { usedPercent, resetAt: null, limitWindowSeconds: 18_000 },
    secondaryWindow: null,
    credits: null,
    updatedAt: "2026-05-24T00:00:00Z",
  };
}

/** Two cached managed Codex accounts: `work` has usage, `home` has none. */
function codexAccountsState() {
  return {
    accounts: [
      codexAccountRecord("acc-work", "work@example.com"),
      codexAccountRecord("acc-home", "home@example.com"),
    ],
    snapshots: { "acc-work": codexAccountUsage(42) },
    displayNames: {
      "acc-work": "work@example.com",
      "acc-home": "home@example.com",
    },
    accountOrdinals: { "acc-work": 1, "acc-home": 2 },
    needsAuthentication: {},
  };
}

describe("TrayPanel overview account rows", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.listeners.clear();
    tauriMocks.claudeReconciliationState.mockResolvedValue(null);
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue(null);
    tauriMocks.getUsageSpendSummary.mockResolvedValue({ rows: [], models: [] });
    tauriMocks.getAppInfo.mockResolvedValue({
      name: "CodexBar",
      version: "0.70.0",
      buildNumber: "1",
      updateChannel: "stable",
      tagline: "",
    });
    tauriMocks.getSystemAccentColor.mockResolvedValue(null);
    tauriMocks.getProviderDetail.mockResolvedValue({ dashboardUrl: null, statusPageUrl: null });
    tauriMocks.getProviderStatuses.mockResolvedValue([]);
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
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({ ActionRefresh: "Refresh" }));
    tauriMocks.updateSettings.mockResolvedValue(settings());
    tauriMocks.getCodexAccountsState.mockResolvedValue(codexAccountsState());
    // One saved Claude token account by default, so Claude keeps its single card.
    tauriMocks.getTokenAccounts.mockResolvedValue(
      claudeTokenAccountsBridge([["tok-work", "Team Alpha"]]),
    );
    tauriMocks.getProviderAccountUsage.mockResolvedValue(claudeProviderAccountUsage({}));
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "codex",
      costHistory: [],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: null,
    });
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
    cleanup();
  });

  /** Eight catalog providers; the default Overview shows the first six, codex among them. */
  function overviewProviders() {
    return TEST_PROVIDER_CATALOG.slice(0, 8).map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );
  }

  it("keeps one card per provider and does not read accounts while the setting is off", async () => {
    const { container } = renderTrayPanel(overviewProviders(), overviewOrder(8), overviewCatalog(8));

    await waitFor(() => {
      expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(6);
    });
    expect(container.querySelector("#card-codex")).not.toBeNull();
    expect(container.querySelector("#card-codex\\/account\\/acc-work")).toBeNull();
    expect(tauriMocks.getCodexAccountsState).not.toHaveBeenCalled();
  });

  it("expands Codex into one row per cached account, keeping the six-provider cap", async () => {
    const { container } = renderTrayPanel(
      overviewProviders(),
      { ...overviewOrder(8), mergedOverviewAllAccounts: true },
      overviewCatalog(8),
    );

    // Six providers, with Codex as two rows: rows do not take provider slots.
    await waitFor(() => {
      expect(container.querySelector("#card-codex\\/account\\/acc-work")).not.toBeNull();
    });
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(7);
    expect(container.querySelector("#card-codex")).toBeNull();
    expect(container.querySelector("#card-codex\\/account\\/acc-home")).not.toBeNull();
    expect(container.querySelectorAll(".menu-stack__item")[7]).toBeUndefined();
    expect(tauriMocks.getCodexAccountsState).toHaveBeenCalled();
    // Local-log charts are per provider; account rows never read them.
    expect(
      tauriMocks.getProviderChartData.mock.calls.some(([providerId]) => providerId === "codex"),
    ).toBe(false);
  });

  it("labels account rows generically under Hide personal information", async () => {
    const { container } = renderTrayPanel(
      overviewProviders(),
      { ...overviewOrder(8), mergedOverviewAllAccounts: true, hidePersonalInfo: true },
      overviewCatalog(8),
    );

    await waitFor(() => {
      expect(container.querySelector("#card-codex\\/account\\/acc-work")).not.toBeNull();
    });
    const work = container.querySelector("#card-codex\\/account\\/acc-work")!;
    const home = container.querySelector("#card-codex\\/account\\/acc-home")!;
    expect(work.textContent).toContain("Account 1");
    expect(home.textContent).toContain("Account 2");
    expect(container.textContent).not.toContain("work@example.com");
    expect(container.textContent).not.toContain("home@example.com");
  });

  it("shows an account with no cached usage as unavailable instead of borrowing quota", async () => {
    const { container } = renderTrayPanel(
      overviewProviders(),
      { ...overviewOrder(8), mergedOverviewAllAccounts: true },
      overviewCatalog(8),
    );

    await waitFor(() => {
      expect(container.querySelector("#card-codex\\/account\\/acc-home")).not.toBeNull();
    });
    const home = container.querySelector("#card-codex\\/account\\/acc-home")!;
    expect(home.textContent).toContain("CodexAccountsUsageUnavailable");
    expect(home.textContent).not.toContain("42%");
    const work = container.querySelector("#card-codex\\/account\\/acc-work")!;
    expect(work.textContent).not.toContain("CodexAccountsUsageUnavailable");
  });

  it("expands Claude into one row per saved token account, keeping the six-provider cap", async () => {
    tauriMocks.getTokenAccounts.mockResolvedValue(
      claudeTokenAccountsBridge([
        ["tok-work", "Team Alpha"],
        ["tok-personal", "Side Project"],
      ]),
    );
    tauriMocks.getProviderAccountUsage.mockResolvedValue(
      claudeProviderAccountUsage({ "tok-work": claudeAccountUsageEntry(22) }),
    );
    const { container } = renderTrayPanel(
      overviewProviders(),
      { ...overviewOrder(8), mergedOverviewAllAccounts: true },
      overviewCatalog(8),
    );

    await waitFor(() => {
      expect(container.querySelector("#card-claude\\/account\\/tok-personal")).not.toBeNull();
    });
    expect(container.querySelector("#card-claude")).toBeNull();
    // Six providers: Codex and Claude each add rows; the rows take no provider slot.
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(8);
    expect(container.querySelectorAll(".menu-stack__item")[8]).toBeUndefined();
    const work = container.querySelector("#card-claude\\/account\\/tok-work")!;
    expect(work.textContent).not.toContain("CodexAccountsUsageUnavailable");
    const personal = container.querySelector("#card-claude\\/account\\/tok-personal")!;
    expect(personal.textContent).toContain("CodexAccountsUsageUnavailable");
    expect(personal.textContent).not.toContain("22%");
    expect(tauriMocks.getTokenAccounts).toHaveBeenCalledWith("claude");
    expect(tauriMocks.getProviderAccountUsage).toHaveBeenCalledWith("claude");
  });

  it("labels Claude token rows generically under Hide personal information", async () => {
    tauriMocks.getTokenAccounts.mockResolvedValue(
      claudeTokenAccountsBridge([
        ["tok-work", "Team Alpha"],
        ["tok-personal", "Side Project"],
      ]),
    );
    const { container } = renderTrayPanel(
      overviewProviders(),
      { ...overviewOrder(8), mergedOverviewAllAccounts: true, hidePersonalInfo: true },
      overviewCatalog(8),
    );

    await waitFor(() => {
      expect(container.querySelector("#card-claude\\/account\\/tok-work")).not.toBeNull();
    });
    // Ordinals follow the row order, though "tok-personal" sorts first.
    expect(container.querySelector("#card-claude\\/account\\/tok-work")!.textContent).toContain(
      "Account 1",
    );
    expect(container.querySelector("#card-claude\\/account\\/tok-personal")!.textContent).toContain(
      "Account 2",
    );
    expect(container.textContent).not.toContain("Team Alpha");
    expect(container.textContent).not.toContain("Side Project");
  });

  it("keeps the single Claude card when only one token account is saved", async () => {
    const { container } = renderTrayPanel(
      overviewProviders(),
      { ...overviewOrder(8), mergedOverviewAllAccounts: true },
      overviewCatalog(8),
    );

    await waitFor(() => {
      expect(container.querySelector("#card-claude")).not.toBeNull();
    });
    expect(container.querySelector("#card-claude\\/account\\/tok-work")).toBeNull();
  });

  it("does not read Claude token accounts while the setting is off", async () => {
    tauriMocks.getTokenAccounts.mockResolvedValue(
      claudeTokenAccountsBridge([
        ["tok-work", "Team Alpha"],
        ["tok-personal", "Side Project"],
      ]),
    );
    const { container } = renderTrayPanel(overviewProviders(), overviewOrder(8), overviewCatalog(8));

    await waitFor(() => {
      expect(container.querySelector("#card-claude")).not.toBeNull();
    });
    expect(tauriMocks.getTokenAccounts).not.toHaveBeenCalled();
    expect(tauriMocks.getProviderAccountUsage).not.toHaveBeenCalled();
  });
});

function claudeTokenAccountsBridge(accounts: Array<[id: string, label: string]>) {
  return {
    providerId: "claude",
    support: {
      providerId: "claude",
      displayName: "Claude",
      title: "Claude token accounts",
      subtitle: "",
      placeholder: "",
    },
    accounts: accounts.map(([id, label], index) => ({
      id,
      label,
      addedAt: "2026-05-01T00:00:00Z",
      lastUsed: null,
      isActive: index === 0,
      claudeConfigDirectory: null,
      claudeConfigDirectoryAvailable: false,
    })),
    activeIndex: 0,
  };
}

function claudeAccountUsageEntry(usedPercent: number) {
  return {
    primary: { usedPercent, windowMinutes: 300, resetsAt: null },
    secondary: null,
    updatedAt: "2026-05-24T00:00:00Z",
  };
}

function claudeProviderAccountUsage(accounts: Record<string, ReturnType<typeof claudeAccountUsageEntry>>) {
  return { providerId: "claude", accounts };
}
