import type { ClaudeSwapAccountSource } from "./codexAccounts";
import type { ModelRateLimitSnapshot } from "./usage";

// Surface targets, persisted settings, spend reports and bootstrap DTOs.

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
  | "average"
  | "tokensPerMinute"
  /** A provider-declared named extra allowance, by extra rate window id (upstream 0.73.0 #4269). */
  | `extra:${string}`;

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
  /** Hours before the estimated run-out a pace warning may show; 0 = any time. */
  predictivePaceWarningLeadHours: number;
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
  /** Overview shows one row per cached account for selected multi-account providers. */
  mergedOverviewAllAccounts: boolean;
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
  /** Built-in order of the providers the sidebar lists; "Restore default order" is unavailable while the order matches it. Absent on older shells: always available. */
  providerDefaultOrder?: string[];
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
  /** Agent sessions on this PC only: no Tailscale or SSH discovery. */
  agentSessionsLocalOnly?: boolean;
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
  /** Write the usage cache the `codexbar` CLI falls back to. Machine-local. */
  shareUsageWithCli: boolean;
  claudeAvoidKeychainPrompts: boolean;
  /** Opt-in external claude-swap (`cswap`) account import (Claude only). */
  claudeSwapEnabled?: boolean;
  /** Path to the cswap executable (Claude only, empty when unset). */
  claudeSwapExecutablePath?: string;
  /** Which tool the executable path names. Absent on older shells. */
  claudeSwapSource?: ClaudeSwapAccountSource;
  /** Claude web usage reads the Claude desktop app session (automatic cookie source only). */
  claudeDesktopCookiesEnabled?: boolean;
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
  predictivePaceWarningLeadHours?: number;
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
  mergedOverviewAllAccounts?: boolean;
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
  agentSessionsLocalOnly?: boolean;
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
  shareUsageWithCli?: boolean;
  claudeAvoidKeychainPrompts?: boolean;
  claudeAllowReadingClaudeCodeCredentials?: boolean;
  claudeSwapEnabled?: boolean;
  claudeSwapExecutablePath?: string;
  claudeSwapSource?: ClaudeSwapAccountSource;
  claudeDesktopCookiesEnabled?: boolean;
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
  /** null = unknown (rows lacked usable token counts), never a real zero. */
  inputTokens: number | null;
  outputTokens: number | null;
  cacheReadTokens: number | null;
  totalTokens: number | null;
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

export type CodexToolOperationKind =
  | "command"
  | "mcp"
  | "dynamic"
  | "fileChange"
  | "webSearch"
  | "image"
  | "extensionItem";

export type CodexToolOperationOutcome = "completed" | "nonzeroExit" | "toolError" | "declined" | "unknown";

/** Native operations carry their own duration; recorded intervals are labeled. */
export type CodexToolOperationTiming = "native" | "recordedInterval";

/** One native Codex tool operation (upstream #4363). */
export interface CodexToolOperation {
  turnId: string;
  itemId: string;
  kind: CodexToolOperationKind;
  /** `server/tool`, `namespace/tool`, the extension kind, or the item type. */
  name: string;
  preview: string | null;
  completedAt: string;
  outcome: CodexToolOperationOutcome;
  exitCode: number | null;
  /** `null` when no timing was recorded, which is distinct from zero. */
  durationMilliseconds: number | null;
  timing: CodexToolOperationTiming | null;
  needsAttention: boolean;
  isSlow: boolean;
}

/** Tool operations of one conversation, newest first (get_codex_session_tool_activity). */
export interface CodexSessionToolActivity {
  sessionId: string;
  operations: CodexToolOperation[];
  ignoredRecordCount: number;
  isPartial: boolean;
  /** Names and previews were removed for Hide personal information. */
  redacted: boolean;
}

/** The session summary the request-level read must reproduce. */
export interface CodexSessionDetailCheckpoint {
  totals: CodexWorkspacesUsageTotals;
  costEstimate: CodexWorkspacesCostEstimate;
}

/** Stable rejection codes of get_codex_session_details. */
export type CodexSessionDetailError = "unavailable" | "sourceChanged" | "limitExceeded";

/** One recorded model request. Input includes cached input; output includes reasoning. */
export interface CodexSessionRequest {
  id: string;
  modelName: string;
  timestamp: string | null;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningTokens: number | null;
  totalTokens: number;
  /** `null` when the request could not be priced. */
  costUsd: number | null;
}

/** Requests grouped by turn; `turnId: null` is usage no turn owns, always last. */
export interface CodexSessionTurn {
  turnId: string | null;
  firstActivity: string | null;
  lastActivity: string | null;
  inputTokens: number;
  cachedInputTokens: number;
  outputTokens: number;
  reasoningTokens: number | null;
  totalTokens: number;
  requestCount: number;
  costUsd: number | null;
  requests: CodexSessionRequest[];
}

/** Request-level usage of one conversation (get_codex_session_details). */
export interface CodexSessionDetails {
  sessionId: string;
  requestCount: number;
  turns: CodexSessionTurn[];
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

/** Machine-local HTTP server settings (`http_server.json`, DPAPI-protected). */
export interface HttpServerConfig {
  enabled: boolean;
  host: string;
  port: number;
  token: string;
  refreshIntervalSecs: number;
  requestTimeoutSecs: number;
  identity: "redacted" | "full";
  allowPlainHttp: boolean;
}

export type HttpServerValidationError =
  | "invalidHost"
  | "invalidPort"
  | "invalidTiming"
  | "invalidIdentity"
  | "invalidToken"
  | "plainHttpNotAccepted";

export type HttpServerStatus =
  | { state: "stopped" }
  | { state: "running"; url: string }
  | { state: "invalid"; code: HttpServerValidationError }
  | { state: "startFailed"; detail: string }
  | { state: "exited"; detail: string }
  | { state: "loadFailed" }
  | { state: "saveFailed" };

export interface HttpServerSnapshot {
  config: HttpServerConfig;
  status: HttpServerStatus;
}