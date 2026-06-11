import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getSettingsSnapshot: vi.fn(),
  updateSettings: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
  listeners: new Map<string, Array<(event: { payload: unknown }) => void>>(),
}));

vi.mock("../lib/tauri", () => tauriMocks);

vi.mock("@tauri-apps/api/event", () => eventMocks);

import type { SettingsSnapshot } from "../types/bridge";
import { useSettings } from "./useSettings";

function settings(overrides: Partial<SettingsSnapshot> = {}): SettingsSnapshot {
  return {
    enabledProviders: ["codex"],
    refreshIntervalSecs: 300,
    startAtLogin: false,
    startMinimized: true,
    showNotifications: true,
    soundEnabled: true,
    soundVolume: 50,
    highUsageThreshold: 80,
    criticalUsageThreshold: 95,
    trayIconMode: "single",
    switcherShowsIcons: true,
    menuBarShowsHighestUsage: true,
    menuBarShowsPercent: false,
    showAsUsed: true,
    showCreditsExtraUsage: true,
    showAllTokenAccountsInMenu: false,
    surpriseAnimations: true,
    enableAnimations: true,
    resetTimeRelative: true,
    menuBarDisplayMode: "compact",
    hidePersonalInfo: false,
    updateChannel: "stable",
    autoDownloadUpdates: true,
    installUpdatesOnQuit: true,
    globalShortcut: "Ctrl+Shift+U",
    uiLanguage: "english",
    theme: "auto",
    claudeAvoidKeychainPrompts: false,
    disableKeychainAccess: false,
    showDebugSettings: false,
    providerMetrics: {},
    floatBarEnabled: false,
    floatBarOpacity: 100,
    floatBarOrientation: "horizontal",
    floatBarClickThrough: false,
    floatBarProviderIds: [],
    floatBarDarkText: false,
    ...overrides,
  };
}

function emitSettingsEvent(payload: SettingsSnapshot) {
  for (const listener of eventMocks.listeners.get("settings-updated") ?? []) {
    listener({ payload });
  }
}

describe("useSettings", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    eventMocks.listeners.clear();
    tauriMocks.getSettingsSnapshot.mockResolvedValue(settings());
    tauriMocks.updateSettings.mockResolvedValue(settings());
    eventMocks.listen.mockImplementation(
      (event: string, handler: (event: { payload: unknown }) => void) => {
        const listeners = eventMocks.listeners.get(event) ?? [];
        listeners.push(handler);
        eventMocks.listeners.set(event, listeners);
        return Promise.resolve(() => {});
      },
    );
  });

  it("subscribes to saved settings from other surfaces", async () => {
    const initial = settings({ switcherShowsIcons: true });
    const next = settings({
      switcherShowsIcons: false,
      menuBarShowsPercent: true,
      trayIconMode: "perProvider",
    });

    const { result } = renderHook(() => useSettings(initial));

    await waitFor(() => {
      expect(tauriMocks.getSettingsSnapshot).toHaveBeenCalledTimes(1);
    });

    act(() => {
      emitSettingsEvent(next);
    });

    expect(result.current.settings.switcherShowsIcons).toBe(false);
    expect(result.current.settings.menuBarShowsPercent).toBe(true);
    expect(result.current.settings.trayIconMode).toBe("perProvider");
  });

  it("keeps same-window hooks synchronized through the local DOM event", async () => {
    const initial = settings({ switcherShowsIcons: true });
    const next = settings({ switcherShowsIcons: false });

    const { result } = renderHook(() => useSettings(initial));

    await waitFor(() => {
      expect(tauriMocks.getSettingsSnapshot).toHaveBeenCalledTimes(1);
    });

    act(() => {
      window.dispatchEvent(
        new CustomEvent<SettingsSnapshot>("codexbar:settings-updated", {
          detail: next,
        }),
      );
    });

    expect(result.current.settings.switcherShowsIcons).toBe(false);
  });
});
