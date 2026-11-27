import type { ProviderUsagePresentationSnapshot } from "../generated/ProviderUsagePresentationSnapshot";
import type { WindowPaceSnapshot } from "../generated/WindowPaceSnapshot";

// ── Provider usage snapshot types ────────────────────────────────────
// Generated from Rust by ts-rs; the CI drift gate keeps them in sync.

export type { CostDailyPointBridge } from "../generated/CostDailyPointBridge";
export type { CostSnapshotBridge } from "../generated/CostSnapshotBridge";
export type { DailyTargetSnapshot } from "../generated/DailyTargetSnapshot";
export type { IconLane } from "../generated/IconLane";
export type { MonthlyLimitBlockSnapshot } from "../generated/MonthlyLimitBlockSnapshot";
export type { NamedRateWindowSnapshot } from "../generated/NamedRateWindowSnapshot";
export type { OpenAiApiDailyUsageSnapshot } from "../generated/OpenAiApiDailyUsageSnapshot";
export type { OpenAiApiLineItemSnapshot } from "../generated/OpenAiApiLineItemSnapshot";
export type { OpenAiApiModelUsageSnapshot } from "../generated/OpenAiApiModelUsageSnapshot";
export type { OpenAiApiUsageSnapshot } from "../generated/OpenAiApiUsageSnapshot";
export type { PaceSnapshot } from "../generated/PaceSnapshot";
export type { ProviderDisplayDetailSnapshot } from "../generated/ProviderDisplayDetailSnapshot";
export type { ProviderDisplayProgressSnapshot } from "../generated/ProviderDisplayProgressSnapshot";
export type { ProviderInventoryItemSnapshot } from "../generated/ProviderInventoryItemSnapshot";
export type { ProviderStateKind } from "../generated/ProviderStateKind";
export type { ProviderThroughputSnapshot } from "../generated/ProviderThroughputSnapshot";
export type { ProviderUsagePresentationSnapshot } from "../generated/ProviderUsagePresentationSnapshot";
export type { QuotaBurndownPointSnapshot } from "../generated/QuotaBurndownPointSnapshot";
export type { QuotaBurndownSnapshot } from "../generated/QuotaBurndownSnapshot";
export type { RateWindowSnapshot } from "../generated/RateWindowSnapshot";
export type { SessionEquivalentForecastSnapshot } from "../generated/SessionEquivalentForecastSnapshot";
export type { SubscriptionMetadataSnapshot } from "../generated/SubscriptionMetadataSnapshot";
export type { ThroughputMinuteSnapshot } from "../generated/ThroughputMinuteSnapshot";
export type { ThroughputModelSnapshot } from "../generated/ThroughputModelSnapshot";
export type { WayfinderRouteSummary } from "../generated/WayfinderRouteSummary";
export type { WayfinderUsageSnapshot } from "../generated/WayfinderUsageSnapshot";
export type { WindowPaceSnapshot } from "../generated/WindowPaceSnapshot";

/**
 * The webview only receives the presentation wrapper (`get_cached_providers`,
 * `provider-updated`); the raw generated `ProviderUsageSnapshot` stays in the shell.
 */
export type ProviderUsageSnapshot = ProviderUsagePresentationSnapshot;

export type PaceStage = WindowPaceSnapshot["stage"];

// ── Not generated yet ────────────────────────────────────────────────

/** One metric or provider-emitted extra row available to visibility controls. */
export interface ProviderUsageItem {
  id: string;
  title: string;
  available: boolean;
  /** Also suppressed while Cost summary display is Hidden. */
  requiresCostSummary?: boolean;
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
