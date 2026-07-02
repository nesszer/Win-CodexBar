import { describe, it, expect } from "vitest";
import type { Language, LocaleStrings, SettingsSnapshot } from "./bridge";

describe("Language type", () => {
  it("accepts 'spanish' as a valid union member", () => {
    // Type-level assertion: this assignment must compile (tsc --noEmit gate).
    // Vitest strips types at transform time, so the runtime assertion only
    // exercises value correctness; tsc provides the RED/GREEN gate.
    const lang: Language = "spanish";
    expect(lang).toBe("spanish");
  });

  it("allows 'spanish' in LocaleStrings payload", () => {
    const payload: LocaleStrings = {
      language: "spanish",
      entries: { TabGeneral: "General" },
    };
    expect(payload.language).toBe("spanish");
    expect(payload.entries.TabGeneral).toBe("General");
  });

  it("allows 'spanish' in SettingsSnapshot.uiLanguage", () => {
    const snap: SettingsSnapshot = {
      enabledProviders: [],
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
      menuBarShowsHighestUsage: true,
      menuBarShowsPercent: true,
      showAsUsed: false,
      showAllTokenAccountsInMenu: true,
      enableAnimations: true,
      resetTimeRelative: true,
      menuBarDisplayMode: "compact",
      windowScalePercent: 125,
      hidePersonalInfo: false,
      autoDownloadUpdates: false,
      installUpdatesOnQuit: false,
      globalShortcut: "",
      updateChannel: "stable",
      uiLanguage: "spanish",
      theme: "dark",
      claudeAvoidKeychainPrompts: true,
      disableKeychainAccess: false,
      providerMetrics: {},
      floatBarEnabled: false,
      floatBarOpacity: 0.9,
      floatBarScale: 100,
      floatBarOrientation: "horizontal",
      floatBarStyle: "floating",
      floatBarClickThrough: false,
      floatBarProviderIds: [],
      floatBarDarkText: false,
      floatBarShowResetInline: false,
    };
    expect(snap.uiLanguage).toBe("spanish");
  });
});
