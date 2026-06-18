export type SurfaceMode = "hidden" | "trayPanel" | "popOut" | "settings";
export type VisibleSurfaceMode = Exclude<SurfaceMode, "hidden">;
export type SettingsTabId =
  | "general"
  | "providers"
  | "display"
  | "advanced"
  | "about";

// ── Narrowed string-literal unions (persisted settings enums) ─────────

export type TrayIconMode = "single" | "perProvider";

export type MetricPreference =
  | "automatic"
  | "session"
  | "weekly"
  | "model"
  | "tertiary"
  | "credits"
  | "extraUsage"
  | "average";

export type Language = "english" | "chinese" | "japanese";

export type UpdateChannel = "stable" | "beta";

export type ThemePreference = "auto" | "light" | "dark";

export type MenuBarDisplayMode = "minimal" | "compact" | "detailed";
export type FloatBarOrientation = "horizontal" | "vertical";
export type FloatBarStyle = "floating" | "taskbar";
export type ProofProviderId =
  | "codex"
  | "claude"
  | "cursor"
  | "factory"
  | "gemini"
  | "antigravity"
  | "copilot"
  | "zai"
  | "minimax"
  | "kiro"
  | "vertexai"
  | "augment"
  | "opencode"
  | "kimi"
  | "kimik2"
  | "amp"
  | "warp"
  | "ollama"
  | "azureopenai"
  | "t3chat"
  | "openrouter"
  | "synthetic"
  | "jetbrains"
  | "alibaba"
  | "alibabatokenplan"
  | "nanogpt"
  | "infini"
  | "perplexity"
  | "abacus"
  | "opencodego"
  | "kilo"
  | "bedrock"
  | "mistral"
  | "codebuff"
  | "deepseek"
  | "windsurf"
  | "manus"
  | "mimo"
  | "doubao"
  | "commandcode"
  | "crof"
  | "stepfun"
  | "venice"
  | "openaiapi"
  | "grok"
  | "elevenlabs"
  | "deepgram"
  | "groq"
  | "llmproxy";

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

export interface ProofRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ProofStatePayload {
  mode: SurfaceMode;
  target: SurfaceTarget;
  windowRect: ProofRect | null;
  trayAnchor: ProofRect | null;
  workArea: ProofRect | null;
  menuPath: string | null;
  menuItems: string[];
}

export type ProofCommand =
  | "open-tray-panel"
  | "open-native-menu"
  | "open-dashboard"
  | "open-about-path"
  | "hide-surface"
  | `open-provider:${ProofProviderId}`
  | `open-settings:${SettingsTabId}`;

export interface SurfaceModeDescriptor {
  id: string;
  label: string;
  description: string;
}

export interface BridgeCommandDescriptor {
  id: string;
  description: string;
}

export interface BridgeEventDescriptor {
  id: string;
  description: string;
}

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
  enabledProviders: string[];
  providerOrder?: string[];
  refreshIntervalSecs: number;
  startAtLogin: boolean;
  startMinimized: boolean;
  showNotifications: boolean;
  soundEnabled: boolean;
  soundVolume: number;
  highUsageThreshold: number;
  criticalUsageThreshold: number;
  trayIconMode: TrayIconMode;
  switcherShowsIcons: boolean;
  menuBarShowsHighestUsage: boolean;
  menuBarShowsPercent: boolean;
  showAsUsed: boolean;
  showCreditsExtraUsage: boolean;
  showAllTokenAccountsInMenu: boolean;
  surpriseAnimations: boolean;
  enableAnimations: boolean;
  resetTimeRelative: boolean;
  menuBarDisplayMode: MenuBarDisplayMode;
  hidePersonalInfo: boolean;
  updateChannel: UpdateChannel;
  autoDownloadUpdates: boolean;
  installUpdatesOnQuit: boolean;
  globalShortcut: string;
  uiLanguage: Language;
  theme: ThemePreference;
  claudeAvoidKeychainPrompts: boolean;
  disableKeychainAccess: boolean;
  showDebugSettings: boolean;
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
  /** When true, render the next primary reset inline in each provider pill. */
  floatBarShowResetInline: boolean;
}

/** Partial settings object — only include fields you want to change. */
export interface SettingsUpdate {
  enabledProviders?: string[];
  refreshIntervalSecs?: number;
  startAtLogin?: boolean;
  startMinimized?: boolean;
  showNotifications?: boolean;
  soundEnabled?: boolean;
  soundVolume?: number;
  highUsageThreshold?: number;
  criticalUsageThreshold?: number;
  trayIconMode?: TrayIconMode;
  switcherShowsIcons?: boolean;
  menuBarShowsHighestUsage?: boolean;
  menuBarShowsPercent?: boolean;
  showAsUsed?: boolean;
  showCreditsExtraUsage?: boolean;
  showAllTokenAccountsInMenu?: boolean;
  surpriseAnimations?: boolean;
  enableAnimations?: boolean;
  resetTimeRelative?: boolean;
  menuBarDisplayMode?: MenuBarDisplayMode;
  hidePersonalInfo?: boolean;
  updateChannel?: UpdateChannel;
  autoDownloadUpdates?: boolean;
  installUpdatesOnQuit?: boolean;
  globalShortcut?: string;
  uiLanguage?: Language;
  theme?: ThemePreference;
  claudeAvoidKeychainPrompts?: boolean;
  disableKeychainAccess?: boolean;
  showDebugSettings?: boolean;
  /** Map of provider CLI name → metric preference label. */
  providerMetrics?: Record<string, MetricPreference>;
  floatBarEnabled?: boolean;
  floatBarOpacity?: number;
  floatBarScale?: number;
  floatBarOrientation?: FloatBarOrientation;
  floatBarStyle?: FloatBarStyle;
  floatBarClickThrough?: boolean;
  floatBarProviderIds?: string[];
  floatBarDarkText?: boolean;
  floatBarShowResetInline?: boolean;
}

export interface BootstrapState {
  contractVersion: string;
  surfaceModes: SurfaceModeDescriptor[];
  commands: BridgeCommandDescriptor[];
  events: BridgeEventDescriptor[];
  providers: ProviderCatalogEntry[];
  settings: SettingsSnapshot;
}

// ── Provider usage snapshot types ────────────────────────────────────

export interface RateWindowSnapshot {
  usedPercent: number;
  remainingPercent: number;
  windowMinutes: number | null;
  resetsAt: string | null;
  resetDescription: string | null;
  isExhausted: boolean;
  reservePercent: number | null;
  reserveDescription: string | null;
}

export interface CostSnapshotBridge {
  used: number;
  limit: number | null;
  remaining: number | null;
  currencyCode: string;
  period: string;
  resetsAt: string | null;
  formattedUsed: string;
  formattedLimit: string | null;
}

export interface PaceSnapshot {
  stage: "on_track" | "slightly_ahead" | "ahead" | "far_ahead" | "slightly_behind" | "behind" | "far_behind";
  deltaPercent: number;
  willLastToReset: boolean;
  etaSeconds: number | null;
  expectedUsedPercent: number;
  actualUsedPercent: number;
}

export interface ProviderUsageSnapshot {
  providerId: string;
  displayName: string;
  primary: RateWindowSnapshot;
  primaryLabel?: string;
  secondary: RateWindowSnapshot | null;
  secondaryLabel?: string;
  modelSpecific: RateWindowSnapshot | null;
  tertiary: RateWindowSnapshot | null;
  extraRateWindows: Array<{
    id: string;
    title: string;
    window: RateWindowSnapshot;
  }>;
  cost: CostSnapshotBridge | null;
  planName: string | null;
  accountEmail: string | null;
  sourceLabel: string;
  updatedAt: string;
  error: string | null;
  pace: PaceSnapshot | null;
  accountOrganization: string | null;
  trayStatusLabel: string | null;
  fetchDurationMs?: number | null;
}

export interface RefreshCompletePayload {
  providerCount: number;
  errorCount: number;
}

export interface SafeDiagnostics {
  appVersion: string;
  platform: string;
  enabledProviders: string[];
  providerCookieSources: Record<string, string>;
  hasManualCookies: string[];
  hasApiKeys: string[];
  hidePersonalInfo: boolean;
  refreshIntervalSecs: number;
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
  updateChannel: string;
  tagline: string;
}

// ── Chart data types ─────────────────────────────────────────────────

export interface DailyCostPoint {
  date: string;
  value: number;
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
  thirtyDayCost: number | null;
  thirtyDayTokens: number | null;
  latestTokens: number | null;
  topModel: string | null;
  estimateNote: string;
}

export interface ProviderChartData {
  providerId: string;
  costHistory: DailyCostPoint[];
  creditsHistory: DailyCostPoint[];
  usageBreakdown: DailyUsageBreakdown[];
  localUsage: ProviderLocalUsageSummary | null;
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

// ── Phase 4 — event payloads ─────────────────────────────────────────

/** Payload emitted for the `global-shortcut-triggered` event: the
 *  accelerator string that fired, e.g. `"Ctrl+Shift+U"`. */
export type GlobalShortcutTriggeredPayload = string;

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
  modelSpecific: RateWindowSnapshot | null;
  tertiary: RateWindowSnapshot | null;
  extraRateWindows: Array<{
    id: string;
    title: string;
    window: RateWindowSnapshot;
  }>;

  cost: CostSnapshotBridge | null;
  pace: PaceSnapshot | null;

  lastError: string | null;

  dashboardUrl: string | null;
  statusPageUrl: string | null;
  buyCreditsUrl: string | null;

  hasSnapshot: boolean;

  /** Phase 6c — currently-persisted cookie source value ("auto" | "manual" | "off" | …).
   *  `null` for providers that do not expose a cookie-source picker. */
  cookieSource: string | null;
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

// ── Phase 6d — credential detection ──────────────────────────────────

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
