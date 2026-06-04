import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getCachedProviders: vi.fn(),
  refreshProviders: vi.fn(),
  refreshProvidersIfStale: vi.fn(),
  getSettingsSnapshot: vi.fn(),
  getUpdateState: vi.fn(),
  checkForUpdates: vi.fn(),
  downloadUpdate: vi.fn(),
  applyUpdate: vi.fn(),
  dismissUpdate: vi.fn(),
  openReleasePage: vi.fn(),
  setSurfaceMode: vi.fn(),
  openSettingsWindow: vi.fn(),
  quitApp: vi.fn(),
  getProviderChartData: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

const windowMocks = vi.hoisted(() => ({
  getCurrentWindow: vi.fn(() => ({
    setSize: vi.fn().mockResolvedValue(undefined),
    setPosition: vi.fn().mockResolvedValue(undefined),
  })),
  LogicalSize: vi.fn((width: number, height: number) => ({ width, height })),
  LogicalPosition: vi.fn((x: number, y: number) => ({ x, y })),
}));

vi.mock("../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/api/event", () => eventMocks);
vi.mock("@tauri-apps/api/window", () => windowMocks);

import PopOutPanel from "./PopOutPanel";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import { TEST_PROVIDER_CATALOG } from "../test/providerCatalog";
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
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    fetchDurationMs: null,
  };
}

function settings(): SettingsSnapshot {
  return {
    enabledProviders: ["codex", "claude"],
    refreshIntervalSecs: 300,
    startAtLogin: false,
    startMinimized: false,
    showNotifications: true,
    soundEnabled: true,
    soundVolume: 100,
    highUsageThreshold: 70,
    criticalUsageThreshold: 90,
    trayIconMode: "single",
    switcherShowsIcons: true,
    menuBarShowsHighestUsage: false,
    menuBarShowsPercent: false,
    showAsUsed: true,
    showCreditsExtraUsage: true,
    showAllTokenAccountsInMenu: false,
    surpriseAnimations: false,
    enableAnimations: true,
    resetTimeRelative: true,
    menuBarDisplayMode: "detailed",
    hidePersonalInfo: false,
    updateChannel: "stable",
    autoDownloadUpdates: false,
    installUpdatesOnQuit: false,
    globalShortcut: "Ctrl+Shift+U",
    uiLanguage: "english",
    theme: "dark",
    claudeAvoidKeychainPrompts: false,
    disableKeychainAccess: false,
    showDebugSettings: false,
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
  };
}

function bootstrap(catalog: ProviderCatalogEntry[] = []): BootstrapState {
  return {
    contractVersion: "v1",
    surfaceModes: [],
    commands: [],
    events: [],
    providers: catalog,
    settings: settings(),
  };
}

function renderPopOut(
  providers: ProviderUsageSnapshot[],
  providerId?: string,
  catalog: ProviderCatalogEntry[] = [],
) {
  tauriMocks.getCachedProviders.mockResolvedValue(providers);
  return render(
    <LocaleProvider>
      <PopOutPanel state={bootstrap(catalog)} providerId={providerId} />
    </LocaleProvider>,
  );
}

describe("PopOutPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.refreshProviders.mockResolvedValue(undefined);
    tauriMocks.refreshProvidersIfStale.mockResolvedValue(undefined);
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());
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
      buildBundle({ SummaryProvidersLabel: "providers" }),
    );
    eventMocks.listen.mockResolvedValue(() => {});
  });

  it("shows the provider grid and focuses provider targets", async () => {
    const { container } = renderPopOut(
      [provider("codex", "Codex", 80), provider("claude", "Claude", 30)],
      "claude",
    );

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(3);
    });

    expect(container.querySelector(".provider-grid__item--active")?.getAttribute("aria-label")).toBe("Claude");
    expect(screen.getAllByText("Claude").length).toBeGreaterThanOrEqual(2);
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(1);
  });

  it("renders overview cards in settings catalog order instead of fetch order", async () => {
    const catalog: ProviderCatalogEntry[] = [
      { id: "codex", displayName: "Codex", cookieDomain: null },
      { id: "claude", displayName: "Claude", cookieDomain: null },
      { id: "cursor", displayName: "Cursor", cookieDomain: null },
    ];

    const { container } = renderPopOut(
      [
        provider("cursor", "Cursor", 15),
        provider("codex", "Codex", 95),
        provider("claude", "Claude", 40),
      ],
      undefined,
      catalog,
    );

    await waitFor(() => {
      expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(3);
    });

    expect(
      Array.from(container.querySelectorAll(".menu-card__name")).map(
        (node) => node.textContent,
      ),
    ).toEqual(["Codex", "Claude", "Cursor"]);
  });

  it("keeps the popout overview focused until the provider grid expands", async () => {
    const providers = TEST_PROVIDER_CATALOG.map(([id, displayName], index) =>
      provider(id, displayName, (index * 7) % 100),
    );

    const { container } = renderPopOut(providers);

    await waitFor(() => {
      expect(container.querySelector(".provider-grid--compact")).not.toBeNull();
    });

    expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(20);
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(4);

    const expand = container.querySelector<HTMLButtonElement>(
      '.provider-grid__item--more[aria-label="Show all providers"]',
    );
    expect(expand).not.toBeNull();

    fireEvent.click(expand!);

    await waitFor(() => {
      expect(container.querySelectorAll(".provider-grid__item")).toHaveLength(
        providers.length + 2,
      );
    });
    expect(container.querySelectorAll(".menu-stack__item")).toHaveLength(
      providers.length,
    );
  });
});
