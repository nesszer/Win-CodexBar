import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key }),
}));

// Mock Tauri invoke for get_available_languages
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async (command: string) => {
    if (command === "get_app_info") {
      return {
        name: "CodexBar",
        version: "0.60.3",
        buildNumber: "42",
        updateChannel: "stable",
        tagline: "usage",
      };
    }
    return [
      { value: "english", display: "English" },
      { value: "chinese", display: "中文" },
      { value: "chinesetraditional", display: "繁體中文" },
      { value: "japanese", display: "日本語" },
      { value: "korean", display: "한국어" },
      { value: "spanish", display: "Español" },
      { value: "russian", display: "Русский" },
      { value: "turkish", display: "Türkçe" },
      { value: "ukrainian", display: "Українська" },
    ];
  }),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(),
}));

import GeneralTab from "./GeneralTab";
import type { SettingsSnapshot } from "../../../types/bridge";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";

const settings: SettingsSnapshot = {
  enabledProviders: [],
  preferredCurrencyCode: "AUTO",
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
  menuBarShowsHighestUsage: true,
  menuBarShowsPercent: true,
  menuBarIconStyle: "percent",
  mergedOverviewSelectedProviders: [],
  mergedOverviewAllAccounts: false,
  quotaWarningMarkersVisible: true,
  workdayTickStyle: "subtle",
  menuBarColorPace: false,
  menuBarColorByProvider: false,
  menuBarPaceReservePositive: false,
  providersSortedAlphabetically: false,
  showAsUsed: false,
  showAllTokenAccountsInMenu: true,
  enableAnimations: true,
  resetTimeRelative: true,
  menuBarDisplayMode: "compact",
  overviewLayout: "detailed",
  windowScalePercent: 125,
  trayScalePercent: 100,
  trayPanelAlwaysOnTop: false,
  powertoysStatusPipeEnabled: false,
  shareUsageWithCli: false,
  hidePersonalInfo: false,
  autoDownloadUpdates: false,
  installUpdatesOnQuit: false,
  globalShortcut: "",
  switcherShortcuts: {},
  codexCustomSessionsDirs: [],
  antigravityAdditionalProfileHomes: [],
  updateChannel: "stable",
  uiLanguage: "english",
  uiLanguageFollowsSystem: false,
  theme: "dark",
  claudeAvoidKeychainPrompts: true,
  codexSparkUsageVisible: true,
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
  floatBarShowCost: false,
  claudeDailyRoutinesUsageVisible: true,
  claudeAllowReadingClaudeCodeCredentials: false,
  alibabaTokenPlanRegion: "cn",
  weeklyProgressWorkDays: null,
    costSummaryDisplayStyle: "compact",
    providerAccentColors: {},
  showResetWhenExhausted: false,
};

describe("GeneralTab running version", () => {
  it("shows the running app version on the general tab", async () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(await screen.findByText("Version 0.60.3 (42)")).toBeInTheDocument();
  });
});

describe("GeneralTab language picker", () => {
  it("lists System first and shows it while following the system", () => {
    render(
      <GeneralTab
        settings={{ ...settings, uiLanguage: "korean", uiLanguageFollowsSystem: true }}
        set={vi.fn()}
        saving={false}
      />,
    );

    const select = screen.getByDisplayValue("System") as HTMLSelectElement;
    expect(select.value).toBe("system");
    expect(select.querySelectorAll("option")[0].textContent).toBe("System");
  });

  it("picking System follows the system; picking a language stops following", () => {
    const set = vi.fn();
    render(<GeneralTab settings={settings} set={set} saving={false} />);

    const select = screen.getByDisplayValue("English");
    fireEvent.change(select, { target: { value: "system" } });
    expect(set).toHaveBeenLastCalledWith({ uiLanguageFollowsSystem: true });

    fireEvent.change(select, { target: { value: "japanese" } });
    expect(set).toHaveBeenLastCalledWith({ uiLanguage: "japanese" });
  });

  it("renders all supported language options", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    const select = screen.getByDisplayValue("English");
    expect(select).toBeInTheDocument();

    const options = select.querySelectorAll("option");
    expect(options.length).toBeGreaterThanOrEqual(8);
  });

  it("includes spanish as a selectable option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(
      screen.getByText("Español"),
    ).toBeInTheDocument();
  });

  it("includes russian as a selectable option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(screen.getByText("Русский")).toBeInTheDocument();
  });

  it("includes turkish as a selectable option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(screen.getByText("Türkçe")).toBeInTheDocument();
  });

  it("includes ukrainian as a selectable option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(screen.getByText("Українська")).toBeInTheDocument();
  });

  it("includes korean as a selectable option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(
      screen.getByText("한국어"),
    ).toBeInTheDocument();
  });

  it("includes Traditional Chinese as a selectable option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    expect(screen.getByText("繁體中文")).toBeInTheDocument();
  });

  it("updates the predictive pace warning preference", () => {
    const set = vi.fn();
    render(
      <GeneralTab
        mode="notifications"
        settings={settings}
        set={set}
        saving={false}
      />,
    );

    fireEvent.click(screen.getByRole("checkbox", { name: "PredictivePaceWarnings" }));

    expect(set).toHaveBeenCalledWith({ predictivePaceWarningEnabled: true });
  });

  it("saves the pace warning lead time and offers Any time first", () => {
    const set = vi.fn();
    render(
      <GeneralTab
        mode="notifications"
        settings={{ ...settings, predictivePaceWarningEnabled: true }}
        set={set}
        saving={false}
      />,
    );

    const select = screen.getByRole("combobox", { name: "PredictivePaceWarningLeadLabel" }) as HTMLSelectElement;
    expect(Array.from(select.options, (option) => option.value)).toEqual(["0", "6", "12", "24", "48"]);
    expect(select.options[0].textContent).toBe("PredictivePaceWarningLeadAny");
    fireEvent.change(select, { target: { value: "24" } });

    expect(set).toHaveBeenCalledWith({ predictivePaceWarningLeadHours: 24 });
  });

  it("shows a saved lead time that is not a preset", () => {
    render(
      <GeneralTab
        mode="notifications"
        settings={{ ...settings, predictivePaceWarningEnabled: true, predictivePaceWarningLeadHours: 168 }}
        set={vi.fn()}
        saving={false}
      />,
    );

    const select = screen.getByRole("combobox", { name: "PredictivePaceWarningLeadLabel" }) as HTMLSelectElement;
    expect(select).toHaveValue("168");
    expect(Array.from(select.options, (option) => option.value)).toEqual(["0", "6", "12", "24", "48", "168"]);
  });

  it("disables the pace warning lead time while pace warnings are off", () => {
    render(<GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />);

    expect(screen.getByRole("combobox", { name: "PredictivePaceWarningLeadLabel" })).toBeDisabled();
  });

  it("updates the credential expiry alert preference", () => {
    const set = vi.fn();
    render(
      <GeneralTab
        mode="notifications"
        settings={settings}
        set={set}
        saving={false}
      />,
    );

    const toggle = screen.getByRole("checkbox", {
      name: "CredentialExpiryNotifications",
    });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);

    expect(set).toHaveBeenCalledWith({
      credentialExpiryNotificationsEnabled: true,
    });
  });

  it("updates the low power mode preference", () => {
    const set = vi.fn();
    render(<GeneralTab settings={settings} set={set} saving={false} />);

    fireEvent.change(screen.getByDisplayValue("LowPowerModeOff"), {
      target: { value: "automatic" },
    });

    expect(set).toHaveBeenCalledWith({ lowPowerModePreference: "automatic" });
  });

  it("shows Check provider status on by default and saves turning it off", () => {
    const set = vi.fn();
    render(<GeneralTab settings={settings} set={set} saving={false} />);

    const toggle = screen.getByRole("checkbox", { name: "CheckProviderStatus" });
    expect(toggle).toBeChecked();
    expect(screen.getByText("CheckProviderStatusHelper")).toBeInTheDocument();
    fireEvent.click(toggle);

    expect(set).toHaveBeenCalledWith({ statusChecksEnabled: false });
  });

  it("shows Check provider status off when saved off and saves turning it on", () => {
    const set = vi.fn();
    render(
      <GeneralTab
        settings={{ ...settings, statusChecksEnabled: false }}
        set={set}
        saving={false}
      />,
    );

    const toggle = screen.getByRole("checkbox", { name: "CheckProviderStatus" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);

    expect(set).toHaveBeenCalledWith({ statusChecksEnabled: true });
  });

  it("updates the default notification sound set", () => {
    const set = vi.fn();
    render(
      <GeneralTab mode="notifications" settings={settings} set={set} saving={false} />,
    );

    const select = screen.getByRole("combobox", { name: "NotificationSoundTheme" });
    expect(select.querySelectorAll("option")).toHaveLength(2);
    expect(select).toHaveStyle({ minWidth: "180px" });
    fireEvent.change(select, {
      target: { value: "codexBar" },
    });

    expect(set).toHaveBeenCalledWith({ notificationSoundTheme: "codexBar" });
  });

  it("renders and previews all seven notification events", () => {
    render(
      <GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />,
    );

    const previewButtons = screen.getAllByRole("button", {
      name: /NotificationTestSound$/,
    });
    expect(previewButtons).toHaveLength(7);

    fireEvent.click(
      screen.getByRole("button", {
        name: "NotificationSoundEventSessionRestored: NotificationTestSound",
      }),
    );
    expect(invoke).toHaveBeenCalledWith("play_notification_sound", {
      event: "sessionRestored",
    });
  });

  it("cancels the sound preview timer on unmount", () => {
    vi.useFakeTimers();
    try {
      const { unmount } = render(
        <GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />,
      );
      fireEvent.click(
        screen.getByRole("button", {
          name: "NotificationSoundEventHighUsage: NotificationTestSound",
        }),
      );
      expect(vi.getTimerCount()).toBeGreaterThan(0);

      unmount();
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });

  it("assigns and clears a custom WAV for one notification", async () => {
    const set = vi.fn();
    vi.mocked(open).mockResolvedValue("C:\\sounds\\high-usage.wav");
    const { rerender } = render(
      <GeneralTab mode="notifications" settings={settings} set={set} saving={false} />,
    );

    fireEvent.click(
      screen.getByRole("button", {
        name: "NotificationSoundEventHighUsage: NotificationSoundChooseFile",
      }),
    );
    await waitFor(() =>
      expect(set).toHaveBeenCalledWith({
        notificationSoundPaths: {
          ...settings.notificationSoundPaths,
          highUsage: "C:\\sounds\\high-usage.wav",
        },
      }),
    );

    rerender(
      <GeneralTab
        mode="notifications"
        settings={{
          ...settings,
          notificationSoundPaths: {
            ...settings.notificationSoundPaths,
            highUsage: "C:\\sounds\\high-usage.wav",
          },
        }}
        set={set}
        saving={false}
      />,
    );
    expect(
      screen.getByRole("button", {
        name: "NotificationSoundEventHighUsage: high-usage.wav, NotificationSoundChooseFile",
      }),
    ).toBeInTheDocument();
    fireEvent.click(
      screen.getByRole("button", {
        name: "NotificationSoundEventHighUsage: NotificationSoundClearFile",
      }),
    );
    expect(set).toHaveBeenLastCalledWith({
      notificationSoundPaths: settings.notificationSoundPaths,
    });
  });

  it("reenables sound previews immediately when playback fails", async () => {
    render(
      <GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />,
    );
    await waitFor(() =>
      expect(invoke).toHaveBeenCalledWith("get_available_languages"),
    );
    vi.mocked(invoke).mockRejectedValueOnce(new Error("playback failed"));

    const preview = screen.getByRole("button", {
      name: "NotificationSoundEventCriticalUsage: NotificationTestSound",
    });
    fireEvent.click(preview);

    expect(await screen.findByRole("alert")).toHaveTextContent("playback failed");
    expect(preview).toBeEnabled();
  });

  it("no longer offers per-provider threshold overrides (they moved to Providers)", () => {
    render(<GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />);
    expect(screen.queryByText("ThresholdOverridesTitle")).toBeNull();
    expect(
      screen.queryByRole("spinbutton", { name: /ProviderNameCodex · ProviderSession/ }),
    ).toBeNull();
  });
});

describe("GeneralTab preferred currency picker", () => {
  it("offers AUTO then every catalog currency as CODE (symbol) in catalog order", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);
    const select = screen.getByLabelText("PreferredCurrencyLabel");
    const options = Array.from(select.querySelectorAll("option"), (option) => option.textContent);
    expect(options).toEqual([
      "AUTO", "USD ($)", "GBP (£)", "EUR (€)", "CZK (Kč)", "CNY (¥)", "JPY (¥)", "KRW (₩)",
      "CAD ($)", "AUD ($)", "HKD ($)", "TWD (NT$)", "SGD ($)", "INR (₹)", "CHF (Fr.)",
      "AED (د.إ)", "TRY (₺)", "NZD ($)", "SEK (kr)", "NOK (kr)", "DKK (kr)", "PLN (zł)",
      "BRL (R$)", "MXN ($)", "ZAR (R)", "THB (฿)", "IDR (Rp)", "VND (₫)", "UAH (₴)", "ILS (₪)",
    ]);
  });

  it("persists an explicitly selected preferred currency", () => {
    const set = vi.fn();
    render(<GeneralTab settings={settings} set={set} saving={false} />);
    fireEvent.change(screen.getByLabelText("PreferredCurrencyLabel"), { target: { value: "TRY" } });
    expect(set).toHaveBeenCalledWith({ preferredCurrencyCode: "TRY" });
  });
});


  it("renders the theme picker with auto/light/dark options in general mode", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);

    const select = screen.getByRole("combobox", { name: "ThemeLabel" });
    expect(select).toBeInTheDocument();
    expect(select.querySelectorAll("option")).toHaveLength(3);
    expect(select.querySelector('option[value="light"]')).not.toBeNull();
    expect(select.querySelector('option[value="dark"]')).not.toBeNull();
    expect(select.querySelector('option[value="auto"]')).not.toBeNull();
  });

  it("persists a light theme choice via updateSettings", () => {
    const set = vi.fn();
    render(<GeneralTab settings={settings} set={set} saving={false} />);

    const select = screen.getByRole("combobox", { name: "ThemeLabel" });
    fireEvent.change(select, { target: { value: "light" } });

    expect(set).toHaveBeenCalledWith({ theme: "light" });
  });

  it("does not render the theme picker in notifications mode", () => {
    render(
      <GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />,
    );

    expect(screen.queryByRole("combobox", { name: "ThemeLabel" })).toBeNull();
  });

describe("GeneralTab Mac grouped layout", () => {
  it("orders the groups System, Refreshing, Portable preferences, Keyboard shortcut", () => {
    const { container } = render(
      <GeneralTab settings={settings} set={vi.fn()} saving={false} />,
    );
    const titles = Array.from(
      container.querySelectorAll(".settings-group__title"),
      (node) => node.textContent,
    );
    expect(titles).toEqual([
      "StartupSettings",
      "SectionRefreshing",
      "SectionPreferencesTransfer",
      "SectionKeyboard",
    ]);
  });

  it("lists System rows in Mac order with Windows extras after them", () => {
    const { container } = render(
      <GeneralTab settings={settings} set={vi.fn()} saving={false} />,
    );
    const system = container.querySelector(".settings-group") as HTMLElement;
    const labels = Array.from(
      system.querySelectorAll(".settings-field__label"),
      (node) => node.textContent,
    );
    expect(labels).toEqual([
      "InterfaceLanguage",
      "PreferredCurrencyLabel",
      "Appearance",
      "StartAtLogin",
      "StartMinimized",
    ]);
    const toggles = within(system).getAllByRole("checkbox");
    expect(toggles).toHaveLength(2);
    for (const toggle of toggles) {
      expect(toggle).toHaveClass("toggle--switch");
    }
  });

  it("offers the Mac refresh intervals plus the Windows 1 hour option", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);
    const select = screen.getByRole("combobox", { name: "RefreshIntervalLabel" });
    const values = Array.from(
      select.querySelectorAll("option"),
      (option) => option.getAttribute("value"),
    );
    expect(values).toEqual(["0", "60", "120", "300", "900", "1800", "3600", "adaptive"]);
  });

  it("saves 2 min as a fixed interval and turns adaptive off", () => {
    const set = vi.fn();
    render(
      <GeneralTab
        settings={{ ...settings, adaptiveRefresh: true }}
        set={set}
        saving={false}
      />,
    );
    fireEvent.change(screen.getByRole("combobox", { name: "RefreshIntervalLabel" }), {
      target: { value: "120" },
    });
    expect(set).toHaveBeenCalledWith({ adaptiveRefresh: false, refreshIntervalSecs: 120 });
  });

  it("keeps a saved interval that is not in the menu visible", () => {
    render(
      <GeneralTab
        settings={{ ...settings, refreshIntervalSecs: 45 }}
        set={vi.fn()}
        saving={false}
      />,
    );
    const select = screen.getByRole("combobox", { name: "RefreshIntervalLabel" });
    expect(select).toHaveValue("45");
    expect(within(select).getByText("45 s")).toBeInTheDocument();
  });

  it("shows the Manual footer only when auto-refresh is off", () => {
    const { rerender } = render(
      <GeneralTab settings={settings} set={vi.fn()} saving={false} />,
    );
    expect(screen.queryByText("RefreshManualFooter")).toBeNull();

    rerender(
      <GeneralTab
        settings={{ ...settings, refreshIntervalSecs: 0, adaptiveRefresh: false }}
        set={vi.fn()}
        saving={false}
      />,
    );
    expect(screen.getByText("RefreshManualFooter")).toBeInTheDocument();

    rerender(
      <GeneralTab
        settings={{ ...settings, refreshIntervalSecs: 0, adaptiveRefresh: true }}
        set={vi.fn()}
        saving={false}
      />,
    );
    expect(screen.queryByText("RefreshManualFooter")).toBeNull();
  });

  it("hosts the global shortcut recorder as Open menu", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);
    expect(screen.getByText("GlobalShortcutFieldLabel")).toBeInTheDocument();
    expect(screen.getByText("ShortcutRecordingHint")).toBeInTheDocument();
  });

  it("shows the register error and does not save a shortcut that failed to register", async () => {
    const set = vi.fn();
    render(<GeneralTab settings={settings} set={set} saving={false} />);
    vi.mocked(invoke).mockRejectedValueOnce("Failed to register shortcut \"Ctrl+Shift+K\": taken");

    fireEvent.click(screen.getByRole("button", { name: "ShortcutRecordButton" }));
    fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true, shiftKey: true });

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Failed to register shortcut \"Ctrl+Shift+K\": taken",
    );
    expect(invoke).toHaveBeenCalledWith("register_global_shortcut", { accelerator: "Ctrl+Shift+K" });
    expect(set).not.toHaveBeenCalledWith({ globalShortcut: "Ctrl+Shift+K" });
  });

  it("shows the unregister error and keeps the shortcut when clearing fails", async () => {
    const set = vi.fn();
    render(
      <GeneralTab settings={{ ...settings, globalShortcut: "Ctrl+Shift+U" }} set={set} saving={false} />,
    );
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Failed to clear shortcuts: busy"));

    fireEvent.click(screen.getByRole("button", { name: "ShortcutClearButton" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Failed to clear shortcuts: busy");
    expect(invoke).toHaveBeenCalledWith("unregister_global_shortcut");
    expect(set).not.toHaveBeenCalledWith({ globalShortcut: "" });
  });

  it("saves the shortcut after it registers", async () => {
    const set = vi.fn();
    render(<GeneralTab settings={settings} set={set} saving={false} />);

    fireEvent.click(screen.getByRole("button", { name: "ShortcutRecordButton" }));
    fireEvent.keyDown(window, { key: "k", code: "KeyK", ctrlKey: true, shiftKey: true });

    await waitFor(() => expect(set).toHaveBeenCalledWith({ globalShortcut: "Ctrl+Shift+K" }));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("quits from the footer button", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);
    fireEvent.click(screen.getByRole("button", { name: "QuitCodexBar" }));
    expect(invoke).toHaveBeenCalledWith("quit_app");
  });

  it("opens the provider switcher shortcuts dialog and closes it with Escape or Done", () => {
    render(<GeneralTab settings={settings} set={vi.fn()} saving={false} />);
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "SwitcherShortcutsOpenButton" }));
    const dialog = screen.getByRole("dialog", { name: "SwitcherShortcutsTitle" });
    expect(
      within(dialog).getByRole("button", { name: "SwitcherShortcutReset" }),
    ).toBeInTheDocument();

    fireEvent.keyDown(dialog, { key: "Escape" });
    expect(screen.queryByRole("dialog")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "SwitcherShortcutsOpenButton" }));
    fireEvent.click(screen.getByRole("button", { name: "DialogDone" }));
    expect(screen.queryByRole("dialog")).toBeNull();
  });
});

describe("GeneralTab notifications pane", () => {
  it("orders the Alerts rows like the Mac pane", () => {
    const { container } = render(
      <GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />,
    );
    const alerts = container.querySelector(".settings-group") as HTMLElement;
    const labels = Array.from(
      alerts.querySelectorAll(".settings-field__label"),
      (node) => node.textContent,
    );
    expect(labels).toEqual([
      "CredentialExpiryNotifications",
      "SessionQuotaNotifications",
      "ShowNotifications",
      "PredictivePaceWarnings",
      "PredictivePaceWarningLeadLabel",
      "LimitResetNotifications",
    ]);
  });

  it("saves the limit reset preference and treats an absent key as off", () => {
    const set = vi.fn();
    render(
      <GeneralTab
        mode="notifications"
        settings={{ ...settings, limitResetNotificationsEnabled: undefined }}
        set={set}
        saving={false}
      />,
    );
    const toggle = screen.getByRole("checkbox", { name: "LimitResetNotifications" });
    expect(toggle).not.toBeChecked();
    fireEvent.click(toggle);
    expect(set).toHaveBeenCalledWith({ limitResetNotificationsEnabled: true });
  });

  it("keeps the limit reset toggle usable while usage warnings are off", () => {
    render(
      <GeneralTab
        mode="notifications"
        settings={{
          ...settings,
          showNotifications: false,
          limitResetNotificationsEnabled: true,
        }}
        set={vi.fn()}
        saving={false}
      />,
    );
    const toggle = screen.getByRole("checkbox", { name: "LimitResetNotifications" });
    expect(toggle).toBeEnabled();
    expect(toggle).toBeChecked();
  });

  it("saves quota depleted and restored alerts as their own key", () => {
    const set = vi.fn();
    render(
      <GeneralTab
        mode="notifications"
        settings={{ ...settings, sessionQuotaNotificationsEnabled: true }}
        set={set}
        saving={false}
      />,
    );
    const toggle = screen.getByRole("checkbox", { name: "SessionQuotaNotifications" });
    expect(toggle).toBeChecked();
    fireEvent.click(toggle);
    expect(set).toHaveBeenCalledWith({ sessionQuotaNotificationsEnabled: false });
  });

  it("falls back to show notifications when the quota key is absent", () => {
    render(
      <GeneralTab
        mode="notifications"
        settings={{
          ...settings,
          sessionQuotaNotificationsEnabled: undefined,
          showNotifications: false,
        }}
        set={vi.fn()}
        saving={false}
      />,
    );
    expect(
      screen.getByRole("checkbox", { name: "SessionQuotaNotifications" }),
    ).not.toBeChecked();
  });

  it("locks credential and pace alerts but keeps the thresholds while usage warnings are off (#816)", () => {
    render(
      <GeneralTab
        mode="notifications"
        settings={{
          ...settings,
          showNotifications: false,
          predictivePaceWarningEnabled: true,
          predictivePaceWarningLeadHours: 0,
          credentialExpiryNotificationsEnabled: true,
        }}
        set={vi.fn()}
        saving={false}
      />,
    );
    const pace = screen.getByRole("checkbox", { name: "PredictivePaceWarnings" });
    const credential = screen.getByRole("checkbox", { name: "CredentialExpiryNotifications" });
    expect(pace).toBeDisabled();
    expect(pace).toBeChecked();
    expect(credential).toBeDisabled();
    expect(credential).toBeChecked();
    expect(screen.getByRole("checkbox", { name: "SessionQuotaNotifications" })).toBeEnabled();
    // The thresholds also colour the usage bars and the float bar, so they stay editable.
    expect(screen.getByText("WarningAtPercentUsed")).toBeInTheDocument();
    expect(screen.getByText("CriticalAtPercentUsed")).toBeInTheDocument();
    expect(screen.getByDisplayValue("70")).toBeEnabled();
    expect(screen.getByDisplayValue("90")).toBeEnabled();
    expect(screen.queryByText("ThresholdOverridesTitle")).toBeNull();
    expect(screen.getByRole("checkbox", { name: "SoundEnabled" })).toBeInTheDocument();
  });

  it("shows % used thresholds while usage warnings are on", () => {
    render(
      <GeneralTab mode="notifications" settings={settings} set={vi.fn()} saving={false} />,
    );
    expect(screen.getByText("WarningAtPercentUsed")).toBeInTheDocument();
    expect(screen.getByText("CriticalAtPercentUsed")).toBeInTheDocument();
    expect(screen.getByText("ThresholdsCaption")).toBeInTheDocument();
  });

  it("hides the sound set and per-event sounds while sound is off", () => {
    render(
      <GeneralTab
        mode="notifications"
        settings={{ ...settings, soundEnabled: false }}
        set={vi.fn()}
        saving={false}
      />,
    );
    expect(screen.queryByRole("combobox", { name: "NotificationSoundTheme" })).toBeNull();
    expect(screen.queryAllByRole("button", { name: /NotificationTestSound$/ })).toHaveLength(0);
  });
});
