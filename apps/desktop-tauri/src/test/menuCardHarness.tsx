import { beforeEach, vi } from "vitest";
import { render } from "@testing-library/react";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "./localeHarness";
import type { ProviderUsageSnapshot, RateWindowSnapshot } from "../types/bridge";
import MenuCard from "../components/MenuCard";
import { makeRateWindow, makeUsageSnapshot } from "./fixtures";
import { tauriMocks, eventMocks } from "./menuCardMocks";

export function rateWindow(
  usedPercent = 0,
  { exhausted = false, ...rest }: Partial<RateWindowSnapshot> & { exhausted?: boolean } = {},
) {
  return makeRateWindow(usedPercent, { isExhausted: exhausted, reserveWillLastToReset: false, ...rest });
}

export function provider(
  error: string | null,
  usedPercent = 0,
  opts: { exhausted?: boolean; resetDescription?: string | null; resetsAt?: string | null } = {},
): ProviderUsageSnapshot {
  return makeUsageSnapshot("claude", {
    displayName: "Claude",
    primary: rateWindow(usedPercent, opts),
    selectedMetric: rateWindow(usedPercent, opts),
    primaryLabel: "Session",
    sourceLabel: "oauth",
    error,
    errorState: "unknown",
    fetchDurationMs: null,
  });
}

export function renderCard(
  snapshot: ProviderUsageSnapshot,
  opts: {
    showAsUsed?: boolean;
    showResetWhenExhausted?: boolean;
    showPace?: boolean;
    compactOverview?: boolean;
    onLayoutChange?: () => void;
    costSummaryDisplayStyle?: "compact" | "detailed" | "hidden";
  } = {},
) {
  return render(
    <LocaleProvider>
      <MenuCard
        provider={snapshot}
        display={{
          hideEmail: false,
          resetTimeRelative: true,
          showAsUsed: opts.showAsUsed,
          showResetWhenExhausted: opts.showResetWhenExhausted,
          showPace: opts.showPace,
          compactOverview: opts.compactOverview,
          costSummaryDisplayStyle: opts.costSummaryDisplayStyle,
        }}
        onLayoutChange={opts.onLayoutChange}
      />
    </LocaleProvider>,
  );
}

export function setupMenuCardTests() {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.claudeAccountsList.mockResolvedValue([]);
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        ActionCopyError: "Copy error",
        ApiSpendTitle: "API spend",
        AtlasCloudBalance: "Atlas Cloud balance",
        DetailPaceRunsOutIn: "Runs out in",
        PanelEstimatedFromLocalLogs: "Estimated from local logs",
        PanelLeftSuffix: "left",
        PanelNow: "now",
        PanelOneHour: "1h",
        PanelFiveHours: "5h",
        PanelBlockedByMonthlyLimit: "Blocked by monthly limit",
        PanelOnPaceBudget: "On-pace budget",
        PanelReserveSuffix: "in reserve",
        PanelPeriodCost: "{} cost",
        PanelPeriodTokens: "{} tokens",
        CostPeriodShortMonthToDate: "MTD",
        CostPeriodShortDays: "{}d",
        PanelTodayBudget: "today",
        PanelUsedSuffix: "used",
        ResetsInHoursMinutes: "Resets in {}h {}m",
        ResetsInMinutes: "Resets in {}m",
        ResetsInDaysHours: "Resets in {}d {}h",
        NextExpiresInHoursMinutes: "Next expires in {}h {}m",
        NextExpiresInMinutes: "Next expires in {}m",
        NextExpiresInDaysHours: "Next expires in {}d {}h",
        NextExpiresDueNow: "Expires now",
        WayfinderGatewayStatus: "Gateway",
        WayfinderModels: "Models",
        WayfinderRequests: "Requests",
        OpenAIChartRequests: "Requests",
        ProviderTextRequests: "{} requests",
        ProviderTextCreditsLeft: "{} credits left",
        ProviderTextBalanceSuffix: "{} balance",
        ProviderTextResetCreditsAvailable: "{} reset credits available",
        ProviderLabelAdditionalBudget: "Additional budget",
        ProviderLabelResetCredits: "Reset credits",
        ProviderLabelTotalUsage: "Total usage",
        WindowLabelHours: "{}-hour",
        WindowLabelDays: "{}-day",
        ProviderTextNoActiveSession: "No active 5h session",
        WayfinderTokens: "Tokens",
        WayfinderSaved: "Saved",
        WayfinderOffline: "Gateway offline",
        WayfinderDryRun: "Dry run",
        WayfinderMissingKeys: "Missing keys",
        UsageSpendTokens: "tokens",
        DeepSeekPricingTitle: "DeepSeek pricing",
        DeepSeekPricingStandard: "Standard / pre-schedule",
        DeepSeekPricingPeak: "Peak hours",
        DeepSeekPricingOffPeak: "Off-peak hours",
        DeepSeekPricingCurrent: "Current local time:",
        DeepSeekPricingNext: "Next transition:",
        DeepSeekPricingEffective: "Effective local time:",
        DeepSeekPricingAdvice: "Official schedule",
      }),
    );
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue(null);
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "claude",
      costHistory: [{ date: "2026-05-24", value: 1.23 }],
      tokensHistory: [{ date: "2026-05-24", tokens: 14_200 }],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: {
        todayCost: null,
        thirtyDayCost: 1.23,
        thirtyDayTokens: 584_000,
        periodCost: 1.23,
        periodTokens: 584_000,
        reportingPeriod: "rolling:30",
        latestTokens: null,
        topModel: "glim-4.6",
        estimateNote: "Estimated from local logs",
        tokenCostUpdatedAtMs: 1234,
      },
    });
    eventMocks.listen.mockResolvedValue(() => {});
  });
}
