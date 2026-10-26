import type {
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  RateWindowSnapshot,
  SettingsSnapshot,
} from "../../types/bridge";

const MINUTE_MS = 60_000;

export function isoAfter(now: Date, minutes: number): string {
  return new Date(now.getTime() + minutes * MINUTE_MS).toISOString();
}

export function isoBefore(now: Date, minutes: number): string {
  return isoAfter(now, -minutes);
}

/** Local `YYYY-MM-DD` for the day `daysAgo` before `now`. */
export function dayKey(now: Date, daysAgo: number): string {
  const day = new Date(now.getFullYear(), now.getMonth(), now.getDate() - daysAgo);
  const month = String(day.getMonth() + 1).padStart(2, "0");
  const date = String(day.getDate()).padStart(2, "0");
  return `${day.getFullYear()}-${month}-${date}`;
}

export function rateWindow(
  now: Date,
  usedPercent: number,
  windowMinutes: number | null,
  resetsInMinutes: number | null,
): RateWindowSnapshot {
  return {
    usedPercent,
    remainingPercent: Math.max(0, 100 - usedPercent),
    windowMinutes,
    resetsAt: resetsInMinutes === null ? null : isoAfter(now, resetsInMinutes),
    resetDescription: null,
    isExhausted: usedPercent >= 100,
    reservePercent: null,
    reserveDescription: null,
  };
}

export interface SnapshotInput {
  providerId: string;
  displayName: string;
  primary: RateWindowSnapshot;
  primaryLabel: string;
  updatedAt: string;
}

export function providerSnapshot(
  input: SnapshotInput,
  overrides: Partial<ProviderUsageSnapshot> = {},
): ProviderUsageSnapshot {
  return {
    providerId: input.providerId,
    displayName: input.displayName,
    primary: input.primary,
    selectedMetric: input.primary,
    primaryLabel: input.primaryLabel,
    secondary: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "auto",
    updatedAt: input.updatedAt,
    error: null,
    errorState: "ready",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    fetchDurationMs: 420,
    ...overrides,
  };
}

/** A subset of the shipped catalog: enough for sidebars without listing every provider. */
export const PROOF_PROVIDER_CATALOG: ProviderCatalogEntry[] = [
  { id: "codex", displayName: "Codex", cookieDomain: "chatgpt.com", cliAliases: ["openai"] },
  { id: "claude", displayName: "Claude", cookieDomain: "claude.ai" },
  {
    id: "claudeplatform",
    displayName: "Claude Platform",
    cookieDomain: "platform.claude.com",
    cliAliases: ["claude-platform"],
  },
  { id: "cursor", displayName: "Cursor", cookieDomain: "cursor.com" },
  { id: "gemini", displayName: "Gemini", cookieDomain: "gemini.google.com" },
  { id: "copilot", displayName: "Copilot", cookieDomain: null },
  { id: "openrouter", displayName: "OpenRouter", cookieDomain: null },
  { id: "factory", displayName: "Factory", cookieDomain: "app.factory.ai" },
  { id: "kiro", displayName: "Kiro", cookieDomain: null },
  { id: "zai", displayName: "z.ai", cookieDomain: null },
  { id: "mistral", displayName: "Mistral", cookieDomain: null },
  { id: "openaiapi", displayName: "OpenAI API", cookieDomain: null },
  { id: "vercel", displayName: "Vercel", cookieDomain: null },
  { id: "kimi", displayName: "Kimi", cookieDomain: "kimi.com" },
];

export function catalogOrder(): string[] {
  return PROOF_PROVIDER_CATALOG.map((entry) => entry.id);
}

export function baseSettings(overrides: Partial<SettingsSnapshot> = {}): SettingsSnapshot {
  return {
    preferredCurrencyCode: "AUTO",
    enabledProviders: [],
    providerOrder: catalogOrder(),
    providerDefaultOrder: catalogOrder(),
    refreshIntervalSecs: 300,
    adaptiveRefresh: false,
    refreshAllProvidersOnMenuOpen: false,
    statusChecksEnabled: true,
    lowPowerMode: false,
    lowPowerModePreference: "off",
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
    providerQuotaWarnings: {},
    predictivePaceWarningEnabled: false,
    predictivePaceWarningLeadHours: 0,
    limitResetNotificationsEnabled: false,
    credentialExpiryNotificationsEnabled: false,
    sessionQuotaNotificationsEnabled: true,
    showPace: true,
    trayIconMode: "single",
    stackedTrayTopProvider: null,
    stackedTrayBottomProvider: null,
    switcherShowsIcons: true,
    menuBarShowsHighestUsage: false,
    menuBarShowsPercent: false,
    menuBarIconStyle: "bars",
    mergedOverviewSelectedProviders: [],
    mergedOverviewAllAccounts: false,
    quotaWarningMarkersVisible: true,
    highlightMenuCardsOnHover: true,
    workdayTickStyle: "subtle",
    menuBarColorPace: false,
    menuBarColorByProvider: false,
    menuBarPaceReservePositive: false,
    providersSortedAlphabetically: false,
    providersEnabledOnly: false,
    showAsUsed: true,
    showAllTokenAccountsInMenu: false,
    enableAnimations: false,
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
    antigravityAdditionalProfileHomes: [],
    agentSessionsEnabled: false,
    agentSessionsLocalOnly: false,
    stayAwakeEnabled: false,
    agentSessionSshHosts: [],
    hooksEnabled: false,
    httpProxyEnabled: false,
    httpProxyUrl: "",
    httpProxyUsername: "",
    httpProxyPassword: "",
    uiLanguage: "english",
    uiLanguageFollowsSystem: false,
    theme: "dark",
    windowScalePercent: 100,
    trayScalePercent: 100,
    trayPanelAlwaysOnTop: false,
    powertoysStatusPipeEnabled: false,
    shareUsageWithCli: false,
    claudeAvoidKeychainPrompts: false,
    claudeSwapEnabled: false,
    claudeSwapExecutablePath: "",
    claudeSwapSource: "claude-swap",
    claudeDesktopCookiesEnabled: false,
    codexSparkUsageVisible: true,
    codexOpenaiWebExtras: false,
    codexHistoricalTracking: true,
    codexExternalOauthSourcesAllowed: false,
    codexLocalSessionCostLedgerEnabled: true,
    codexOpenaiWebBatterySaverEnabled: false,
    openAiLiveUsageEnabled: true,
    openAiModelRateLimits: [],
    disableKeychainAccess: false,
    wayfinderGatewayUrl: "",
    providerMetrics: {},
    floatBarEnabled: true,
    floatBarOpacity: 90,
    floatBarScale: 100,
    floatBarOrientation: "horizontal",
    floatBarStyle: "floating",
    floatBarClickThrough: false,
    floatBarProviderIds: [],
    floatBarDarkText: false,
    floatBarShowResetInline: true,
    floatBarShowCost: false,
    promoteTrayIcon: false,
    claudeDailyRoutinesUsageVisible: true,
    claudeAllowReadingClaudeCodeCredentials: true,
    alibabaTokenPlanRegion: "intl",
    copilotSeatCreditEntitlement: null,
    weeklyProgressWorkDays: null,
    costSummaryDisplayStyle: "compact",
    openCodexUsageLogsEnabled: false,
    hideNativeCodexCostWhenOpenCodexPresent: false,
    costReportingPeriod: "rolling:30",
    costUsageEnabled: true,
    spendDashboardPeriod: "rolling:30",
    spendDashboardHiddenSourceIds: [],
    providerAccentColors: {},
    ...overrides,
  };
}
