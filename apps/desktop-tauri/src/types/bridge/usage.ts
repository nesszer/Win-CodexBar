import type { LocaleKey } from "../../i18n/keys";

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
  /**
   * Per-day share of a monthly allowance (upstream `monthlyTargetMetrics`).
   * Independent of pace scoring; omitted outside monthly windows and when
   * usage or the reset is unknown.
   */
  dailyTarget?: DailyTargetSnapshot | null;
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
  /** Pay-as-you-go card (upstream `.payAsYouGoSpend`): period spend and balance lines, no bar. */
  payAsYouGo?: boolean;
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

/** Remaining allowance over the local days left, in `unit` (`%`, `credits` or `requests`). */
export interface DailyTargetSnapshot {
  amount: number;
  unit: string;
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
  /**
   * The quota percentages are a local estimate (upstream
   * `dataConfidence == .estimated`). Sent only when true; the tray prefixes
   * estimated percentages with `~`.
   */
  quotaEstimated?: boolean;
  /**
   * Sent only as `false`, when the fetch does not support pace advice (for
   * example a spend budget). Absent means pace is allowed.
   */
  paceAuthoritative?: boolean;
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
  /** Provider access tokens stored in settings (Vercel, OpenRouter management). */
  accessTokens: string;
}
