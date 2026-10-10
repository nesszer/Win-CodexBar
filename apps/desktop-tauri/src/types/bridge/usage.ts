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
  reservePercent: number | null;
  reserveDescription: string | null;
  reserveWillLastToReset?: boolean;
  reserveEtaSeconds?: number | null;
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
}

export interface PaceSnapshot {
  stage: "on_track" | "slightly_ahead" | "ahead" | "far_ahead" | "slightly_behind" | "behind" | "far_behind";
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

