import { describe, it, expect } from "vitest";
import type { Language, LocaleStrings, SettingsSnapshot } from "./bridge";

describe("Language type", () => {
  it("accepts 'spanish' as a valid union member", () => {
    // Type-level assertion: this assignment must compile (tsc --noEmit gate).
    // Vitest strips types at transform time, so the runtime assertion only
    // exercises value correctness; tsc provides the RED/GREEN gate.
    const lang: Language = "spanish";
    expect(lang).toBe("spanish");
    const langKo: Language = "korean";
    expect(langKo).toBe("korean");
  });

  it("allows 'spanish' in LocaleStrings payload", () => {
    const payload: LocaleStrings = {
      language: "spanish",
      entries: { TabGeneral: "General" },
    };
    expect(payload.language).toBe("spanish");
    expect(payload.entries.TabGeneral).toBe("General");

    const payloadKo: LocaleStrings = {
      language: "korean",
      entries: { TabGeneral: "일반" },
    };
    expect(payloadKo.language).toBe("korean");
    expect(payloadKo.entries.TabGeneral).toBe("일반");
  });

  it("allows 'spanish' in SettingsSnapshot.uiLanguage", () => {
    const snap: SettingsSnapshot = {
      enabledProviders: [],
      refreshIntervalSecs: 300,
      refreshAllProvidersOnMenuOpen: false,
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
      trayScalePercent: 100,
      hidePersonalInfo: false,
      autoDownloadUpdates: false,
      installUpdatesOnQuit: false,
      globalShortcut: "",
      codexCustomSessionsDirs: [],
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

    const snapKo: SettingsSnapshot = {
      ...snap,
      uiLanguage: "korean",
    };
    expect(snapKo.uiLanguage).toBe("korean");
  });
});
