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
    credentialExpiryNotificationsEnabled: false,
    trayIconMode: "single",
    switcherShowsIcons: true,
    menuBarShowsHighestUsage: false,
    menuBarShowsPercent: false,
    menuBarColorPace: false,
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
    uiLanguage: "english",
    theme: "dark",
    windowScalePercent: 125,
    trayScalePercent: 100,
    trayPanelAlwaysOnTop: false,
    powertoysStatusPipeEnabled: false,
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

/** The pages `get_provider_detail` reports for a provider with both links. */
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
        TrayMenuUsageDashboard: "Usage Dashboard",
        TrayMenuStatusPage: "Status Page",
        PanelAllProviders: "All providers",
        PanelAllProvidersShort: "All",
        PanelLeftSuffix: "left",
        PanelShowAllProviders: "Show all providers",
        PanelShowFewerProviders: "Show fewer providers",
        PanelUsedSuffix: "used",
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
    // TrayPanel is now hosted exclusively in the dedicated `flyout` OS
    // window (see App.tsx's isFlyoutWindow() routing), so it must not depend
    // on `main`'s surface-mode machine to know it's "open" — that machine
    // can report something other than "trayPanel". Overriding the snapshot
    // mock to another mode confirms the reveal gate is not wired to
    // useSurfaceMode() at all.
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
    // The overview of several providers has no provider to act on.
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

    expect(tauriMocks.openSettingsWindow).toHaveBeenCalledWith("providers");
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
    expect(container.querySelector(".menu-card__subtitle")?.textContent).toContain("日前");
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
      Array.from(container.querySelectorAll(".menu-card__name")).map(
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
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(
      providers.length,
    );
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
    // Pins the one dashboard layout: tray-variant surface, icon-first
    // provider switcher, and the Refresh / Settings... / About / Quit rows
    // with their Ctrl shortcuts. The retired PopOut layout had a "CodexBar"
    // title bar with window controls.
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

    // Width 310 × 1.5 = 465. Height 505 × 1.5 = 757.5, rounded up to 758,
    // plus 1 px for DPI rounding = 759.
    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 465, height: 759 }),
      );
    });
    first.unmount();
    setSize.mockClear();

    // Same zoom, taller content: 700 × 1.5 + 1 = 1051 exceeds the mocked
    // work-area cap (900 - 16 = 884), so the clamp still wins.
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
});
