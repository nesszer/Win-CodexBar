import type {
  AppInfoBridge,
  CodexSessionDetails,
  CodexSessionToolActivity,
  CostCoverageCounts,
  CurrencyRatesSnapshot,
  HooksConfigSnapshot,
  ProviderChartData,
  ProviderDetail,
  ProviderSummary,
  ProviderUsageSnapshot,
  SpendContract,
  UpdateStatePayload,
  UsageSpendRow,
  UsageSpendSummary,
} from "../../types/bridge";
import type { LocalSpendFixture, ProofScenario } from "./scenarios";
import { dayKey } from "./common";
import { shortSessionId } from "../../lib/cardConversations";

const EMPTY_COVERAGE: CostCoverageCounts = { priced: 0, unpriced: 0, unmetered: 0, estimated: 0 };

export function proofUpdateState(): UpdateStatePayload {
  return {
    status: "idle",
    version: null,
    error: null,
    progress: null,
    releaseUrl: null,
    canDownload: false,
    canApply: false,
    lastCheckedAt: null,
  };
}

export function proofAppInfo(version: string): AppInfoBridge {
  return {
    name: "CodexBar",
    version,
    buildNumber: "proof",
    buildDate: "2026-01-01",
    updateChannel: "stable",
    tagline: "May your tokens never run out.",
  };
}

export function proofCurrencyRates(preferredCode: string): CurrencyRatesSnapshot {
  return {
    rates: { USD: 1, EUR: 0.92, GBP: 0.79, JPY: 151.4 },
    supportedCodes: ["USD", "EUR", "GBP", "JPY"],
    preferredCode,
  };
}

export function proofHooksConfig(): HooksConfigSnapshot {
  return {
    enabled: false,
    settingsEnabled: false,
    fileEnabled: true,
    rules: [],
    path: "C:\\Users\\proof\\AppData\\Roaming\\CodexBar\\hooks.json",
    error: null,
    maxRules: 32,
    maxArguments: 16,
  };
}

export function providerSummaries(scenario: ProofScenario): ProviderSummary[] {
  const enabled = new Set(scenario.settings.enabledProviders);
  return scenario.catalog.map((entry, order) => ({
    id: entry.id,
    displayName: entry.displayName,
    enabled: enabled.has(entry.id),
    order,
  }));
}

export function providerDetail(
  scenario: ProofScenario,
  providerId: string,
): ProviderDetail {
  const entry = scenario.catalog.find((candidate) => candidate.id === providerId);
  const snapshot: ProviderUsageSnapshot | undefined = scenario.providers.find(
    (candidate) => candidate.providerId === providerId,
  );
  return {
    id: providerId,
    displayName: entry?.displayName ?? snapshot?.displayName ?? providerId,
    enabled: scenario.settings.enabledProviders.includes(providerId),
    autoResumeAfterQuotaReset: false,
    autoResumeSupported: false,
    optionalDetailsSupported: false,
    optionalDetailsEnabled: false,
    email: snapshot?.accountEmail ?? null,
    plan: snapshot?.planName ?? null,
    authType: snapshot ? snapshot.sourceLabel : null,
    sourceLabel: snapshot?.sourceLabel ?? null,
    organization: snapshot?.accountOrganization ?? null,
    lastUpdated: snapshot?.updatedAt ?? null,
    session: snapshot?.primary ?? null,
    weekly: snapshot?.secondary ?? null,
    primaryLabel: snapshot?.primaryLabel ?? null,
    secondaryLabel: snapshot?.secondaryLabel ?? null,
    modelSpecific: snapshot?.modelSpecific ?? null,
    tertiary: snapshot?.tertiary ?? null,
    // Mirrors `Provider::reports_tokens_per_minute`: only OpenAI API reports throughput.
    tokensPerMinuteMetric: providerId === "openaiapi",
    // Mirrors `Provider::named_extra_metrics`: Cursor declares Grok Bot.
    namedExtraMetrics:
      providerId === "cursor" ? [{ id: "cursor-grok-bot", label: "Grok Bot" }] : [],
    extraRateWindows: snapshot?.extraRateWindows ?? [],
    inventory: snapshot?.inventory ?? [],
    displayDetails: snapshot?.displayDetails ?? [],
    usageItems: [],
    hiddenUsageItemIds: [],
    cost: snapshot?.cost ?? null,
    pace: snapshot?.pace ?? null,
    lastError: snapshot?.error ?? null,
    errorState: snapshot?.errorState ?? null,
    dashboardUrl: `https://${providerId}.example.com/usage`,
    statusPageUrl: null,
    buyCreditsUrl: null,
    hasSnapshot: snapshot !== undefined,
    usageSource: "auto",
    cookieSource: entry?.cookieDomain ? "auto" : null,
    region: null,
  };
}

function localSpend(scenario: ProofScenario, providerId: string): LocalSpendFixture | undefined {
  if (!scenario.settings.enabledProviders.includes(providerId)) return undefined;
  return scenario.localSpend.find((candidate) => candidate.providerId === providerId);
}

export function providerChartData(
  scenario: ProofScenario,
  providerId: string,
  now: Date,
): ProviderChartData {
  const spend = localSpend(scenario, providerId);
  const days = spend ? Array.from({ length: 14 }, (_, index) => 13 - index) : [];
  // A fixed wave keeps the bars varied but identical across reloads.
  const factor = (daysAgo: number) => 0.55 + ((daysAgo * 37) % 10) / 10;
  return {
    providerId,
    costHistory: days.map((daysAgo) => ({
      date: dayKey(now, daysAgo),
      value: spend ? Number((spend.daily * factor(daysAgo)).toFixed(2)) : null,
    })),
    creditsHistory: [],
    usageBreakdown: [],
    localUsage: spend
      ? {
          todayCost: spend.daily,
          thirtyDayCost: Number((spend.daily * 30).toFixed(2)),
          thirtyDayTokens: spend.tokensPerDay * 30,
          periodCost: Number((spend.daily * 30).toFixed(2)),
          periodTokens: spend.tokensPerDay * 30,
          reportingPeriod: scenario.settings.costReportingPeriod ?? "rolling:30",
          latestTokens: spend.tokensPerDay,
          topModel: spend.model,
          estimateNote: "Estimated from local logs",
          tokenCostUpdatedAtMs: now.getTime(),
        }
      : null,
    tokensHistory: days.map((daysAgo) => ({
      date: dayKey(now, daysAgo),
      tokens: spend ? Math.round(spend.tokensPerDay * factor(daysAgo)) : 0,
    })),
    tokensIncomplete: false,
    quotaWindowHistory:
      scenario.quotaWindowHistory?.find((history) => history.providerId === providerId) ?? null,
  };
}

export function spendContract(providerId: string, reportingPeriod: string): SpendContract {
  return {
    providerId,
    historyDays: 30,
    reportingPeriod,
    knownCostUsd: null,
    knownZero: false,
    provenance: "listPriceEstimate",
    priceCoverage: EMPTY_COVERAGE,
    priceCoverageRatio: null,
    historyCoverageEstablished: true,
    tokenMix: {
      inputTokens: null,
      outputTokens: null,
      cacheReadTokens: null,
      cacheCreationTokens: null,
      reasoningTokens: null,
    },
    conversationCount: 0,
    models: [],
    projects: [],
    conversations: [],
    daily: [],
    hourlyActivity: [],
    projectSourceStatus: null,
    customPricingActive: false,
    imports: [],
  };
}

/** Mirrors `get_codex_session_tool_activity`, including its Hide personal information redaction. */
export function codexSessionToolActivity(
  scenario: ProofScenario,
  sessionId: string,
): CodexSessionToolActivity {
  const activity = scenario.codexToolActivity?.find((candidate) => candidate.sessionId === sessionId) ?? {
    sessionId,
    operations: [],
    ignoredRecordCount: 0,
    isPartial: false,
    redacted: false,
  };
  if (!scenario.settings.hidePersonalInfo) return activity;
  return {
    ...activity,
    redacted: true,
    operations: activity.operations.map((operation) => ({ ...operation, name: operation.kind, preview: null })),
  };
}

/**
 * Mirrors `get_codex_session_details`: the fixture's details, or a rejection
 * with the stable error code the shell returns.
 */
export function codexSessionDetails(scenario: ProofScenario, sessionId: string): CodexSessionDetails {
  const error = scenario.codexSessionDetailErrors?.[sessionId];
  if (error !== undefined) throw error;
  const details = scenario.codexSessionDetails?.find((candidate) => candidate.sessionId === sessionId);
  if (!details) throw "unavailable";
  return details;
}

/**
 * The Codex contract with conversations. Like `redact_for_privacy`, Hide
 * personal information replaces every title with `Session <short id>`.
 */
function codexConversationsContract(scenario: ProofScenario, reportingPeriod: string): SpendContract | null {
  const conversations = scenario.codexConversations;
  if (!conversations) return null;
  const masked = scenario.settings.hidePersonalInfo
    ? conversations.map((session) => ({
        ...session,
        displayTitle: `Session ${shortSessionId(session.id)}`,
        cwd: null,
      }))
    : conversations;
  return {
    ...spendContract("codex", reportingPeriod),
    conversationCount: masked.length,
    conversations: masked,
  };
}

/** The Usage & Spend contract: Codex conversations or model rows when a scenario provides them, else the empty all-provider contract. */
function usageSpendContract(scenario: ProofScenario, reportingPeriod: string): SpendContract {
  const conversations = codexConversationsContract(scenario, reportingPeriod);
  if (conversations) return conversations;
  if (scenario.codexSpendModels) {
    return { ...spendContract("codex", reportingPeriod), models: scenario.codexSpendModels };
  }
  return spendContract("all", reportingPeriod);
}

export function usageSpendSummary(scenario: ProofScenario, now: Date): UsageSpendSummary {
  const reportingPeriod = scenario.settings.spendDashboardPeriod ?? "rolling:30";
  const rows: UsageSpendRow[] = scenario.catalog.flatMap((entry) => {
    const spend = localSpend(scenario, entry.id);
    if (!spend) return [];
    return [
      {
        providerId: entry.id,
        displayName: entry.displayName,
        sevenDay: Number((spend.daily * 7).toFixed(2)),
        thirtyDay: Number((spend.daily * 30).toFixed(2)),
        sevenDayTokens: spend.tokensPerDay * 7,
        thirtyDayTokens: spend.tokensPerDay * 30,
        periodCost: Number((spend.daily * 30).toFixed(2)),
        periodTokens: spend.tokensPerDay * 30,
        currency: "USD",
        source: "local",
        includedInOverview: true,
        daily: Array.from({ length: 7 }, (_, index) => ({
          day: dayKey(now, 6 - index),
          amount: spend.daily,
        })),
      },
    ];
  });
  return {
    rows,
    contract: usageSpendContract(scenario, reportingPeriod),
    reportingPeriod,
    reportingDay: dayKey(now, 0),
    dashboardTimezone: "UTC",
    costTrackingEnabled: scenario.settings.costUsageEnabled ?? true,
    tokenActivity: [],
  };
}
