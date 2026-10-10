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
export type SettingsSurfaceTarget = { kind: "settings"; tab: SettingsTabId };

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
  providerUsageThresholds?: Record<string, UsageThresholdOverride>;
  predictivePaceWarningEnabled: boolean;
  credentialExpiryNotificationsEnabled: boolean;
  showPace?: boolean;
  trayIconMode: TrayIconMode;
  stackedTrayTopProvider?: string | null;
  stackedTrayBottomProvider?: string | null;
  switcherShowsIcons: boolean;
  menuBarShowsHighestUsage: boolean;
  menuBarShowsPercent: boolean;
  menuBarColorPace: boolean;
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
  /** Optional work-week length [2,6] for session-equivalent weekly forecast. */
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
  providerUsageThresholds?: Record<string, UsageThresholdOverride>;
  predictivePaceWarningEnabled?: boolean;
  credentialExpiryNotificationsEnabled?: boolean;
  showPace?: boolean;
  trayIconMode?: TrayIconMode;
  stackedTrayTopProvider?: string;
  stackedTrayBottomProvider?: string;
  switcherShowsIcons?: boolean;
  menuBarShowsHighestUsage?: boolean;
  menuBarShowsPercent?: boolean;
  menuBarColorPace?: boolean;
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
  agentSessionsEnabled?: boolean;
  stayAwakeEnabled?: boolean;
  agentSessionSshHosts?: string[];
  hooksEnabled?: boolean;
  httpProxyEnabled?: boolean;
  httpProxyUrl?: string;
  httpProxyUsername?: string;
  httpProxyPassword?: string;
  uiLanguage?: Language;
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
  weeklyProgressWorkDays?: number | null;
  costSummaryDisplayStyle?: CostSummaryDisplayStyle;
  openCodexUsageLogsEnabled?: boolean;
  hideNativeCodexCostWhenOpenCodexPresent?: boolean;
  /** `rolling:N` (1..=365), `month-to-date`, or `all`; the backend rejects other values. */
  costReportingPeriod?: string;
  providerAccentColors?: Record<string, string | null>;
}

export interface UsageThresholdOverride {
  high?: number;
  critical?: number;
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

