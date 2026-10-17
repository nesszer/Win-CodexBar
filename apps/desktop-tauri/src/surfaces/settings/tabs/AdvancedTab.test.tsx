import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getSafeDiagnostics: vi.fn(),
  registerGlobalShortcut: vi.fn().mockResolvedValue(undefined),
  unregisterGlobalShortcut: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("../../../lib/tauri", () => tauriMocks);
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => key }),
}));

import AdvancedTab from "./AdvancedTab";
import type { SettingsSnapshot } from "../../../types/bridge";

const settings: SettingsSnapshot = {
  enabledProviders: [],
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
  highUsageThreshold: 70,
  criticalUsageThreshold: 90,
  predictivePaceWarningEnabled: false,
  credentialExpiryNotificationsEnabled: false,
  trayIconMode: "single",
  switcherShowsIcons: true,
  menuBarShowsHighestUsage: true,
  menuBarShowsPercent: true,
  menuBarIconStyle: "percent",
  mergedOverviewSelectedProviders: [],
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
  showResetWhenExhausted: false,
  menuBarDisplayMode: "compact",
  overviewLayout: "detailed",
  notificationSoundPaths: {
    predictiveWarning: null,
    highUsage: null,
    criticalUsage: null,
    exhausted: null,
    statusIssue: null,
    sessionDepleted: null,
    sessionRestored: null,
  },
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
  windowScalePercent: 125,
  trayScalePercent: 100,
  trayPanelAlwaysOnTop: false,
  powertoysStatusPipeEnabled: false,
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
};

describe("AdvancedTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getSafeDiagnostics.mockResolvedValue("diagnostics text");
  });

  it("copies safe diagnostics to the clipboard", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    render(<AdvancedTab settings={settings} set={vi.fn()} saving={false} />);

    screen.getByRole("heading", { name: "DiagnosticsSectionHeading" });
    fireEvent.click(
      screen.getByRole("button", { name: "DiagnosticsCopyButton" }),
    );

    await waitFor(() => {
      expect(tauriMocks.getSafeDiagnostics).toHaveBeenCalled();
    });
    await waitFor(() => {
      expect(writeText).toHaveBeenCalledWith("diagnostics text");
    });
    await waitFor(() => {
      expect(screen.getAllByText("DiagnosticsCopied").length).toBeGreaterThan(0);
    });
  });


  it("keeps proxy text edits local until blur or Enter", () => {
    const set = vi.fn();
    render(
      <AdvancedTab
        settings={{
          ...settings,
          httpProxyEnabled: true,
          httpProxyUrl: "http://old-proxy:8080",
          httpProxyUsername: "old-user",
          httpProxyPassword: "old-pass",
        }}
        set={set}
        saving={false}
      />,
    );

    const url = screen.getByLabelText("NetworkProxyUrlLabel");
    fireEvent.change(url, { target: { value: "  http://127.0.0.1:7890  " } });
    expect(url).toHaveValue("  http://127.0.0.1:7890  ");
    expect(set).not.toHaveBeenCalled();
    fireEvent.blur(url);
    expect(set).toHaveBeenCalledWith({ httpProxyUrl: "http://127.0.0.1:7890" });

    set.mockClear();
    const user = screen.getByDisplayValue("old-user");
    fireEvent.focus(user);
    fireEvent.change(user, { target: { value: "  alice  " } });
    expect(set).not.toHaveBeenCalled();
    fireEvent.blur(user);
    expect(set).toHaveBeenCalledWith({ httpProxyUsername: "alice" });

    set.mockClear();
    const password = screen.getByDisplayValue("old-pass");
    fireEvent.change(password, { target: { value: "secret with spaces" } });
    expect(set).not.toHaveBeenCalled();
    fireEvent.blur(password);
    expect(set).toHaveBeenCalledWith({ httpProxyPassword: "secret with spaces" });
  });

  it("shows an error when copying diagnostics fails", async () => {
    tauriMocks.getSafeDiagnostics.mockRejectedValue(new Error("invoke failed"));
    render(<AdvancedTab settings={settings} set={vi.fn()} saving={false} />);

    screen.getByRole("heading", { name: "DiagnosticsSectionHeading" });
    fireEvent.click(
      screen.getByRole("button", { name: "DiagnosticsCopyButton" }),
    );
    await waitFor(() => {
      expect(
        screen.getAllByText(/DiagnosticsCopyFailed/).length,
      ).toBeGreaterThan(0);
    });
  });

  it("leaves Hooks to Settings > Hooks", () => {
    render(<AdvancedTab settings={settings} set={vi.fn()} saving={false} />);
    expect(screen.queryByText("HooksEnableLabel")).toBeNull();
  });

  it("leaves Agent sessions to Settings > Menu", () => {
    render(<AdvancedTab settings={settings} set={vi.fn()} saving={false} />);
    expect(screen.queryByText("AgentSessionsTitle")).toBeNull();
    expect(screen.queryByText("AgentSessionsStayAwakeLabel")).toBeNull();
    expect(screen.queryByText("AgentSessionsSshHostsLabel")).toBeNull();
  });
});

describe("AdvancedTab Mac grouped layout", () => {
  it("orders the groups with Privacy first and no keyboard or portable preferences", () => {
    const { container } = render(
      <AdvancedTab settings={settings} set={vi.fn()} saving={false} />,
    );
    const titles = Array.from(
      container.querySelectorAll(".settings-group__title"),
      (node) => node.textContent,
    );
    expect(titles).toEqual([
      "PrivacyTitle",
      "CodexLocalLogsTitle",
      "SectionLocalIntegrations",
      "NetworkProxyTitle",
      "DiagnosticsSectionHeading",
    ]);
    expect(screen.queryByText("SectionKeyboard")).toBeNull();
    expect(screen.queryByText("SectionPreferencesTransfer")).toBeNull();
    expect(screen.queryByText("KEYCHAIN ACCESS")).toBeNull();
  });

  it("draws every toggle as a switch", () => {
    render(<AdvancedTab settings={settings} set={vi.fn()} saving={false} />);
    for (const toggle of screen.getAllByRole("checkbox")) {
      expect(toggle).toHaveClass("toggle--switch");
    }
  });

  it("shows the credential-access footer while Credential Manager access is off", () => {
    const set = vi.fn();
    const { rerender } = render(
      <AdvancedTab settings={settings} set={set} saving={false} />,
    );
    // The Claude keychain-prompt option lives in Providers > Claude > Options (Mac parity).
    expect(screen.queryByRole("checkbox", { name: "AvoidKeychainPromptsLabel" })).toBeNull();
    expect(screen.queryByText("CredentialAccessFooter")).toBeNull();

    fireEvent.click(screen.getByRole("checkbox", { name: "DisableAllKeychainLabel" }));
    expect(set).toHaveBeenCalledWith({ disableKeychainAccess: true });

    rerender(
      <AdvancedTab
        settings={{ ...settings, disableKeychainAccess: true }}
        set={set}
        saving={false}
      />,
    );
    expect(screen.getByText("CredentialAccessFooter")).toBeInTheDocument();
  });
});
