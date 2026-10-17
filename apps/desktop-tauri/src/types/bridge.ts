import type { LocaleKey } from "../i18n/keys";

export type SurfaceMode = "hidden" | "trayPanel" | "popOut" | "settings";
export type VisibleSurfaceMode = Exclude<SurfaceMode, "hidden">;
export type SettingsTabId =
  | "general"
  | "providers"
  | "notifications"
  | "menuBar"
  | "menu"
  | "usageSpend"
  | "advanced"
  | "hooks"
  | "about";

// ── Narrowed string-literal unions (persisted settings enums) ─────────

export type TrayIconMode = "single" | "perProvider" | "stacked";

export type NotificationSoundTheme = "windows" | "codexBar";

export type NotificationSoundEvent =
  | "predictiveWarning"
  | "highUsage"
  | "criticalUsage"
  | "exhausted"
  | "statusIssue"
  | "sessionDepleted"
  | "sessionRestored";

export interface NotificationSoundPaths {
  predictiveWarning: string | null;
  highUsage: string | null;
  criticalUsage: string | null;
  exhausted: string | null;
  statusIssue: string | null;
  sessionDepleted: string | null;
  sessionRestored: string | null;
}

export type MetricPreference =
  | "automatic"
  | "session"
  | "weekly"
  | "model"
  | "tertiary"
  | "credits"
  | "extraUsage"
  | "monthlyPlan"
  | "average";

export type Language =
  | "english"
  | "chinese"
  | "chinesetraditional"
  | "japanese"
  | "korean"
  | "spanish"
  | "portuguesebrazil"
  | "russian"
  | "turkish"
  | "ukrainian";

/** Language catalog entry from the Rust backend. */
export type LanguageOption = {
  /** Stable bridge/settings value (e.g. "english") */
  value: Language;
  /** Native display name (e.g. "English", "中文", "Español") */
  display: string;
};

export type UpdateChannel = "stable" | "beta";

export type ThemePreference = "auto" | "light" | "dark";

export type MenuBarDisplayMode = "minimal" | "compact" | "detailed";
export type OverviewLayout = "detailed" | "compact";

/** How cost is rendered on provider MenuCards (#2976). */
export type CostSummaryDisplayStyle = "compact" | "detailed" | "hidden";
/** Settings > Menu Bar > Menu bar style (`menu_bar_icon_style`). */
export type MenuBarIconStyle = "bars" | "percent" | "stackedBars";
/** Settings > Menu > Workday tick appearance (`workday_tick_style`). */
export type WorkdayTickStyle = "hidden" | "subtle" | "high_contrast";
export type FloatBarOrientation = "horizontal" | "vertical";
export type FloatBarStyle = "floating" | "taskbar";

export type TrayVisibilitySupport = "supported" | "unsupportedOs";
export type TrayVisibilityState = "promoted" | "notPromoted" | "entryNotFound" | "unknown";

export type DeepSeekPricingPeriod = "standard" | "peak" | "offPeak";

export interface DeepSeekPricingStatus {
  period: DeepSeekPricingPeriod;
  currentLocalTime: string;
  nextTransitionLocalTime: string | null;
  effectiveLocalTime: string;
}

export interface TrayVisibilityStatusDto {
  support: TrayVisibilitySupport;
  state: TrayVisibilityState;
}

export type TrayPanelSurfaceTarget = { kind: "summary" };
export type PopOutSurfaceTarget =
  | { kind: "dashboard" }
  | { kind: "provider"; providerId: string };
/** `tab` is a settings tab id, or `providers:<id>` for one provider's pane. */
export type SettingsSurfaceTarget = {
  kind: "settings";
  tab: SettingsTabId | `providers:${string}`;
};

export type SurfaceTarget =
  | TrayPanelSurfaceTarget
  | PopOutSurfaceTarget
  | SettingsSurfaceTarget;

export type SurfaceTargetForMode<M extends VisibleSurfaceMode> =
  M extends "trayPanel"
    ? TrayPanelSurfaceTarget
    : M extends "popOut"
      ? PopOutSurfaceTarget
      : SettingsSurfaceTarget;

export interface CurrentSurfaceState {
  mode: SurfaceMode;
  target: SurfaceTarget;
}

export interface AgentSession {
  id: string;
  provider: "codex" | "claude" | "pi";
  /** Pi-family dialect (upstream 0.48.0 #2626); absent for Codex/Claude. */
  dialect?: "pi" | "omp";
  /** Optional session title (Pi-family `session_info`/`title` records). */
  sessionName?: string;
  source: "cli" | "desktopApp" | "ide" | "unknown";
  state: "active" | "idle";
  pid: number | null;
  transcriptPath: string | null;
  host: string;
  workspace: {
    cwd: string | null;
    projectName: string | null;
  };
  activity: {
    startedAt: string | null;
    lastActivityAt: string | null;
  };
  focusTarget:
    | { kind: "process"; pid: number }
    | { kind: "transcript"; transcriptPath: string }
    | { kind: "none" };
}

export interface AgentSessionHostResult {
  host: string;
  sessions: AgentSession[];
  error: string | null;
}

export type AgentSessionDiscoveryResult =
  | { status: "disabled" }
  | { status: "hosts"; hosts: AgentSessionHostResult[] };

export type SessionFocusResult =
  | { status: "focused" }
  | { status: "unsupported"; message: string }
  | { status: "failed"; message: string };

export interface ProviderCatalogEntry {
  id: string;
  displayName: string;
  cookieDomain: string | null;
  /** CLI aliases the Settings search also matches. Absent on older shells. */
  cliAliases?: string[];
}

/** Upstream `ProviderStatusIndicator` raw values; `none` is operational. */
export type ServiceStatusIndicator =
  | "none"
  | "minor"
  | "major"
  | "critical"
  | "maintenance"
  | "unknown";

export interface ProviderServiceStatus {
  indicator: ServiceStatusIndicator;
  description: string | null;
  /** Unix milliseconds of the status feed's last update. */
  updatedAt: number | null;
}

/**
 * One enabled provider with a status page. `status` is null until the first
 * successful poll. The list is empty while "Check provider status" is off.
 */
export interface ProviderStatusEntry {
  providerId: string;
  status: ProviderServiceStatus | null;
}

export interface ProviderSummary {
  id: string;
  displayName: string;
  enabled: boolean;
  order: number;
}

export interface SettingsSnapshot {
  preferredCurrencyCode?: string;
  enabledProviders: string[];
  providerOrder?: string[];
  refreshIntervalSecs: number;
  adaptiveRefresh: boolean;
  refreshAllProvidersOnMenuOpen: boolean;
  statusChecksEnabled: boolean;
  lowPowerMode: boolean;
  lowPowerModePreference?: "off" | "on" | "automatic";
  startAtLogin: boolean;
  startMinimized: boolean;
  showNotifications: boolean;
  soundEnabled: boolean;
  notificationSoundTheme: NotificationSoundTheme;
  notificationSoundPaths: NotificationSoundPaths;
  highUsageThreshold: number;
  criticalUsageThreshold: number;
  /** Per-provider quota warnings keyed by CLI name; Global providers are absent. */
  providerQuotaWarnings?: Record<string, ProviderQuotaWarnings>;
  predictivePaceWarningEnabled: boolean;
  /** Absent on older shells; the UI treats that as off. */
  limitResetNotificationsEnabled?: boolean;
  credentialExpiryNotificationsEnabled: boolean;
  /** Absent on older shells; the UI falls back to `showNotifications`. */
  sessionQuotaNotificationsEnabled?: boolean;
  showPace?: boolean;
  trayIconMode: TrayIconMode;
  stackedTrayTopProvider?: string | null;
  stackedTrayBottomProvider?: string | null;
  switcherShowsIcons: boolean;
  menuBarShowsHighestUsage: boolean;
  /** Legacy mirror of `menuBarIconStyle === "percent"`; kept one release. */
  menuBarShowsPercent: boolean;
  menuBarIconStyle: MenuBarIconStyle;
  /** Overview cards, at most 6; empty means the first 6 enabled providers. */
  mergedOverviewSelectedProviders: string[];
  quotaWarningMarkersVisible: boolean;
  /** Pointer hover highlights the tray panel Refresh row. Absent on older shells: on. */
  highlightMenuCardsOnHover?: boolean;
  workdayTickStyle: WorkdayTickStyle;
  menuBarColorPace: boolean;
  /** Tint the tray usage meter with each shown provider's accent color (`menu_bar_color_by_provider`). */
  menuBarColorByProvider: boolean;
  /** Signed pace shows reserve as `+` (deficit as `-`) instead of the default deficit-positive sign. */
  menuBarPaceReservePositive: boolean;
  /** Settings sidebar lists providers alphabetically, enabled first. Display only. */
  providersSortedAlphabetically: boolean;
  /** Settings sidebar lists only enabled providers. Local, not portable. Absent on older shells: off. */
  providersEnabledOnly?: boolean;
  showAsUsed: boolean;
  showAllTokenAccountsInMenu: boolean;
  enableAnimations: boolean;
  resetTimeRelative: boolean;
  showResetWhenExhausted: boolean;
  menuBarDisplayMode: MenuBarDisplayMode;
  overviewLayout: OverviewLayout;
  hidePersonalInfo: boolean;
  updateChannel: UpdateChannel;
  autoDownloadUpdates: boolean;
  installUpdatesOnQuit: boolean;
  globalShortcut: string;
  /** Fully resolved action -> shortcut map (defaults overlaid by overrides). */
  switcherShortcuts: Record<string, string>;
  /** Extra Codex home or sessions directories scanned for local cost estimates. */
  codexCustomSessionsDirs: string[];
  /** Extra Gemini profile homes whose Antigravity local history is combined with the default home. */
  antigravityAdditionalProfileHomes: string[];
  agentSessionsEnabled?: boolean;
  /** Hold system awake while a local agent session is live. */
  stayAwakeEnabled?: boolean;
  agentSessionSshHosts?: string[];
  /** Master switch for external hooks (hooks.json next to settings). */
  hooksEnabled?: boolean;
  /** Route provider HTTPS through a user HTTP(S) proxy (#235). */
  httpProxyEnabled?: boolean;
  httpProxyUrl?: string;
  httpProxyUsername?: string;
  httpProxyPassword?: string;
  uiLanguage: Language;
  /** Language "System": `uiLanguage` follows the Windows display language. */
  uiLanguageFollowsSystem: boolean;
  theme: ThemePreference;
  /** 100..=250 — clamped server-side. */
  windowScalePercent: number;
  /** 100..=200 — clamped server-side. */
  trayScalePercent: number;
  /** Keep the tray panel above other windows after it loses focus. */
  trayPanelAlwaysOnTop: boolean;
  powertoysStatusPipeEnabled: boolean;
  claudeAvoidKeychainPrompts: boolean;
  /** Opt-in external claude-swap (`cswap`) account import (Claude only). */
  claudeSwapEnabled?: boolean;
  /** Path to the cswap executable (Claude only, empty when unset). */
  claudeSwapExecutablePath?: string;
  codexSparkUsageVisible: boolean;
  /** Absent on older shells. */
  codexOpenaiWebExtras?: boolean;
  codexHistoricalTracking?: boolean;
  codexExternalOauthSourcesAllowed?: boolean;
  codexLocalSessionCostLedgerEnabled?: boolean;
  codexOpenaiWebBatterySaverEnabled?: boolean;
  /** OpenAI API Live Usage (on by default); absent on older shells. */
  openAiLiveUsageEnabled?: boolean;
  /** OpenAI API manual per-model organization limits. */
  openAiModelRateLimits?: ModelRateLimitSnapshot[];
  disableKeychainAccess: boolean;
  wayfinderGatewayUrl?: string;
  providerMetrics: Record<string, MetricPreference>;
  floatBarEnabled: boolean;
  /** 30..=100 — clamped server-side. */
  floatBarOpacity: number;
  /** 75..=200 — clamped server-side. */
  floatBarScale: number;
  floatBarOrientation: FloatBarOrientation;
  floatBarStyle: FloatBarStyle;
  floatBarClickThrough: boolean;
  /** Empty array = show all enabled providers. */
  floatBarProviderIds: string[];
  /** When true, render with dark text/glass for light desktops. */
  floatBarDarkText: boolean;
  /** When true, render the selected metric's next reset inline in each provider pill. */
  floatBarShowResetInline: boolean;
  /** When true, scan and render local cost summaries. */
  floatBarShowCost: boolean;
  /** Promote the tray icon out of the Windows hidden-icons overflow (Win11 only). */
  promoteTrayIcon?: boolean;
  /** When true, show Claude Daily Routines quota row (default true). */
  claudeDailyRoutinesUsageVisible: boolean;
  /**
   * Explicit consent to read (and refresh) Claude Code's own OAuth
   * credentials for the Claude provider. Default false — without consent
   * OAuth stays closed and Auto falls back to labeled reduced-fidelity CLI
   * usage (upstream #2634/#2745).
   */
  claudeAllowReadingClaudeCodeCredentials: boolean;
  /** Alibaba Token Plan region: cn | intl | cn-personal | intl-personal. */
  alibabaTokenPlanRegion: string;
  /**
   * Optional user-entered Copilot seat AI-credit allowance.
   * Snapshot-side null and absent are equivalent.
   */
  copilotSeatCreditEntitlement?: number | null;
  /** Optional work-week length [2,7]; null or absent means Automatic. */
  weeklyProgressWorkDays?: number | null;
  /** How cost is rendered on provider cards (#2976). */
  costSummaryDisplayStyle: CostSummaryDisplayStyle;
  /** Opt-in read-only OpenCodex usage.jsonl import. */
  openCodexUsageLogsEnabled?: boolean;
  hideNativeCodexCostWhenOpenCodexPresent?: boolean;
  /**
   * History window for local cost surfaces: `rolling:N` (1..=365),
   * `month-to-date`, or `all`. Absent from older backends.
   */
  costReportingPeriod?: string;
  /** Usage & Spend "Track costs"; off stops the local cost-log scan. Absent from older backends. */
  costUsageEnabled?: boolean;
  /** Usage & Spend time range, same raw form as `costReportingPeriod`. */
  spendDashboardPeriod?: string;
  /** Usage & Spend sources (row `providerId`s) hidden from the dashboard; sorted and unique. */
  spendDashboardHiddenSourceIds?: string[];
  /** Per-provider accent color overrides (CLI name → hex color, #2972). */
  providerAccentColors: Record<string, string>;
}

export interface CurrencyRatesSnapshot {
  rates: Record<string, number>;
  /** Codes the converter supports (Rust-owned list). */
  supportedCodes: string[];
  /** Normalized preference this snapshot was built for ("AUTO" or a code). */
  preferredCode: string;
}

/** Partial settings object — only include fields you want to change. */
export interface SettingsUpdate {
  preferredCurrencyCode?: string;
  enabledProviders?: string[];
  refreshIntervalSecs?: number;
  adaptiveRefresh?: boolean;
  refreshAllProvidersOnMenuOpen?: boolean;
  statusChecksEnabled?: boolean;
  lowPowerMode?: boolean;
  lowPowerModePreference?: "off" | "on" | "automatic";
  startAtLogin?: boolean;
  startMinimized?: boolean;
  showNotifications?: boolean;
  soundEnabled?: boolean;
  notificationSoundTheme?: NotificationSoundTheme;
  notificationSoundPaths?: NotificationSoundPaths;
  highUsageThreshold?: number;
  criticalUsageThreshold?: number;
  /** Replaces each listed provider's whole value; `{}` resets it to Global. */
  providerQuotaWarnings?: Record<string, ProviderQuotaWarnings>;
  predictivePaceWarningEnabled?: boolean;
  limitResetNotificationsEnabled?: boolean;
  credentialExpiryNotificationsEnabled?: boolean;
  sessionQuotaNotificationsEnabled?: boolean;
  showPace?: boolean;
  trayIconMode?: TrayIconMode;
  stackedTrayTopProvider?: string;
  stackedTrayBottomProvider?: string;
  switcherShowsIcons?: boolean;
  menuBarShowsHighestUsage?: boolean;
  menuBarShowsPercent?: boolean;
  menuBarIconStyle?: MenuBarIconStyle;
  mergedOverviewSelectedProviders?: string[];
  quotaWarningMarkersVisible?: boolean;
  highlightMenuCardsOnHover?: boolean;
  workdayTickStyle?: WorkdayTickStyle;
  menuBarColorPace?: boolean;
  menuBarColorByProvider?: boolean;
  menuBarPaceReservePositive?: boolean;
  providersSortedAlphabetically?: boolean;
  providersEnabledOnly?: boolean;
  showAsUsed?: boolean;
  showAllTokenAccountsInMenu?: boolean;
  enableAnimations?: boolean;
  resetTimeRelative?: boolean;
  showResetWhenExhausted?: boolean;
  menuBarDisplayMode?: MenuBarDisplayMode;
  overviewLayout?: OverviewLayout;
  hidePersonalInfo?: boolean;
  updateChannel?: UpdateChannel;
  autoDownloadUpdates?: boolean;
  installUpdatesOnQuit?: boolean;
  globalShortcut?: string;
  /** Overrides only; replaces the stored map. `{}` restores the defaults. */
  switcherShortcuts?: Record<string, string>;
  codexCustomSessionsDirs?: string[];
  antigravityAdditionalProfileHomes?: string[];
  agentSessionsEnabled?: boolean;
  stayAwakeEnabled?: boolean;
  agentSessionSshHosts?: string[];
  hooksEnabled?: boolean;
  httpProxyEnabled?: boolean;
  httpProxyUrl?: string;
  httpProxyUsername?: string;
  httpProxyPassword?: string;
  uiLanguage?: Language;
  /** `true` picks "System"; picking a `uiLanguage` turns it off. */
  uiLanguageFollowsSystem?: boolean;
  theme?: ThemePreference;
  windowScalePercent?: number;
  trayScalePercent?: number;
  trayPanelAlwaysOnTop?: boolean;
  powertoysStatusPipeEnabled?: boolean;
  claudeAvoidKeychainPrompts?: boolean;
  claudeAllowReadingClaudeCodeCredentials?: boolean;
  claudeSwapEnabled?: boolean;
  claudeSwapExecutablePath?: string;
  codexSparkUsageVisible?: boolean;
  codexOpenaiWebExtras?: boolean;
  codexHistoricalTracking?: boolean;
  codexExternalOauthSourcesAllowed?: boolean;
  codexLocalSessionCostLedgerEnabled?: boolean;
  codexOpenaiWebBatterySaverEnabled?: boolean;
  /** Fetch the last hour of OpenAI API Tokens/min on each refresh. */
  openAiLiveUsageEnabled?: boolean;
  /** Replaces the stored OpenAI API limit list; an empty list clears it. */
  openAiModelRateLimits?: ModelRateLimitSnapshot[];
  disableKeychainAccess?: boolean;
  /** Map of provider CLI name → metric preference label. */
  providerMetrics?: Record<string, MetricPreference>;
  /**
   * Map of provider CLI name → full hidden usage-item ID list. Replaces the
   * whole list for that provider; empty clears all hidden rows.
   */
  providerHiddenUsageItemIds?: Record<string, string[]>;
  floatBarEnabled?: boolean;
  floatBarOpacity?: number;
  floatBarScale?: number;
  floatBarOrientation?: FloatBarOrientation;
  floatBarStyle?: FloatBarStyle;
  floatBarClickThrough?: boolean;
  floatBarProviderIds?: string[];
  floatBarDarkText?: boolean;
  floatBarShowResetInline?: boolean;
  floatBarShowCost?: boolean;
  promoteTrayIcon?: boolean;
  claudeDailyRoutinesUsageVisible?: boolean;
  alibabaTokenPlanRegion?: string;
  /** Optional user-entered Copilot seat AI-credit allowance; null clears it. */
  copilotSeatCreditEntitlement?: number | null;
  /** 2..=7 work days; null selects Automatic. */
  weeklyProgressWorkDays?: number | null;
  costSummaryDisplayStyle?: CostSummaryDisplayStyle;
  openCodexUsageLogsEnabled?: boolean;
  hideNativeCodexCostWhenOpenCodexPresent?: boolean;
  /** `rolling:N` (1..=365), `month-to-date`, or `all`; the backend rejects other values. */
  costReportingPeriod?: string;
  costUsageEnabled?: boolean;
  /** Same format as `costReportingPeriod`. */
  spendDashboardPeriod?: string;
  spendDashboardHiddenSourceIds?: string[];
  providerAccentColors?: Record<string, string | null>;
}

/**
 * One quota warning window. All fields absent = Global (inherit the global
 * thresholds). `enabled: false` = Off (values kept for when it turns back on).
 * Otherwise Custom; a missing value inherits the global one.
 */
export interface QuotaWarningWindow {
  enabled?: boolean;
  high?: number;
  critical?: number;
}

export interface ProviderQuotaWarnings {
  session?: QuotaWarningWindow;
  weekly?: QuotaWarningWindow;
}

/** One provider row for Settings → Usage & Spend. */
export interface UsageSpendDailyPoint {
  day: string;
  amount: number;
}

export interface UsageSpendRow {
  providerId: string;
  displayName: string;
  sevenDay: number | null;
  thirtyDay: number | null;
  sevenDayEstimate?: LocalCostEstimate;
  thirtyDayEstimate?: LocalCostEstimate;
  sevenDayTokens?: number | null;
  thirtyDayTokens?: number | null;
  /** The token figure is a floor from an incomplete scan ("at least N"). */
  sevenDayTokensLowerBound?: boolean;
  thirtyDayTokensLowerBound?: boolean;
  /** Cost over the selected History window (`UsageSpendSummary.reportingPeriod`). */
  periodCost: number | null;
  periodTokens: number | null;
  currency: string;
  source: string;
  includedInOverview: boolean;
  daily?: UsageSpendDailyPoint[];
  /** F8: true when served from stale cache while a re-scan is in progress. */
  refreshing?: boolean;
  /** ISO 8601 timestamp of the stale snapshot when refreshing. */
  staleUpdatedAt?: string;
}

export interface LocalCostEstimate {
  knownSubtotalUsd: number | null;
  coverage: CostCoverageCounts;
}

export interface UsageSpendSummary {
  rows: UsageSpendRow[];
  contract: SpendContract;
  /** Raw History window the `period*` columns were built for. */
  reportingPeriod: string;
  reportingDay: string;
  dashboardTimezone: string;
  /** False when "Track costs" is off: nothing was scanned and the summary is empty. */
  costTrackingEnabled?: boolean;
  /** The last year of per-day tokens per local-log row, independent of the period. */
  tokenActivity?: TokenActivitySource[];
  /** Per-source spend series for the trend explorer; absent while Track costs is off. */
  trend?: SpendTrend;
}

export interface TokenActivityDay {
  day: string;
  /** null is unknown: never scanned, or scanned and still not totalled. */
  totalTokens: number | null;
  isScanned: boolean;
}

export interface TokenActivitySource {
  providerId: string;
  /** Oldest day first; the last entry is today in the cost bucket zone. */
  days: TokenActivityDay[];
}

export interface SpendTrendSource {
  id: string;
  providerId: string;
  displayName: string;
  /** The whole window is a covered, priced zero for this source. */
  knownZero: boolean;
}

export interface SpendTrendDailyPoint {
  sourceId: string;
  day: string;
  /** null is unknown (unscanned or unpriced); 0 is a covered zero. */
  costUsd: number | null;
}

export interface SpendTrendHourlyPoint {
  sourceId: string;
  /** RFC 3339 UTC instant the local hour began. */
  hour: string;
  costUsd: number | null;
}

export interface SpendTrendHourSlot {
  start: string;
  offsetMinutes: number;
}

/** One local day with hourly data, as its real (23, 24 or 25) hour slots. */
export interface SpendTrendHourlyDay {
  day: string;
  start: string;
  end: string;
  hours: SpendTrendHourSlot[];
}

export interface SpendTrend {
  /** First and last reporting day of the window, inclusive. */
  startDay: string;
  endDay: string;
  timeZone: string;
  sources: SpendTrendSource[];
  daily: SpendTrendDailyPoint[];
  hourly: SpendTrendHourlyPoint[];
  hourlyDays: SpendTrendHourlyDay[];
}

export type CostProvenance = "listPriceEstimate" | "vendorMetered" | "mixed" | "unknown";

export interface CostCoverageCounts {
  priced: number;
  unpriced: number;
  unmetered: number;
  estimated: number;
}

export interface SpendTokenMix {
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  cacheCreationTokens: number | null;
  reasoningTokens: number | null;
}

export interface SpendModelRow {
  model: string;
  costUsd: number | null;
  inputTokens: number;
  outputTokens: number;
  cacheReadTokens: number;
  totalTokens: number;
  customPricing: boolean;
}

export interface SpendDailyPoint {
  day: string;
  costUsd: number | null;
  totalTokens: number | null;
}

export interface SpendActivityCell {
  weekday: number;
  hour: number;
  conversations: number;
}

export interface ImportedSpendSource {
  sourceId: string;
  displayName: string;
  requestCount: number;
  conversationCount: number;
  tokenMix: SpendTokenMix;
  coverage: CostCoverageCounts;
  models: SpendModelRow[];
  hourlyActivity: SpendActivityCell[];
}

/** Codex local Workspaces snapshot (get_codex_workspaces_snapshot). */
export type CodexWorkspacesSourceStatus =
  | "complete"
  | "catalogMissing"
  | "catalogLocked"
  | "catalogCorrupt"
  | "catalogIncompatible";

export interface CodexWorkspacesUsageTotals {
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  totalTokens: number;
}

export interface CodexWorkspacesCostEstimate {
  knownUsd: number;
  unknownTokens: number;
}

export interface CodexWorkspacesDailyPoint {
  day: string;
  totalTokens: number;
  cachedInputTokens: number;
  estimatedCostUsd: number | null;
}

/** Turns that share one model and reasoning effort (upstream #4304). */
export interface CodexWorkspacesTurnPerformanceGroup {
  model: string | null;
  reasoningEffort: string | null;
  sampleCount: number;
  firstTokenSampleCount: number;
  medianFirstTokenMilliseconds: number | null;
  medianDurationMilliseconds: number;
  outputTokensPerSecond: number;
}

export interface CodexWorkspacesTurnPerformanceDetails {
  p95DurationMilliseconds: number | null;
  p95FirstTokenMilliseconds: number | null;
  outputRateLowerQuartile: number | null;
  outputRateUpperQuartile: number | null;
  cachedInputFraction: number | null;
  cacheSampleCount: number;
  groups: CodexWorkspacesTurnPerformanceGroup[];
}

/** Native Codex turn latency and throughput for one conversation. */
export interface CodexWorkspacesTurnPerformance {
  sampleCount: number;
  outputTokensPerSecond: number;
  medianDurationMilliseconds: number;
  medianFirstTokenMilliseconds: number | null;
  firstTokenSampleCount: number;
  details: CodexWorkspacesTurnPerformanceDetails;
}

export interface CodexWorkspacesSessionUsage {
  id: string;
  projectId: string;
  displayTitle: string;
  cwd: string | null;
  startedAt: string | null;
  latestActivity: string | null;
  totals: CodexWorkspacesUsageTotals;
  costEstimate: CodexWorkspacesCostEstimate;
  topModel: string | null;
  turnPerformance: CodexWorkspacesTurnPerformance | null;
}

export interface CodexWorkspacesProjectUsage {
  id: string;
  displayName: string;
  path: string | null;
  totals: CodexWorkspacesUsageTotals;
  costEstimate: CodexWorkspacesCostEstimate;
  sessionCount: number;
  latestActivity: string | null;
  topModel: string | null;
  topSessions: CodexWorkspacesSessionUsage[];
  /** Explicit desktop ownership: an independent chat, not a project folder. */
  isProjectless: boolean;
}

export interface CodexLocalProjectUsageSnapshot {
  updatedAt: string;
  historyDays: number;
  scopeSignature: string;
  indexedFileCount: number;
  skippedFileCount: number;
  total: CodexWorkspacesUsageTotals;
  /** All indexed conversations in the selected history window. */
  sessions: CodexWorkspacesSessionUsage[];
  projects: CodexWorkspacesProjectUsage[];
  daily: CodexWorkspacesDailyPoint[];
  sourceStatus: CodexWorkspacesSourceStatus;
}


export interface SpendContract {
  providerId: string;
  historyDays: number;
  /** Raw History window this contract was built for. */
  reportingPeriod: string;
  knownCostUsd: number | null;
  knownZero: boolean;
  provenance: CostProvenance;
  priceCoverage: CostCoverageCounts;
  priceCoverageRatio: number | null;
  historyCoverageEstablished: boolean;
  tokenMix: SpendTokenMix;
  conversationCount: number;
  models: SpendModelRow[];
  projects: CodexWorkspacesProjectUsage[];
  conversations: CodexWorkspacesSessionUsage[];
  daily: SpendDailyPoint[];
  hourlyActivity: SpendActivityCell[];
  projectSourceStatus: CodexWorkspacesSourceStatus | null;
  customPricingActive: boolean;
  imports: ImportedSpendSource[];
}


export interface BootstrapState {
  contractVersion: string;
  providers: ProviderCatalogEntry[];
  settings: SettingsSnapshot;
}

// ── Provider usage snapshot types ────────────────────────────────────

/**
 * A longer exhausted pool (Kimi monthly, upstream 0.69.0 #4091) blocks the
 * window until `resetsAt`; `null` means the reset is unknown and the block
 * holds. Surfaces re-check it at render time because snapshots are cached.
 */
export interface MonthlyLimitBlock {
  resetsAt: string | null;
}

export interface RateWindowSnapshot {
  usedPercent: number;
  remainingPercent: number;
  windowMinutes: number | null;
  resetsAt: string | null;
  resetDescription: string | null;
  isExhausted: boolean;
  isInformational?: boolean;
  /** `resetDescription` is a detail line (for example spend amounts), not reset wording. */
  descriptionIsDetail?: boolean;
  /** The reset is the next quota regeneration: the card says "Regenerates in" (Synthetic). */
  resetRegenerates?: boolean;
  /** The window feeds the tray and float bar only; the card draws no row for it. */
  hiddenOnCard?: boolean;
  reservePercent: number | null;
  reserveDescription: string | null;
  reserveWillLastToReset?: boolean;
  reserveEtaSeconds?: number | null;
  /** Pace of this window; absent when the provider's pace is not authoritative or the window is informational. */
  pace?: WindowPaceSnapshot | null;
  /** Set while a longer exhausted pool blocks this window; raw percentages stay the provider data. */
  monthlyLimitBlock?: MonthlyLimitBlock | null;
}

export interface CostDailyPoint {
  day: string;
  amount: number;
}

export interface CostSnapshotBridge {
  used: number;
  limit: number | null;
  remaining: number | null;
  currencyCode: string;
  /** Optional currency symbol (e.g. "€", "$", "¥") for localized rendering. */
  currencySymbol?: string | null;
  period: string;
  resetsAt: string | null;
  formattedUsed: string;
  formattedLimit: string | null;
  balance?: number | null;
  /** Successful balance observation time; independent from the usage-cap age. */
  balanceUpdatedAt?: string | null;
  /** Stable provider account scope for reconciling paired observations. */
  accountId?: string | null;
  formattedBalance?: string | null;
  daily?: CostDailyPoint[];
  /** Provider-metered spend that is itself a primary usage signal. */
  alwaysVisible?: boolean;
  /** The primary meter already shows this cap, so the cost block omits its bar. */
  foldedIntoPrimary?: boolean;
  /** Provider-owned cost-block title (upstream `ProviderCostMenuCardStyle`); absent keeps the shared title. */
  titleKey?: LocaleKey;
}

export type PaceStage =
  | "on_track"
  | "slightly_ahead"
  | "ahead"
  | "far_ahead"
  | "slightly_behind"
  | "behind"
  | "far_behind";

/** Pace of one rate window. A positive `deltaPercent` is a deficit, a negative one a reserve. */
export interface WindowPaceSnapshot {
  stage: PaceStage;
  deltaPercent: number;
  expectedUsedPercent: number;
  actualUsedPercent: number;
  etaSeconds?: number | null;
  willLastToReset: boolean;
  /**
   * Scored against the provider's reset window (upstream
   * `ProviderPaceCapability.resetWindowPace`). Card lanes show it as weekly
   * pace whatever the window's length. Omitted when false.
   */
  resetWindow?: boolean;
}

export interface PaceSnapshot {
  stage: PaceStage;
  deltaPercent: number;
  willLastToReset: boolean;
  etaSeconds: number | null;
  expectedUsedPercent: number;
  actualUsedPercent: number;
  /** Block of the window this pace comes from; no pace is shown while it is active. */
  monthlyLimitBlock?: MonthlyLimitBlock | null;
}

/** One burndown chart point (RFC 3339 capture time + remaining percent). */
export interface QuotaBurndownPoint {
  capturedAt: string;
  remainingPercent: number;
}

/** Recorded remaining-quota burndown for one series (upstream 0.70.0 #4085). */
export interface QuotaBurndownSnapshot {
  series: "session" | "weekly";
  windowMinutes: number;
  start: string;
  reset: string;
  samples: QuotaBurndownPoint[];
  ideal: [QuotaBurndownPoint, QuotaBurndownPoint];
}

export interface SessionEquivalentForecastSnapshot {
  estimatedWindowsToExhaustWeekly: number;
  windowsUntilReset: number;
  availableWindowsUntilReset: number;
  sampleCount: number;
  weeklyResetsAt: string;
  weeklyUsedPercent: number;
}

export interface SubscriptionMetadataSnapshot {
  startsAt: string | null;
  expiresAt: string | null;
  renewsAt: string | null;
  /** Calendar-only dates are UTC midnight; format them in UTC. */
  renewsAtIsDateOnly?: boolean;
  expiresAtIsDateOnly?: boolean;
}

export interface ProviderInventoryItem {
  id: string;
  title: string;
  availableCount: number;
  nextExpiresAt: string | null;
}

export interface ProviderDisplayProgress {
  used: number;
  total: number;
}

/** Transient provider detail row; it is display-only and never quota math. */
export interface ProviderDisplayDetail {
  id: string;
  sectionTitle: string | null;
  title: string;
  value: string;
  secondaryValue: string | null;
  progress: ProviderDisplayProgress | null;
}

/** One metric or provider-emitted extra row available to visibility controls. */
export interface ProviderUsageItem {
  id: string;
  title: string;
  available: boolean;
  /** Also suppressed while Cost summary display is Hidden. */
  requiresCostSummary?: boolean;
}
/** Backend-classified provider availability state (camelCase serde on the bridge). */
export type ProviderStateKind =
  | "ready"
  | "needsAuthentication"
  | "expiredSession"
  | "localRuntimeOffline"
  | "unknown";

export interface ProviderUsageSnapshot {
  providerId: string;
  displayName: string;
  primary: RateWindowSnapshot;
  /** Settings-selected metric shared by native and webview presentation surfaces. */
  selectedMetric: RateWindowSnapshot;
  primaryLabel?: string;
  secondary: RateWindowSnapshot | null;
  secondaryLabel?: string;
  /** Provider-chosen locale key that names the secondary lane (wins over `secondaryLabel`). */
  secondaryLabelKey?: LocaleKey | null;
  modelSpecific: RateWindowSnapshot | null;
  tertiary: RateWindowSnapshot | null;
  /** F5: duration-cadence label for tertiary ("monthly", "weekly" etc.) */
  tertiaryLabel?: string;
  extraRateWindows: Array<{
    id: string;
    title: string;
    window: RateWindowSnapshot;
    /** Provider-declared fallback lane; only fills in without a core quota window. */
    fallbackLane?: boolean;
    /** Provider-declared tray-icon lane this window stands in for when that core lane is absent. */
    iconFallback?: "primary" | "secondary";
  }>;
  /** Display-only discrete provider inventory; never used as quota math. */
  inventory?: ProviderInventoryItem[];
  /** Provider-specific display rows; never used as quota math or persistence. */
  displayDetails?: ProviderDisplayDetail[];
  /** Presentation-only hidden metric/extra row IDs. */
  hiddenUsageItemIds?: string[];
  cost: CostSnapshotBridge | null;
  planName: string | null;
  accountEmail: string | null;
  /**
   * Active token-account label, shown in the header only when the provider
   * reports no email (upstream account fallback). Never a scoping key.
   */
  accountLabel?: string | null;
  subscription?: SubscriptionMetadataSnapshot | null;
  sourceLabel: string;
  /** Backend proof of a live successful Claude CLI quota fetch; only true is proof. */
  hasSuccessfulClaudeCliQuota?: boolean;
  updatedAt: string;
  error: string | null;
  errorState: ProviderStateKind;
  pace: PaceSnapshot | null;
  accountOrganization: string | null;
  trayStatusLabel: string | null;
  fetchDurationMs?: number | null;
  wayfinderUsage?: WayfinderUsageSnapshot | null;
  /** Per-UTC-day OpenAI Admin API history; only the `openaiapi` Admin path sets it. */
  openAiApiUsage?: OpenAiApiUsageSnapshot | null;
  /** Last-hour Tokens/min grid; only the `openaiapi` Admin path sets it. */
  throughput?: ProviderThroughputSnapshot | null;
  sessionEquivalentForecast?: SessionEquivalentForecastSnapshot | null;
  /** Recorded remaining-quota burndown; Codex and Claude only. */
  quotaBurndown?: QuotaBurndownSnapshot | null;
}

export interface WayfinderRouteSummary {
  name: string;
  requests: number;
  tokens: number;
  realized: number;
  baseline: number;
  saved: number;
}

export interface WayfinderUsageSnapshot {
  gatewayStatus: string;
  offline: boolean;
  dryRun: boolean;
  missingKeys: string[];
  modelCount: number;
  models: string[];
  requests: number;
  estimatedRequests: number;
  tokens: number;
  realized: number;
  baseline: number;
  saved: number;
  savedPercent: number;
  periodDays: number;
  unit: string;
  priced: boolean;
  routes: WayfinderRouteSummary[];
  /** Mean router decision latency in ms, from the gateway's /metrics. */
  avgDecisionMs?: number;
}

/** Line item cost for one UTC day; descending by cost, then name. */
export interface OpenAiApiLineItemSnapshot {
  name: string;
  costUsd: number;
}

/** Model usage for one UTC day; descending by total tokens, then name. */
export interface OpenAiApiModelUsageSnapshot {
  name: string;
  requests: number;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  totalTokens: number;
}

/**
 * One UTC-day bucket. Input and output include audio tokens, cached input is a
 * subset of input, and `totalTokens === inputTokens + outputTokens`.
 */
export interface OpenAiApiDailyUsageSnapshot {
  /** Bucket start, epoch seconds. */
  startTime: number;
  /** Bucket end, epoch seconds; always after `startTime`. */
  endTime: number;
  costUsd: number;
  requests: number;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  totalTokens: number;
  lineItems: OpenAiApiLineItemSnapshot[];
  models: OpenAiApiModelUsageSnapshot[];
}

export interface OpenAiApiUsageSnapshot {
  /** Requested window in days (1-365). */
  historyDays: number;
  projectId: string | null;
  /** Ascending by `startTime`; empty when the window had no data. */
  daily: OpenAiApiDailyUsageSnapshot[];
}

/** One minute of the Tokens/min grid. */
export interface ThroughputMinuteSnapshot {
  /** Minute start, epoch seconds. */
  start: number;
  totalTokens: number;
  requests: number;
  /** After the reporting frontier: not reported yet, never a real zero or a peak. */
  isPending: boolean;
}

/** Per-model throughput, ranked by reported volume, then pending volume, then name. */
export interface ThroughputModelSnapshot {
  name: string;
  latestTpm: number;
  peakTpm: number;
  totalTokens: number;
  /** Peak non-pending tokens that count toward a rate limit (Batch API excluded). */
  peakLimitCountedTokens: number;
  /** Limit-counted tokens aligned with `ProviderThroughputSnapshot.minutes`. */
  limitCountedTokensByMinute: number[];
}

export interface ProviderThroughputSnapshot {
  /** Regular ascending one-minute grid. */
  minutes: ThroughputMinuteSnapshot[];
  /** Newest complete minute with reported traffic, epoch seconds. */
  frontier: number | null;
  latestTokensPerMinute: number | null;
  peakTokensPerMinute: number;
  peakAt: number | null;
  fiveMinuteAverageTokensPerMinute: number;
  models: ThroughputModelSnapshot[];
  windowMinutes: number;
  /** Epoch seconds. */
  fetchedAt: number;
  /** Project scope; `null` covers the whole organization (only then can limits apply). */
  projectId: string | null;
}

/** Manual organization limit for one model (OpenAI API Live Usage, upstream #4495). */
export interface ModelRateLimitSnapshot {
  model: string;
  tokensPerMinute: number;
  /** `null` or absent means no requests-per-minute limit. */
  requestsPerMinute?: number | null;
}

export interface OrgModelRateLimitSnapshot {
  model: string;
  tokensPerMinute: number;
  requestsPerMinute: number | null;
}

/** Project caps read across the organization, sorted by model ID. */
export interface OrgRateLimitsSnapshot {
  byModel: OrgModelRateLimitSnapshot[];
  projectsSampled: number;
  truncated: boolean;
  /** Epoch seconds. */
  fetchedAt: number;
}

export interface RefreshCompletePayload {
  providerCount: number;
  errorCount: number;
}

export interface RefreshStartedPayload {
  providerIds: string[];
}

export interface CredentialStorageStatus {
  manualCookies: string;
  apiKeys: string;
  tokenAccounts: string;
}

// ── Update state types ───────────────────────────────────────────────

export type UpdateStatus =
  | "idle"
  | "checking"
  | "available"
  | "downloading"
  | "ready"
  | "error";

export interface UpdateStatePayload {
  status: UpdateStatus;
  version: string | null;
  error: string | null;
  progress: number | null;
  releaseUrl: string | null;
  canDownload: boolean;
  canApply: boolean;
  /** Unix-ms timestamp of the last completed update check, or `null`
   *  if the app has not checked during this session. */
  lastCheckedAt: number | null;
}

// ── Credential store types ───────────────────────────────────────────

export interface ApiKeyInfoBridge {
  providerId: string;
  provider: string;
  maskedKey: string;
  savedAt: string;
  label: string | null;
}

export interface ApiKeyProviderInfoBridge {
  id: string;
  displayName: string;
  envVar: string | null;
  help: string | null;
  dashboardUrl: string | null;
}

export interface CookieInfoBridge {
  providerId: string;
  provider: string;
  savedAt: string;
}

export interface DetectedBrowserBridge {
  browserType: string;
  displayName: string;
  profileCount: number;
}

export interface AppInfoBridge {
  name: string;
  version: string;
  buildNumber: string;
  /** UTC build date, YYYY-MM-DD. */
  buildDate?: string;
  updateChannel: string;
  tagline: string;
}

// ── Chart data types ─────────────────────────────────────────────────

export interface DailyCostPoint {
  date: string;
  value: number | null;
  /** Claude requests excluded from this day's totals (upstream 0.60.5 #3688). */
  incompleteRequestCount?: number;
}

/** Exact local token totals per day (upstream 0.50.0 #2930). */
export interface DailyTokenPoint {
  date: string;
  tokens: number;
}

export interface ServiceUsagePoint {
  service: string;
  creditsUsed: number;
}

export interface DailyUsageBreakdown {
  day: string;
  services: ServiceUsagePoint[];
  totalCreditsUsed: number;
}

export interface ProviderLocalUsageSummary {
  todayCost: number | null;
  /** Always the trailing 30 days. */
  thirtyDayCost: number | null;
  thirtyDayTokens: number | null;
  /** Selected History window totals. */
  periodCost: number | null;
  periodTokens: number | null;
  reportingPeriod: string;
  latestTokens: number | null;
  topModel: string | null;
  estimateNote: string;
  tokenCostUpdatedAtMs: number;
  /** Claude requests excluded from the selected-period totals (upstream 0.60.5 #3688). */
  incompleteRequestCount?: number;
}

export interface QuotaWindowHistoryPoint {
  offset: number;
  start: string;
  end: string;
  totalTokens: number | null;
  totalCostUsd: number | null;
  tokensAreComplete: boolean;
  costIsComplete: boolean;
  entryCount: number;
  boundariesAreEstimated: boolean;
}

export interface QuotaWindowHistoryBridge {
  providerId: string;
  accountScope: string | null;
  windows: QuotaWindowHistoryPoint[];
  historyCoverageEstablished: boolean;
}

export interface ProviderChartData {
  providerId: string;
  costHistory: DailyCostPoint[];
  creditsHistory: DailyCostPoint[];
  usageBreakdown: DailyUsageBreakdown[];
  localUsage: ProviderLocalUsageSummary | null;
  tokensHistory: DailyTokenPoint[];
  tokensIncomplete: boolean;
  quotaWindowHistory?: QuotaWindowHistoryBridge | null;
}

// ── Token account types ──────────────────────────────────────────────

export interface TokenAccountSupportBridge {
  providerId: string;
  displayName: string;
  title: string;
  subtitle: string;
  placeholder: string;
}

export interface TokenAccountBridge {
  id: string;
  label: string;
  addedAt: string;
  lastUsed: string | null;
  isActive: boolean;
}

export interface ProviderTokenAccountsBridge {
  providerId: string;
  support: TokenAccountSupportBridge;
  accounts: TokenAccountBridge[];
  activeIndex: number;
}

// ── Phase 4 — provider ordering / cookie source / region ─────────────

export interface ProviderSummary {
  id: string;
  displayName: string;
  enabled: boolean;
  order: number;
}

// ── Phase 4 — credential detection ───────────────────────────────────

export interface GeminiCliStatus {
  signedIn: boolean;
  credentialsPath: string | null;
}

export interface VertexAiStatus {
  hasCredentials: boolean;
  credentialsPath: string | null;
}

export interface JetbrainsIde {
  id: string;
  displayName: string;
  path: string;
  detected: boolean;
}

export interface KiroStatus {
  available: boolean;
  hint: string | null;
}

// ── Phase 4 — session / environment ──────────────────────────────────

export interface WorkAreaRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

// ── Phase 5 — i18n ────────────────────────────────────────────────────

/** Snapshot returned by `get_locale_strings`. */
export interface LocaleStrings {
  language: Language;
  entries: Record<string, string>;
}

/** Payload emitted for `locale-changed`: the persisted language label. */
export type LocaleChangedPayload = Language;

// ── Phase 6b — provider detail pane ──────────────────────────────────

/** Aggregated per-provider payload powering the Settings detail pane. */
export interface ProviderDetail {
  id: string;
  displayName: string;
  enabled: boolean;
  autoResumeAfterQuotaReset: boolean;
  /** Whether the active credential lane can be correlated to a local CLI session. */
  autoResumeSupported: boolean;
  /** LiteLLM and Claude expose one opt-in extra breakdown; other providers do not. */
  optionalDetailsSupported: boolean;
  optionalDetailsEnabled: boolean;

  // Identity
  email: string | null;
  plan: string | null;
  authType: string | null;
  sourceLabel: string | null;
  organization: string | null;
  lastUpdated: string | null;

  // Usage windows — mirror RateWindowSnapshot.
  session: RateWindowSnapshot | null;
  weekly: RateWindowSnapshot | null;
  /** Provider-declared label for the session (primary) lane, e.g. "Personal budget". */
  primaryLabel?: string | null;
  /** Provider-declared label for the weekly (secondary) lane, e.g. "Team budget". */
  secondaryLabel?: string | null;
  modelSpecific: RateWindowSnapshot | null;
  tertiary: RateWindowSnapshot | null;
  /** Locale key for the tertiary metric lane when it carries a semantic label (upstream F5). */
  tertiaryLabelKey?: string | null;
  /** Extra rate window id holding the monthly plan allowance; offers the Monthly Plan metric (upstream 0.70.0). */
  monthlyPlanWindowId?: string | null;
  /** Metric picker label for the primary lane when it is not a session window. */
  primaryMetricLabel?: string | null;
  extraRateWindows: Array<{
    id: string;
    title: string;
    window: RateWindowSnapshot;
    /** Provider-declared fallback lane; only fills in without a core quota window. */
    fallbackLane?: boolean;
    /** Provider-declared tray-icon lane this window stands in for when that core lane is absent. */
    iconFallback?: "primary" | "secondary";
  }>;
  /** Metric and extra rows exposed by the current provider snapshot. */
  usageItems?: ProviderUsageItem[];
  /** Persisted presentation-only hidden metric/extra row IDs. */
  hiddenUsageItemIds?: string[];
  /** Display-only discrete provider inventory; never used as quota math. */
  inventory?: ProviderInventoryItem[];
  /** Provider-specific display rows; never used as quota math or persistence. */
  displayDetails?: ProviderDisplayDetail[];
  /** Plan renewal / expiry dates for the usage preview. */
  subscription?: SubscriptionMetadataSnapshot | null;

  cost: CostSnapshotBridge | null;
  pace: PaceSnapshot | null;
  /** Per-UTC-day OpenAI Admin API history; only the `openaiapi` Admin path sets it. */
  openAiApiUsage?: OpenAiApiUsageSnapshot | null;

  lastError: string | null;
  errorState: ProviderStateKind | null;

  dashboardUrl: string | null;
  statusPageUrl: string | null;
  buyCreditsUrl: string | null;

  hasSnapshot: boolean;

  /** Persisted provider usage source (auto | cli | oauth | web). */
  usageSource?: string | null;
  /** Phase 6c — currently-persisted cookie source value ("auto" | "manual" | "off" | …).
   *  `null` for providers that do not expose a cookie-source picker. */
  cookieSource: string | null;
  /** Manual cookie source is selected with no usable header, and the provider
   *  fails closed instead of importing browser cookies. */
  manualCookieMissing?: boolean;
  /** Phase 6c — currently-persisted region value. `null` for non-regional providers. */
  region: string | null;
}

// ── Phase 6c — cookie-source & region pickers ────────────────────────

export interface CookieSourceOption {
  value: string;
  label: string;
  description?: string;
}

export interface RegionOption {
  value: string;
  label: string;
}

// ── Codex multi-account (ADR 0003) ───────────────────────────────────

export type CodexAccountSource = "ambient" | "managedByApp";

export interface CodexAccount {
  id: string;
  nickname: string | null;
  emailHint: string | null;
  authSubject: string | null;
  providerAccountId: string | null;
  codexHomePath: string;
  source: CodexAccountSource;
  createdAt: string;
  updatedAt: string;
  lastAuthenticatedAt: string | null;
}

export interface CodexUsageWindow {
  usedPercent: number;
  resetAt: string | null;
  limitWindowSeconds: number;
}

export interface CodexCreditsBalance {
  hasCredits: boolean;
  unlimited: boolean;
  balance: number | null;
}

export interface CodexAccountUsageSnapshot {
  email: string | null;
  providerAccountId: string | null;
  plan: string | null;
  allowed: boolean | null;
  limitReached: boolean | null;
  primaryWindow: CodexUsageWindow | null;
  secondaryWindow: CodexUsageWindow | null;
  credits: CodexCreditsBalance | null;
  /** Persisted account-scoped extra-usage cost, when available. */
  cost?: CostSnapshotBridge | null;
  subscription?: SubscriptionMetadataSnapshot | null;
  updatedAt: string;
}

export interface CodexSwitchResult {
  switchId: string;
  materializedAccount: CodexAccount | null;
  backupPath: string | null;
  ambientAccount: CodexAccount | null;
  desktopSessionBackupPath: string | null;
  desktopSessionRestorePath: string | null;
  desktopSessionRestoreExists: boolean;
}

export interface CodexAccountsStateBridge {
  accounts: CodexAccount[];
  /** Canonical privacy-safe account labels, keyed by stable account id. */
  displayNames?: Record<string, string>;
  /** Canonical opaque account ordinals, keyed by stable account id. */
  accountOrdinals: Record<string, number>;
  snapshots: Record<string, CodexAccountUsageSnapshot>;
  needsAuthentication?: Record<string, boolean>;
}
export interface ClaudeAccount {
  id: string;
  email: string;
  organization: string | null;
  plan: string | null;
  isActive: boolean;
  isSaved: boolean;
  usage?: {
    fiveHour: { usedPercent: number; resetsAt: string | null } | null;
    sevenDay: { usedPercent: number; resetsAt: string | null } | null;
    updatedAt: string;
  } | null;
  usageError?: string | null;
  needsAuthentication?: boolean;
}

export interface ClaudeReconciliationSnapshot {
  generation: number;
  status: "pending" | "succeeded" | "failed";
  providerRefreshGeneration: number | null;
  detail: string;
}

export interface GrokAccount {
  id: string;
  email: string;
  organization: string | null;
  plan: string | null;
  isActive: boolean;
  isSaved: boolean;
}

export interface GrokAccountUsage {
  status?: "loading" | "ready" | "signInRequired" | "unavailable" | "failed";
  usageAvailable: boolean;
  usedPercent: number | null;
  plan: string | null;
  windowMinutes: number | null;
  resetsAt: string | null;
}

/** One source-issued usage window from the external claude-swap adapter. */
export interface ClaudeSwapUsageWindow {
  usedPercent: number;
  /** RFC 3339 timestamp, or null when cswap reported no reset. */
  resetsAt: string | null;
}

export interface ClaudeSwapScopedWindow extends ClaudeSwapUsageWindow {
  /** Display-only provider/model label (e.g. "Fable only"). */
  name: string;
}

export interface ClaudeSwapSpendWindow {
  used: number;
  limit: number;
  usedPercent: number;
  currencyCode: string | null;
  resetsAt: string | null;
}

/** Source-reported historical usage. It never drives current provider state. */
export interface ClaudeSwapHistoricalUsage {
  fiveHour: ClaudeSwapUsageWindow | null;
  sevenDay: ClaudeSwapUsageWindow | null;
  scoped: ClaudeSwapScopedWindow[];
  spend: ClaudeSwapSpendWindow | null;
  fetchedAt: string;
  provenance: "source_reported_last_good";
}

export type ClaudeSwapAccountAction = "switch" | "reauthenticate";

/**
 * One external claude-swap account. Identity is the source-issued numeric slot
 * (`claude-swap:<slot>`); CodexBar never reads or stores its credentials.
 */
export interface ClaudeSwapAccount {
  id: string;
  slot: number;
  /** Privacy-aware display label (alias, email, or `Account N`). */
  label: string;
  email: string | null;
  organization: string | null;
  alias: string | null;
  isActive: boolean;
  action: ClaudeSwapAccountAction | null;
  isDisabled: boolean;
  /** Raw cswap usageStatus label (e.g. "ok", "token_expired"). */
  status: string;
  error: string | null;
  fiveHour: ClaudeSwapUsageWindow | null;
  sevenDay: ClaudeSwapUsageWindow | null;
  scoped: ClaudeSwapScopedWindow[];
  spend: ClaudeSwapSpendWindow | null;
  historicalUsage: ClaudeSwapHistoricalUsage | null;
}

/** External claude-swap adapter state for the Claude accounts settings section. */
export interface ClaudeSwapAccountsState {
  enabled: boolean;
  executableConfigured: boolean;
  accounts: ClaudeSwapAccount[];
  error: string | null;
}

// ── Hooks (Settings > Hooks; Rust `commands/hooks.rs`) ────────────────

/** The seven hook events, in the macOS picker order (`HookEventType.allCases`). */
export const HOOK_EVENT_TYPES = [
  "quota_low",
  "quota_reached",
  "quota_reset",
  "usage_updated",
  "provider_unavailable",
  "provider_recovered",
  "refresh_failed",
] as const;

export type HookEventType = (typeof HOOK_EVENT_TYPES)[number];

/** One rule in `hooks.json` (`HookRulePayload`). */
export interface HookRulePayload {
  enabled: boolean;
  /** Event names stay strings so the backend can reject an unknown one. */
  event: string | null;
  /** Extra events from a hand-edited file; the pane carries them through. */
  events: string[];
  /** Provider id, or null for any provider. */
  provider: string | null;
  /** For `quota_low`: usage fraction 0..1 (edited as 1..100 %). */
  threshold: number | null;
  executable: string;
  arguments: string[];
  timeoutSecs: number;
}

/** What the Hooks pane shows (`HooksConfigSnapshot`). */
export interface HooksConfigSnapshot {
  /** On only when `settings.hooks_enabled` and `hooks.json` `enabled` are both on. */
  enabled: boolean;
  settingsEnabled: boolean;
  fileEnabled: boolean;
  rules: HookRulePayload[];
  path: string;
  /** Set when `hooks.json` exists but can't be read; saving is refused. */
  error: string | null;
  maxRules: number;
  maxArguments: number;
}

/** A save from the Hooks pane (`HooksConfigSave`). `enabled` is sent only by the Enable switch. */
export interface HooksConfigSave {
  enabled?: boolean | null;
  rules: HookRulePayload[];
}
