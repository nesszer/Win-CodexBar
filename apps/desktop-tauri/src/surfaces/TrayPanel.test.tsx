import { act, fireEvent, render, waitFor } from "@testing-library/react";
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
  openSettingsWindow: vi.fn(),
  quitApp: vi.fn(),
  getWorkAreaRect: vi.fn(),
  reanchorTrayPanel: vi.fn(),
  revealTrayPanelWindow: vi.fn(),
  openProviderDashboard: vi.fn(),
  openProviderStatusPage: vi.fn(),
  getProviderChartData: vi.fn(),
  getCurrentSurfaceState: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listeners: new Map<string, Array<(event: { payload: unknown }) => void>>(),
}));

const windowMocks = vi.hoisted(() => ({
  getCurrentWindow: vi.fn(() => ({
    setSize: vi.fn().mockResolvedValue(undefined),
    close: vi.fn().mockResolvedValue(undefined),
  })),
  LogicalSize: vi.fn((width: number, height: number) => ({ width, height })),
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

function settings(overrides: Partial<SettingsSnapshot> = {}): SettingsSnapshot {
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
    showAllTokenAccountsInMenu: false,
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
    windowScalePercent: 125,
    claudeAvoidKeychainPrompts: false,
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
  tauriMocks.getSettingsSnapshot.mockResolvedValue(settings(settingsOverrides));
  return render(
    <LocaleProvider>
      <TrayPanel state={bootstrap(settingsOverrides, catalog)} />
    </LocaleProvider>,
  );
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
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle());
    eventMocks.listen.mockImplementation(
      (event: string, handler: (event: { payload: unknown }) => void) => {
        const listeners = eventMocks.listeners.get(event) ?? [];
        listeners.push(handler);
        eventMocks.listeners.set(event, listeners);
        return Promise.resolve(() => {});
      },
    );
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

  it("reveals the tray panel if the native resize pass fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize: vi.fn().mockRejectedValue(new Error("resize failed")),
      close: vi.fn().mockResolvedValue(undefined),
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
    });
    const denseProviders = TEST_PROVIDER_CATALOG.slice(0, 36).map(([id, displayName]) =>
      provider(id, displayName),
    );

    renderTrayPanel(denseProviders, {
      enabledProviders: denseProviders.map((snapshot) => snapshot.providerId),
    });

    await waitFor(() => {
      expect(setSize).toHaveBeenCalledWith(
        expect.objectContaining({ width: 328, height: 776 }),
      );
    });
  });

  it("keeps provider detail mode tall enough for context actions and footer", async () => {
    const setSize = vi.fn().mockResolvedValue(undefined);
    windowMocks.getCurrentWindow.mockReturnValue({
      setSize,
      close: vi.fn().mockResolvedValue(undefined),
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
        expect.objectContaining({ width: 328, height: 420 }),
      );
    });
  });
});
