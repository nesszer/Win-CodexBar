import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getAppInfo: vi.fn(),
  openExternalUrl: vi.fn(),
}));

const updateMocks = vi.hoisted(() => ({
  checkNow: vi.fn(),
  download: vi.fn(),
  apply: vi.fn(),
  dismiss: vi.fn(),
  openRelease: vi.fn(),
}));

vi.mock("../../../lib/tauri", () => tauriMocks);
vi.mock("../../../hooks/useLocale", () => ({
  // AboutBuilt keeps its Fluent `{}` placeholder so the rendered date is asserted.
  useLocale: () => ({ t: (key: string) => (key === "AboutBuilt" ? "Built {}" : key) }),
}));
vi.mock("../../../hooks/useUpdateState", () => ({
  useUpdateState: () => ({
    updateState: {
      status: "idle",
      version: null,
      error: null,
      progress: null,
      releaseUrl: null,
      canDownload: false,
      canApply: false,
      lastCheckedAt: null,
    },
    ...updateMocks,
  }),
}));

import AboutTab from "./AboutTab";
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

describe("AboutTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getAppInfo.mockResolvedValue({
      name: "CodexBar",
      version: "0.30.3",
      buildNumber: "dev",
      updateChannel: "stable",
      tagline: "Keep agent limits in view.",
    });
    tauriMocks.openExternalUrl.mockResolvedValue(undefined);
  });

  it("opens about links through the Tauri URL bridge", async () => {
    render(<AboutTab settings={settings} set={vi.fn()} saving={false} />);

    fireEvent.click(await screen.findByRole("button", { name: "AboutLinkGitHub" }));
    fireEvent.click(screen.getByRole("button", { name: "AboutLinkWebsite" }));
    fireEvent.click(screen.getByRole("button", { name: "AboutLinkOriginalProject" }));
    fireEvent.click(screen.getByRole("button", { name: "SubmitIssue" }));

    expect(tauriMocks.openExternalUrl).toHaveBeenNthCalledWith(
      1,
      "https://github.com/nesszer/Win-CodexBar",
    );
    expect(tauriMocks.openExternalUrl).toHaveBeenNthCalledWith(
      2,
      "https://codexbar.app",
    );
    expect(tauriMocks.openExternalUrl).toHaveBeenNthCalledWith(
      3,
      "https://github.com/steipete/CodexBar",
    );
    expect(tauriMocks.openExternalUrl).toHaveBeenNthCalledWith(
      4,
      "https://github.com/nesszer/Win-CodexBar/issues/new?labels=bug&template=bug_report.yml",
    );
  });

  it("shows a link error if the OS browser launch fails", async () => {
    tauriMocks.openExternalUrl.mockRejectedValue("no browser");

    render(<AboutTab settings={settings} set={vi.fn()} saving={false} />);

    fireEvent.click(await screen.findByRole("button", { name: "AboutLinkWebsite" }));

    await waitFor(() => {
      expect(screen.getByText("ErrorPrefix no browser")).toBeInTheDocument();
    });
  });
});

describe("AboutTab Mac grouped layout", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.openExternalUrl.mockResolvedValue(undefined);
  });

  it("shows the build date in the hero when the shell reports one", async () => {
    tauriMocks.getAppInfo.mockResolvedValue({
      name: "CodexBar",
      version: "0.70.0",
      buildNumber: "7",
      buildDate: "2026-10-11",
      updateChannel: "stable",
      tagline: "Keep agent limits in view.",
    });
    render(<AboutTab settings={settings} set={vi.fn()} saving={false} />);

    expect(await screen.findByText("Built 2026-10-11")).toBeInTheDocument();
    expect(screen.getByText("Version 0.70.0 (7)")).toBeInTheDocument();
  });

  it("leaves the build line out when no build date is known", async () => {
    tauriMocks.getAppInfo.mockResolvedValue({
      name: "CodexBar",
      version: "0.70.0",
      buildNumber: "dev",
      updateChannel: "stable",
      tagline: "Keep agent limits in view.",
    });
    render(<AboutTab settings={settings} set={vi.fn()} saving={false} />);

    await screen.findByText("Keep agent limits in view.");
    expect(screen.queryByText(/^Built /)).toBeNull();
  });

  it("groups Updates then Links, with Submit an Issue as a link row", async () => {
    tauriMocks.getAppInfo.mockResolvedValue({
      name: "CodexBar",
      version: "0.70.0",
      buildNumber: "dev",
      updateChannel: "stable",
      tagline: "Keep agent limits in view.",
    });
    const { container } = render(
      <AboutTab settings={settings} set={vi.fn()} saving={false} />,
    );
    await screen.findByText("Keep agent limits in view.");

    const titles = Array.from(
      container.querySelectorAll(".settings-group__title"),
      (node) => node.textContent,
    );
    expect(titles).toEqual(["UpdatesTitle", "SectionLinks"]);
    const linkRows = Array.from(
      container.querySelectorAll(".settings-link-row"),
      (node) => node.textContent,
    );
    expect(linkRows).toEqual([
      "AboutLinkGitHub↗",
      "AboutLinkWebsite↗",
      "AboutLinkOriginalProject↗",
      "SubmitIssue↗",
    ]);
    expect(screen.getByRole("checkbox", { name: "AutoDownloadUpdates" })).toHaveClass(
      "toggle--switch",
    );
    expect(screen.getByRole("combobox", { name: "UpdateChannelChoice" })).toHaveValue("stable");
    expect(screen.getByRole("button", { name: "AboutCheckForUpdates" })).toBeEnabled();
  });
});
