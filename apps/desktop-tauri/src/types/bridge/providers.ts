import type { Language } from "./settings";
import type {
  CostSnapshotBridge,
  OpenAiApiUsageSnapshot,
  PaceSnapshot,
  ProviderDisplayDetail,
  ProviderInventoryItem,
  ProviderStateKind,
  ProviderUsageItem,
  RateWindowSnapshot,
} from "./usage";

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

