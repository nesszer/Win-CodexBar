import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key }),
}));

import GeneralTab from "./GeneralTab";
import type { SettingsSnapshot } from "../../../types/bridge";

const settings: SettingsSnapshot = {
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
  hidePersonalInfo: false,
  autoDownloadUpdates: false,
  installUpdatesOnQuit: false,
  globalShortcut: "",
  updateChannel: "stable",
  uiLanguage: "english",
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

describe("GeneralTab language picker", () => {
  it("renders 4 language options when spanish is wired", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    const select = screen.getByRole("combobox", {
      name: "InterfaceLanguage",
    });
    expect(select).toBeInTheDocument();

    const options = select.querySelectorAll("option");
    expect(options).toHaveLength(4);
  });

  it("includes spanish as a selectable option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(
      screen.getByText("LanguageSpanishOption"),
    ).toBeInTheDocument();
  });
});
