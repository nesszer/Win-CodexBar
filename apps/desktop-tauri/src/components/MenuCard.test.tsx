import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getProviderChartData: vi.fn(),
  getDeepSeekPricingStatus: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
  claudeAccountsList: vi.fn(),
  getCodexAccountsState: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(),
}));

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  ...tauriMocks,
}));
vi.mock("@tauri-apps/api/event", () => eventMocks);

import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import { loadStyles, ruleBlock } from "../test/styles";
import type { UsageThresholdSettings } from "../lib/metricRowModel";
import type { ProviderUsageSnapshot, WayfinderUsageSnapshot } from "../types/bridge";
import MenuCard from "./MenuCard";

function rateWindow(
  usedPercent = 0,
  opts: {
    exhausted?: boolean;
    resetDescription?: string | null;
    reservePercent?: number | null;
    reserveDescription?: string | null;
    reserveWillLastToReset?: boolean;
    reserveEtaSeconds?: number | null;
    windowMinutes?: number | null;
    resetsAt?: string | null;
  } = {},
) {
  return {
    usedPercent,
    remainingPercent: 100 - usedPercent,
    windowMinutes: opts.windowMinutes ?? null,
    resetsAt: opts.resetsAt ?? null,
    resetDescription: opts.resetDescription ?? null,
    isExhausted: opts.exhausted ?? false,
    reservePercent: opts.reservePercent ?? null,
    reserveDescription: opts.reserveDescription ?? null,
    reserveWillLastToReset: opts.reserveWillLastToReset ?? false,
    reserveEtaSeconds: opts.reserveEtaSeconds ?? null,
  };
}

function provider(
  error: string | null,
  usedPercent = 0,
  opts: { exhausted?: boolean; resetDescription?: string | null; resetsAt?: string | null } = {},
): ProviderUsageSnapshot {
  return {
    providerId: "claude",
    displayName: "Claude",
    primary: rateWindow(usedPercent, opts),
    selectedMetric: rateWindow(usedPercent, opts),
    primaryLabel: "Session",
    secondary: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "oauth",
    updatedAt: "2026-05-24T00:00:00Z",
    error,
    errorState: "unknown",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    fetchDurationMs: null,
  };
}

/** Waits for a metric row whose title starts with `label` (the percent is an inline span). */
function metricTitle(label: string) {
  return waitFor(() => {
    const titles = [...document.querySelectorAll(".menu-metric__title")];
    const match = titles.find((el) => el.firstChild?.textContent === label);
    if (!match) throw new Error(`metric "${label}" not rendered yet`);
    return match;
  });
}

function renderCard(
  snapshot: ProviderUsageSnapshot,
  opts: {
    showAsUsed?: boolean;
    showResetWhenExhausted?: boolean;
    showPace?: boolean;
    compactOverview?: boolean;
    onLayoutChange?: () => void;
    costSummaryDisplayStyle?: "compact" | "detailed" | "hidden";
    usageThresholds?: UsageThresholdSettings | null;
    weeklyProgressWorkDays?: number | null;
    hideEmail?: boolean;
  } = {},
) {
  return render(
    <LocaleProvider>
      <MenuCard
        provider={snapshot}
        display={{
          hideEmail: opts.hideEmail ?? false,
          resetTimeRelative: true,
          showAsUsed: opts.showAsUsed,
          showResetWhenExhausted: opts.showResetWhenExhausted,
          showPace: opts.showPace,
          compactOverview: opts.compactOverview,
          costSummaryDisplayStyle: opts.costSummaryDisplayStyle,
          usageThresholds: opts.usageThresholds,
          weeklyProgressWorkDays: opts.weeklyProgressWorkDays,
        }}
        onLayoutChange={opts.onLayoutChange}
      />
    </LocaleProvider>,
  );
}

describe("MenuCard", () => {
  it("shows no email under Hide Personal Info, in the header or the error line", async () => {
    const snapshot = {
      ...provider(null, 20),
      accountEmail: "parity.user@example.com",
      error: "parity.user@example.com: session expired",
    };
    const { container, unmount } = renderCard(snapshot, { hideEmail: true });
    await waitFor(() => expect(container.textContent).toContain("session expired"));
    expect(container.querySelector(".card-header__email")).toBeNull();
    expect(container.innerHTML).not.toContain("@");
    expect(container.innerHTML).not.toContain("example.com");
    unmount();

    const shown = renderCard(snapshot);
    await waitFor(() =>
      expect(shown.container.querySelector(".card-header__email")).toHaveTextContent(
        "parity.user@example.com",
      ),
    );
  });

  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.claudeAccountsList.mockResolvedValue([]);
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        ActionCopyError: "Copy error",
        ApiSpendTitle: "API spend",
        AtlasCloudAvailableBalance: "Available balance",
        AtlasCloudBalance: "Atlas Cloud balance",
        DetailPaceRunsOutIn: "Runs out in",
        PanelEstimatedFromLocalLogs: "Estimated from local logs",
        PanelLeftSuffix: "left",
        PanelNow: "now",
        PanelOneHour: "1h",
        PanelFiveHours: "5h",
        PanelBlockedByMonthlyLimit: "Blocked by monthly limit",
        PanelOnPaceBudget: "On-pace budget",
        PanelPeriodCost: "{} cost",
        PanelPeriodTokens: "{} tokens",
        CostPeriodShortMonthToDate: "MTD",
        CostPeriodShortDays: "{}d",
        PanelTodayBudget: "today",
        PanelUsedSuffix: "used",
        ResetsInHoursMinutes: "Resets in {}h {}m",
        ResetsInMinutes: "Resets in {}m",
        ResetsInDaysHours: "Resets in {}d {}h",
        ResetsInHoursOnly: "Resets in {}h",
        DurationHoursMinutes: "{}h {}m",
        PaceOnPace: "On pace",
        PaceInReserve: "{}% in reserve",
        PaceInDeficit: "{}% in deficit",
        PaceLastsUntilReset: "Lasts until reset",
        PaceRunsOutIn: "Runs out in {}",
        UsageBarQuotaWarnings: "Quota warnings",
        UsageBarWorkDays: "Work days",
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

  it("shows a Codex authentication error without embedding the account menu", async () => {
    const codex = { ...provider("Authentication required"), providerId: "codex", displayName: "Codex", errorState: "needsAuthentication" as const };
    const { container } = render(<LocaleProvider><MenuCard provider={codex} display={{ hideEmail: false, resetTimeRelative: true, showResetWhenExhausted: true }} /></LocaleProvider>);
    expect(await screen.findByText("Authentication required")).toBeDefined();
    // Account switching moved to Settings > Providers (the "Switch Account..." row).
    expect(screen.queryByRole("button", { name: "CodexAccountsSignInButton" })).not.toBeInTheDocument();
    expect(container.querySelector(".codex-menu-accounts")).not.toBeInTheDocument();
    expect(tauriMocks.getCodexAccountsState).not.toHaveBeenCalled();
  });

  it("keeps Fireworks vendor API spend visible when local cost summaries are hidden", async () => {
    const snapshot = provider(null, 0);
    snapshot.providerId = "fireworks";
    snapshot.displayName = "Fireworks";
    snapshot.cost = {
      used: 12.34,
      limit: null,
      remaining: null,
      currencyCode: "USD",
      currencySymbol: "$",
      period: "30 days",
      resetsAt: null,
      formattedUsed: "$12.34",
      formattedLimit: null,
      balance: null,
      formattedBalance: null,
      daily: [],
      alwaysVisible: true,
    };

    renderCard(snapshot, { costSummaryDisplayStyle: "hidden" });

    expect(await screen.findByText("API spend")).toHaveClass("card-extra__title");
    expect(document.querySelector(".card-extra__spend")).toHaveTextContent("30 days: $12.34");
  });

  it("filters hidden metric and extra rows without changing the snapshot payload", async () => {
    const snapshot = provider(null, 25);
    snapshot.secondary = rateWindow(60);
    snapshot.secondaryLabel = "Weekly";
    snapshot.extraRateWindows = [
      { id: "credits", title: "Credits", window: rateWindow(10) },
    ];
    snapshot.hiddenUsageItemIds = ["metric:primary", "metric:extra-credits"];

    renderCard(snapshot);

    await screen.findByText("ProviderWeeklyLabel");
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
    expect(screen.queryByText("Credits")).not.toBeInTheDocument();
  });
  it("does not mix stale local usage into an error card", async () => {
    const { container } = renderCard(
      provider("OAuth error: Claude OAuth credentials not found."),
    );

    expect(
      await screen.findByText("OAuth error: Claude OAuth credentials not found."),
    ).toBeInTheDocument();
    expect(container.querySelector(".menu-card--header-only")).toBeInTheDocument();
    expect(container.querySelector(".menu-card--with-details")).not.toBeInTheDocument();

    await waitFor(() => {
      expect(tauriMocks.getProviderChartData).toHaveBeenCalled();
    });

    expect(screen.queryByText("30d cost")).not.toBeInTheDocument();
    expect(screen.queryByText("30d tokens")).not.toBeInTheDocument();
    expect(screen.queryByText("Estimated from local logs")).not.toBeInTheDocument();
  });

  it("leaves DeepSeek pricing to Settings > Providers", async () => {
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue({
      period: "offPeak",
      currentLocalTime: "2026-08-17 05:00:00 UTC",
      nextTransitionLocalTime: "2026-08-17 06:00:00 UTC",
      effectiveLocalTime: "2026-08-16 18:00:00 EDT",
    });
    const snapshot = provider(null);
    snapshot.providerId = "deepseek";
    snapshot.displayName = "DeepSeek";

    renderCard(snapshot);

    await metricTitle("ProviderSessionLabel");
    expect(screen.queryByText("DeepSeek pricing: Off-peak hours")).not.toBeInTheDocument();
    expect(tauriMocks.getDeepSeekPricingStatus).not.toHaveBeenCalled();
  });

  it("can render metric bars as used instead of remaining", async () => {
    renderCard(provider(null, 35), { showAsUsed: true });

    expect(await screen.findByText("35% used")).toBeInTheDocument();
    expect(screen.queryByText("65% left")).not.toBeInTheDocument();

    const fill = document.querySelector(".menu-metric__progress-fill");
    expect(fill?.getAttribute("width")).toBe("35%");
  });

  it("shows an additional balance description below the meter and reset time", async () => {
    const snapshot = provider(null, 25, {
      resetDescription: "750 / 1000 credits left",
    });
    snapshot.primaryLabel = "Credits";
    snapshot.primary.descriptionIsDetail = true;
    snapshot.selectedMetric.descriptionIsDetail = true;
    const resetsAt = new Date(Date.now() + 60 * 60 * 1000).toISOString();
    snapshot.primary.resetsAt = resetsAt;
    snapshot.selectedMetric.resetsAt = resetsAt;

    renderCard(snapshot);

    const description = await screen.findByText("750 / 1000 credits left");
    expect(description).toHaveClass("menu-metric__detail");
    expect(screen.getByText("Resets in 1h")).toHaveClass("menu-metric__reset");
  });

  it("does not repeat a reset-phrase description as an extra detail", async () => {
    const snapshot = provider(null, 20, { resetDescription: "Resets in 3h" });
    const resetsAt = new Date(Date.now() + 5 * 60 * 60 * 1000).toISOString();
    snapshot.primary.resetsAt = resetsAt;
    snapshot.selectedMetric.resetsAt = resetsAt;

    renderCard(snapshot);

    expect(await screen.findByText("Resets in 5h")).toHaveClass("menu-metric__reset");
    expect(document.querySelectorAll(".menu-metric__reset")).toHaveLength(1);
    expect(document.querySelectorAll(".menu-metric__detail")).toHaveLength(0);
    expect(screen.queryByText("Resets in 3h")).not.toBeInTheDocument();
  });

  it("clamps over-quota usage to a full bar and 100% used", async () => {
    renderCard(provider(null, 115, { exhausted: true, resetDescription: "115% used" }), {
      showAsUsed: true,
    });

    expect(await screen.findByText("100% used")).toHaveClass("menu-metric__percent");
    expect(screen.queryByText("115% used")).not.toBeInTheDocument();
    const fill = document.querySelector(".menu-metric__progress-fill");
    expect(fill?.getAttribute("width")).toBe("100%");
  });

  it("replaces an exhausted percentage with a future reset countdown", async () => {
    const snapshot = provider(null, 100, { exhausted: true });
    snapshot.primary.resetsAt = new Date(Date.now() + 60 * 60 * 1000).toISOString();

    renderCard(snapshot, { showResetWhenExhausted: true });

    expect(await screen.findByText("Resets in 1h")).toHaveClass("menu-metric__reset");
    expect(document.querySelector(".menu-metric__percent")).toBeNull();
    expect(screen.queryByText("0% left")).not.toBeInTheDocument();
  });

  it("keeps an exhausted percentage without a concrete future reset", async () => {
    renderCard(provider(null, 100, { exhausted: true, resetDescription: "in 2h" }), {
      showResetWhenExhausted: true,
    });

    expect(await screen.findByText("0% left")).toBeInTheDocument();
  });

  it("renders additional Copilot budget windows", async () => {
    const snapshot = provider(null, 20);
    snapshot.providerId = "copilot";
    snapshot.displayName = "GitHub Copilot";
    snapshot.extraRateWindows = [
      {
        id: "additional_budget",
        title: "Additional Budget",
        window: rateWindow(42),
      },
    ];

    renderCard(snapshot);

    expect(await screen.findByText("Additional Budget")).toBeInTheDocument();
    expect(screen.getByText("58% left")).toBeInTheDocument();
  });

  it("names Notion's billing lane Monthly while other monthly lanes keep 30-Day", async () => {
    // Upstream NotionProviderDescriptor.swift:32 (`weeklyLabel: "Monthly"`).
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ ProviderLabelMonthly: "Monthly", ProviderMonthly: "30-Day" }),
    );
    const notion = provider(null, 20);
    notion.providerId = "notion";
    notion.displayName = "Notion";
    notion.secondary = rateWindow(25, { windowMinutes: 43_200 });
    notion.secondaryLabel = "Monthly";
    notion.secondaryLabelKey = "ProviderLabelMonthly";
    const { unmount } = renderCard(notion);
    expect(await metricTitle("Monthly")).toBeInTheDocument();
    unmount();

    const other = provider(null, 20);
    other.providerId = "augment";
    other.displayName = "Augment";
    other.secondary = rateWindow(25, { windowMinutes: 43_200 });
    other.secondaryLabel = "Monthly";
    renderCard(other);
    expect(await metricTitle("30-Day")).toBeInTheDocument();
  });

  it("shows every quota row in compact Overview (upstream 0.62.0 #2616)", async () => {
    const snapshot = provider(null, 20, { resetDescription: "Resets in 2h" });
    snapshot.secondary = rateWindow(42, { windowMinutes: 7 * 24 * 60 });
    snapshot.secondaryLabel = "Weekly";
    snapshot.tertiary = rateWindow(63, { windowMinutes: 30 * 24 * 60 });
    snapshot.tertiaryLabel = "Monthly";

    renderCard(snapshot, { compactOverview: true });

    expect(await screen.findByText("ProviderSessionLabel")).toBeInTheDocument();
    expect(screen.getByText("ProviderWeeklyLabel")).toBeInTheDocument();
    expect(screen.getByText("ProviderMonthly")).toBeInTheDocument();
    expect(document.querySelectorAll(".menu-metric")).toHaveLength(3);
  });

  it("shows both Agent Plan lanes next to the informational placeholder in compact Overview", async () => {
    const snapshot = provider(null, 0);
    snapshot.primary = { ...rateWindow(0), isInformational: true, resetDescription: "No active 5h session" };
    snapshot.extraRateWindows = [
      { id: "doubao-agent-session", title: "5-hour", window: rateWindow(42, { windowMinutes: 300 }) },
      { id: "doubao-agent-weekly", title: "Weekly", window: rateWindow(67, { windowMinutes: 10080 }) },
    ];

    renderCard(snapshot, { compactOverview: true });

    // Compact Overview shows every quota row (upstream 0.62.0 #2616), so the
    // placeholder no longer competes with the measured lanes for a row.
    expect(await screen.findByText("58% left")).toBeInTheDocument();
    expect(screen.getByText("33% left")).toBeInTheDocument();
    expect(screen.getByText("No active 5h session")).toBeInTheDocument();
    expect(document.querySelectorAll(".menu-metric")).toHaveLength(3);
  });

  it("localizes Claude scoped weekly extra-window labels", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({ ClaudeScopedWeeklyLabel: "{} weekly", ProviderLabelModelOnly: "{} only" }));
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "claude-weekly-scoped-fable",
        title: "Fable only",
        window: rateWindow(42, { windowMinutes: 7 * 24 * 60 }),
      },
      {
        id: "custom",
        title: "Custom only",
        window: rateWindow(10),
      },
    ];

    renderCard(snapshot);

    expect(await screen.findByText("Fable weekly")).toBeInTheDocument();
    expect(screen.getByText("Custom only")).toBeInTheDocument();
    expect(screen.queryByText("Fable only")).not.toBeInTheDocument();

    const otherProvider = provider(null, 20);
    otherProvider.providerId = "synthetic";
    otherProvider.extraRateWindows = [{ ...snapshot.extraRateWindows[0], id: "synthetic-weekly" }];
    renderCard(otherProvider);
    expect(await screen.findByText("Fable only")).toBeInTheDocument();
  });

  it.each([false, true])(
    "shows detail-backed amounts as their own line, never as reset text (resetsAt: %s)",
    async (hasReset) => {
      const resetsAt = hasReset
        ? new Date(Date.now() + 3 * 60 * 60 * 1000 + 30_000).toISOString()
        : null;
      const snapshot = provider(null, 75, {
        resetDescription: "19.17 EUR / 25.50 EUR · 6.33 EUR remaining",
        resetsAt,
      });
      snapshot.providerId = "mistral";
      snapshot.primary.descriptionIsDetail = true;
      snapshot.selectedMetric.descriptionIsDetail = true;
      snapshot.extraRateWindows = [
        {
          id: "mistral-monthly-plan",
          title: "Monthly Plan",
          window: {
            ...rateWindow(13, {
              resetDescription: "34.07 EUR / 255.00 EUR · 220.93 EUR remaining",
              resetsAt,
            }),
            descriptionIsDetail: true,
          },
        },
      ];

      renderCard(snapshot);

      const primaryDetail = await screen.findByText(
        "19.17 EUR / 25.50 EUR · 6.33 EUR remaining",
      );
      const planDetail = screen.getByText("34.07 EUR / 255.00 EUR · 220.93 EUR remaining");
      expect(primaryDetail).toHaveClass("menu-metric__detail");
      expect(planDetail).toHaveClass("menu-metric__detail");
      expect(screen.queryByText(/Resets .*EUR/)).not.toBeInTheDocument();
      expect(document.querySelectorAll(".menu-metric__reset")).toHaveLength(hasReset ? 2 : 0);
    },
  );

  it("renders informational metrics without quota percentages", async () => {
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "requests",
        title: "Requests",
        window: {
          ...rateWindow(0),
          isInformational: true,
          resetDescription: "7 requests",
        },
      },
    ];

    renderCard(snapshot);

    const title = await screen.findByText("Requests");
    expect(title.parentElement).not.toHaveTextContent("100% left");
    expect(title.parentElement?.querySelector(".menu-metric__progress")).toBeNull();
    expect(document.querySelectorAll(".menu-metric__progress")).toHaveLength(1);
    expect(screen.getByText("7 requests")).toBeInTheDocument();
  });

  it("shows reset credit count and next expiry without a percent bar", async () => {
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "reset-credits",
        title: "Reset credits",
        window: {
          ...rateWindow(0, {
            resetsAt: new Date(Date.now() + 6 * 24 * 60 * 60 * 1000 + 21 * 60 * 60 * 1000).toISOString(),
            resetDescription: "2 reset credits available",
          }),
          isInformational: true,
        },
      },
    ];

    renderCard(snapshot);

    const title = await screen.findByText("Reset credits");
    expect(screen.getByText("2 reset credits available")).toBeInTheDocument();
    expect(screen.getByText(/Next expires in/)).toBeInTheDocument();
    expect(title.parentElement?.querySelector(".menu-metric__progress")).toBeNull();
    expect(document.querySelectorAll(".menu-metric__progress")).toHaveLength(1);
  });

  it("keeps reset credit count in absolute reset-time mode", async () => {
    const snapshot = provider(null, 20);
    snapshot.extraRateWindows = [
      {
        id: "reset-credits",
        title: "Reset credits",
        window: {
          ...rateWindow(0, {
            resetsAt: new Date(Date.now() + 6 * 24 * 60 * 60 * 1000).toISOString(),
            resetDescription: "2 reset credits available",
          }),
          isInformational: true,
        },
      },
    ];

    render(
      <LocaleProvider>
        <MenuCard
          provider={snapshot}
          display={{
            hideEmail: false,
            resetTimeRelative: false,
          }}
        />
      </LocaleProvider>,
    );

    expect(await screen.findByText("2 reset credits available")).toBeInTheDocument();
    expect(screen.queryByText(/Next expires in/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Resets in/)).not.toBeInTheDocument();
  });

  it("renders Wayfinder telemetry without quota or identity rows", async () => {
    const snapshot = provider(null);
    snapshot.providerId = "wayfinder";
    snapshot.displayName = "Wayfinder";
    snapshot.accountEmail = "should-not-render@example.test";
    snapshot.planName = "should-not-render";
    snapshot.wayfinderUsage = {
      gatewayStatus: "ok",
      offline: false,
      dryRun: false,
      missingKeys: [],
      modelCount: 2,
      models: ["model-a", "model-b"],
      requests: 14,
      estimatedRequests: 0,
      tokens: 1028,
      realized: 0.004,
      baseline: 0.01,
      saved: 0.006,
      savedPercent: 60,
      periodDays: 30,
      unit: "usd",
      priced: true,
      routes: [],
    };

    renderCard(snapshot);

    expect(await screen.findByText("ok")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(screen.getByText("1K")).toBeInTheDocument();
    expect(screen.queryByText("should-not-render@example.test")).not.toBeInTheDocument();
    expect(screen.queryByText("should-not-render")).not.toBeInTheDocument();
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
  });

  it("renders Wayfinder details from the bridge-shaped gateway snapshot", async () => {
    // Same JSON the Rust bridge emits for the Mac-parity Wayfinder pack
    // (rust/src/providers/wayfinder.rs, pack_snapshot_serializes_camel_case_keys_for_the_bridge).
    const bridgeJson = {
      gatewayStatus: "ok",
      offline: false,
      dryRun: false,
      missingKeys: [],
      modelCount: 3,
      models: ["cheap-local", "premium-cloud", "embed-local"],
      requests: 1200,
      estimatedRequests: 0,
      tokens: 350000,
      realized: 4.2,
      baseline: 9.8,
      saved: 5.6,
      savedPercent: 57.1,
      periodDays: 30,
      unit: "USD",
      priced: true,
      routes: [
        { name: "cheap", requests: 900, tokens: 260000, realized: 1.6, baseline: 4.7, saved: 3.1 },
        { name: "premium", requests: 300, tokens: 90000, realized: 2.6, baseline: 5.1, saved: 2.5 },
      ],
    } satisfies WayfinderUsageSnapshot;
    const snapshot = provider(null);
    snapshot.providerId = "wayfinder";
    snapshot.displayName = "Wayfinder";
    snapshot.wayfinderUsage = bridgeJson;

    const { rerender } = renderCard(snapshot);

    expect(await screen.findByText("ok")).toBeInTheDocument();
    expect(screen.getByText("3")).toBeInTheDocument();
    expect(screen.getByText("Saved: 5.6000 USD (57.1%)")).toBeInTheDocument();
    expect(screen.queryByText(/Missing keys/)).not.toBeInTheDocument();

    snapshot.wayfinderUsage = {
      ...bridgeJson,
      gatewayStatus: "degraded",
      dryRun: true,
      missingKeys: ["RIG_CLOUD_KEY"],
    };
    rerender(
      <LocaleProvider>
        <MenuCard provider={{ ...snapshot }} display={{ hideEmail: false, resetTimeRelative: true }} />
      </LocaleProvider>,
    );

    expect(await screen.findByText("degraded")).toBeInTheDocument();
    expect(screen.getByText("Dry run")).toBeInTheDocument();
    expect(screen.getByText("Missing keys: RIG_CLOUD_KEY")).toBeInTheDocument();
  });

  it("uses explicit hourly quota labels in Simplified Chinese", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({}, "chinese"));
    const snapshot = provider(null, 31);
    snapshot.primary = rateWindow(31, { windowMinutes: 3 * 60 });
    snapshot.selectedMetric = snapshot.primary;

    renderCard(snapshot);

    expect(await screen.findByText("3 小时")).toBeInTheDocument();
    expect(screen.queryByText("Session")).not.toBeInTheDocument();
  });

  it("maps a Simplified Chinese session-labelled weekly window to Weekly", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ ProviderWeeklyLabel: "本周" }, "chinese"),
    );
    const snapshot = provider(null, 31);
    snapshot.primary = rateWindow(31, { windowMinutes: 7 * 24 * 60 });
    snapshot.selectedMetric = snapshot.primary;

    renderCard(snapshot);

    expect(await screen.findByText("本周")).toBeInTheDocument();
  });

  it("notifies the tray panel after async local usage data loads", async () => {
    const onLayoutChange = vi.fn();

    renderCard(provider(null), { onLayoutChange });

    await waitFor(() => {
      expect(onLayoutChange).toHaveBeenCalled();
    });
  });

  it("cancels the pending layout frame when it unmounts", async () => {
    const onLayoutChange = vi.fn();
    const frames = new Map<number, FrameRequestCallback>();
    let nextFrame = 0;
    const raf = vi
      .spyOn(window, "requestAnimationFrame")
      .mockImplementation((callback) => {
        frames.set(++nextFrame, callback);
        return nextFrame;
      });
    const cancel = vi
      .spyOn(window, "cancelAnimationFrame")
      .mockImplementation((id) => {
        frames.delete(id);
      });
    try {
      const { unmount } = renderCard(provider(null), { onLayoutChange });
      await waitFor(() => expect(tauriMocks.getProviderChartData).toHaveBeenCalled());
      // Let the resolved chart data land so its layout frame is queued.
      await act(async () => {
        await Promise.resolve();
      });
      await waitFor(() => expect(frames.size).toBeGreaterThan(0));

      unmount();
      for (const callback of frames.values()) callback(0);

      expect(onLayoutChange).not.toHaveBeenCalled();
    } finally {
      raf.mockRestore();
      cancel.mockRestore();
    }
  });

  it("leaves the provider-level exhaustion ETA to Settings > Providers", async () => {
    const snapshot = provider(null, 40);
    snapshot.pace = {
      stage: "far_ahead",
      deltaPercent: 20,
      expectedUsedPercent: 20,
      actualUsedPercent: 40,
      etaSeconds: 90 * 60,
      willLastToReset: false,
    };

    const { container } = renderCard(snapshot);

    await metricTitle("ProviderSessionLabel");
    expect(container.querySelector(".menu-card__pace")).toBeNull();
    expect(screen.queryByText(/Runs out in 2h/)).not.toBeInTheDocument();
  });

  it("hides pace, budgets, and forecast text when Show pace is off", async () => {
    const resetAt = new Date(Date.now() + 6 * 24 * 60 * 60 * 1000);
    const snapshot = provider(null, 31);
    snapshot.pace = {
      stage: "far_ahead",
      deltaPercent: 20,
      expectedUsedPercent: 20,
      actualUsedPercent: 40,
      etaSeconds: 90 * 60,
      willLastToReset: false,
    };
    snapshot.primary = {
      ...rateWindow(31, {
        windowMinutes: 7 * 24 * 60,
        resetsAt: resetAt.toISOString(),
      }),
      pace: {
        stage: "far_ahead",
        deltaPercent: 20,
        expectedUsedPercent: 11,
        actualUsedPercent: 31,
        etaSeconds: 90 * 60,
        willLastToReset: false,
      },
    };

    const { container } = renderCard(snapshot, { showPace: false });

    expect(await screen.findByText("69% left")).toBeInTheDocument();
    expect(container.querySelector(".menu-metric__meta")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__progress-pace")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-card__pace")).not.toBeInTheDocument();
    expect(screen.queryByText("On-pace budget")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__forecast")).not.toBeInTheDocument();
  });

  it("hides derived pace advice for local OpenCode Go estimates", async () => {
    const resetAt = new Date(Date.now() + 3 * 24 * 60 * 60 * 1000);
    const snapshot = provider(null, 12);
    snapshot.providerId = "opencodego";
    snapshot.displayName = "OpenCode Go";
    snapshot.sourceLabel = "local estimate";
    snapshot.primary = rateWindow(12, {
      windowMinutes: 5 * 60,
      resetsAt: new Date(Date.now() + 2 * 60 * 60 * 1000).toISOString(),
    });
    snapshot.secondary = {
      ...rateWindow(23, {
        windowMinutes: 7 * 24 * 60,
        resetsAt: resetAt.toISOString(),
      }),
      pace: {
        stage: "far_behind",
        deltaPercent: -34,
        expectedUsedPercent: 57,
        actualUsedPercent: 23,
        etaSeconds: null,
        willLastToReset: true,
      },
    };
    snapshot.pace = {
      stage: "far_ahead",
      deltaPercent: 20,
      expectedUsedPercent: 20,
      actualUsedPercent: 40,
      etaSeconds: 90 * 60,
      willLastToReset: false,
    };

    const { container } = renderCard(snapshot);

    expect(await screen.findByText("88% left")).toBeInTheDocument();
    expect(screen.getByText("77% left")).toBeInTheDocument();
    expect(container.querySelector(".menu-card__pace")).not.toBeInTheDocument();
    expect(screen.queryByText("On-pace budget")).not.toBeInTheDocument();
    expect(screen.queryByText(/in reserve/)).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__meta")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__progress-pace")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__forecast")).not.toBeInTheDocument();
  });

  function kimiBlockedByMonthlyPool(blockResetsAt: string | null, poolResetsAt: string) {
    const hour = 60 * 60 * 1000;
    const shortReset = new Date(Date.now() + 3 * hour).toISOString();
    const block = { resetsAt: blockResetsAt };
    const snapshot = provider(null);
    snapshot.providerId = "kimi";
    snapshot.displayName = "Kimi";
    snapshot.primaryLabel = "Code 7-day";
    // Raw provider percentages stay untouched (0% used): the block alone
    // decides the presentation, as in upstream `blockingQuotaMetrics`.
    snapshot.primary = {
      ...rateWindow(0, {
        windowMinutes: 7 * 24 * 60,
        resetsAt: shortReset,
      }),
      pace: {
        stage: "far_behind",
        deltaPercent: -30,
        expectedUsedPercent: 30,
        actualUsedPercent: 0,
        etaSeconds: null,
        willLastToReset: true,
      },
      monthlyLimitBlock: block,
    };
    snapshot.secondaryLabel = "Code 5-hour";
    snapshot.secondary = {
      ...rateWindow(0, { windowMinutes: 5 * 60, resetsAt: shortReset }),
      monthlyLimitBlock: block,
    };
    snapshot.sessionEquivalentForecast = {
      estimatedWindowsToExhaustWeekly: 4,
      windowsUntilReset: 6,
      availableWindowsUntilReset: 6,
      sampleCount: 3,
      weeklyResetsAt: shortReset,
      weeklyUsedPercent: 0,
    };
    snapshot.extraRateWindows = [
      {
        id: "kimi-monthly",
        title: "Total usage",
        window: rateWindow(100, {
          windowMinutes: 30 * 24 * 60,
          exhausted: true,
          resetsAt: poolResetsAt,
        }),
      },
    ];
    snapshot.pace = {
      stage: "far_behind",
      deltaPercent: -40,
      expectedUsedPercent: 40,
      actualUsedPercent: 0,
      etaSeconds: null,
      willLastToReset: true,
      monthlyLimitBlock: block,
    };
    return snapshot;
  }

  it("shows only the title and status for windows blocked by an exhausted monthly pool", async () => {
    const poolReset = new Date(Date.now() + 20 * 24 * 60 * 60 * 1000).toISOString();
    const { container } = renderCard(kimiBlockedByMonthlyPool(poolReset, poolReset), {
      showAsUsed: true,
    });

    expect(await screen.findAllByText("Blocked by monthly limit")).toHaveLength(2);
    const blockedRows = container.querySelectorAll(".menu-metric--blocked");
    expect(blockedRows).toHaveLength(2);
    expect(blockedRows[0]).toHaveTextContent(/^Code 7-dayBlocked by monthly limit$/);
    expect(blockedRows[1]).toHaveTextContent(/^Code 5-hourBlocked by monthly limit$/);
    for (const row of blockedRows) {
      expect(row.querySelector(".menu-metric__progress")).toBeNull();
      expect(row.querySelector(".menu-metric__percent")).toBeNull();
      expect(row.querySelector(".menu-metric__reset")).toBeNull();
      expect(row.querySelector(".menu-metric__meta")).toBeNull();
    }
    // The pool row keeps its own bar, percent, reset and exhausted label.
    expect(container.querySelectorAll(".menu-metric__progress")).toHaveLength(1);
    expect(screen.getByText("Total usage")).toBeInTheDocument();
    expect(screen.getAllByText("100% used")).toHaveLength(1);
    expect(screen.queryByText("0% used")).not.toBeInTheDocument();
    expect(container.querySelectorAll(".menu-metric__reset")).toHaveLength(1);
    expect(container.querySelectorAll(".menu-metric__exhausted")).toHaveLength(1);
    // No pace, reserve, budget or session forecast for blocked windows.
    expect(screen.queryByText(/in reserve/)).not.toBeInTheDocument();
    expect(screen.queryByText("On-pace budget")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric__forecast")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-card__pace")).not.toBeInTheDocument();
  });

  it("keeps a block without a known pool reset", async () => {
    const poolReset = new Date(Date.now() + 20 * 24 * 60 * 60 * 1000).toISOString();
    const { container } = renderCard(kimiBlockedByMonthlyPool(null, poolReset), {
      showAsUsed: true,
    });

    expect(await screen.findAllByText("Blocked by monthly limit")).toHaveLength(2);
    expect(container.querySelector(".menu-card__pace")).not.toBeInTheDocument();
  });

  it("ignores a cached block whose monthly pool reset already passed", async () => {
    const past = new Date(Date.now() - 60 * 1000).toISOString();
    const { container } = renderCard(kimiBlockedByMonthlyPool(past, past), {
      showAsUsed: true,
    });

    expect(await screen.findAllByText("0% used")).toHaveLength(2);
    expect(screen.queryByText("Blocked by monthly limit")).not.toBeInTheDocument();
    expect(container.querySelector(".menu-metric--blocked")).toBeNull();
  });

  it("lifts the block when the monthly pool resets while the card stays open", async () => {
    vi.useFakeTimers();
    try {
      vi.setSystemTime(new Date("2026-06-01T00:00:00Z"));
      const poolReset = new Date("2026-06-01T00:10:00Z").toISOString();
      const { container } = renderCard(kimiBlockedByMonthlyPool(poolReset, poolReset), {
        showAsUsed: true,
      });
      await act(async () => {});
      expect(screen.getAllByText("Blocked by monthly limit")).toHaveLength(2);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(10 * 60 * 1000 - 1000);
      });
      expect(screen.getAllByText("Blocked by monthly limit")).toHaveLength(2);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(1000);
      });
      expect(screen.queryByText("Blocked by monthly limit")).not.toBeInTheDocument();
      expect(container.querySelector(".menu-metric--blocked")).toBeNull();
      expect(screen.getAllByText("0% used")).toHaveLength(2);
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps windows that are not blocked untouched", async () => {
    const snapshot = provider(null, 0, {});
    snapshot.providerId = "kimi";
    renderCard(snapshot, { showAsUsed: true });

    expect(await screen.findByText("0% used")).toBeInTheDocument();
    expect(screen.queryByText("Blocked by monthly limit")).not.toBeInTheDocument();
  });

  it("renders local token and cost totals after chart data loads", async () => {
    const { container } = renderCard(provider(null));

    expect(await screen.findByText("30d cost")).toBeInTheDocument();
    expect(container.querySelector(".menu-card--with-details")).toBeInTheDocument();
    expect(container.querySelector(".menu-card--header-only")).not.toBeInTheDocument();
    expect(screen.getAllByText("$1.23").length).toBeGreaterThan(0);
    expect(screen.getByText("30d tokens")).toBeInTheDocument();
    expect(screen.getByText("584K")).toBeInTheDocument();
    expect(container.querySelector(".card-dash")).toBeInTheDocument();
    // No collapsed "usage details" disclosure: the dashboard is inline.
    expect(container.querySelector("details")).toBeNull();
  });

  it("includes the matching daily token count in the local cost tooltip", async () => {
    const snapshot = provider(null);
    snapshot.providerId = "codex";
    snapshot.displayName = "Codex";
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "codex",
      costHistory: [{ date: "2026-05-24", value: 1.23 }],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: {
        todayCost: null,
        thirtyDayCost: 1.23,
        thirtyDayTokens: 14_200,
        latestTokens: null,
        topModel: "gpt-5",
        estimateNote: "Estimated from local logs",
        tokenCostUpdatedAtMs: 1234,
      },
      tokensHistory: [{ date: "2026-05-24", tokens: 14_200 }],
      tokensIncomplete: false,
    });

    const { container } = renderCard(snapshot);
    const bar = await waitFor(() => {
      const element = container.querySelector(".card-dash__bar");
      if (!element) throw new Error("local chart bar not rendered yet");
      return element;
    });

    expect(bar).toHaveAttribute("title", "2026-05-24: $1.23 · 14K tokens");
  });

  it("labels local totals with the selected History window instead of 30 days", async () => {
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "claude",
      costHistory: [{ date: "2026-05-24", value: 1.23 }],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: {
        todayCost: null,
        // The fixed 30-day fields must not leak into the period rows.
        thirtyDayCost: 1.23,
        thirtyDayTokens: 584_000,
        periodCost: 7.5,
        periodTokens: 2_000_000,
        reportingPeriod: "month-to-date",
        latestTokens: null,
        topModel: "glim-4.6",
        estimateNote: "Estimated from local logs",
        tokenCostUpdatedAtMs: 1234,
      },
    });

    renderCard(provider(null));

    expect(await screen.findByText("MTD cost")).toBeInTheDocument();
    expect(screen.getByText("MTD tokens")).toBeInTheDocument();
    expect(screen.getByText("$7.50")).toBeInTheDocument();
    expect(screen.getByText("2M")).toBeInTheDocument();
    expect(screen.queryByText("30d cost")).not.toBeInTheDocument();
    expect(screen.queryByText("584K")).not.toBeInTheDocument();
  });

  it("renders provider display details once and hides them in compact overview", async () => {
    const snapshot = provider(null);
    snapshot.displayDetails = [
      {
        id: "atlascloud-available",
        sectionTitle: null,
        title: "Available balance",
        value: "$95.50",
        secondaryValue: null,
        progress: null,
      },
    ];

    const detailed = renderCard(snapshot);
    const label = await screen.findByText("Available balance");
    expect(label).toHaveClass("card-kv__label");
    expect(screen.getAllByText("Available balance")).toHaveLength(1);
    expect(label.closest(".card-kv__row")?.querySelector(".card-kv__value")).toHaveTextContent(
      "$95.50",
    );
    detailed.unmount();

    renderCard(snapshot, { compactOverview: true });
    await metricTitle("ProviderSessionLabel");
    expect(screen.queryByText("Available balance")).not.toBeInTheDocument();
  });

  it("keeps Claude account switching out of the card", async () => {
    tauriMocks.claudeAccountsList.mockResolvedValue([
      { id: "a", email: "a@example.com", organization: "Personal", isActive: true, isSaved: true },
      { id: "b", email: "b@example.com", organization: "Work", isActive: false, isSaved: true },
    ]);
    const { container } = renderCard(provider(null));
    await metricTitle("ProviderSessionLabel");
    expect(screen.queryByText("ClaudeAccountsTitle")).not.toBeInTheDocument();
    expect(container.querySelector(".codex-menu-accounts")).toBeNull();
    expect(tauriMocks.claudeAccountsList).not.toHaveBeenCalled();
  });

  it("leaves on-pace budgets and the projection chart to Settings > Providers", async () => {
    const resetAt = new Date(Date.now() + 0.6 * 7 * 24 * 60 * 60 * 1000);
    const snapshot = provider(null, 20);
    snapshot.primary = rateWindow(20, {
      reservePercent: 20,
      reserveWillLastToReset: true,
      windowMinutes: 7 * 24 * 60,
      resetsAt: resetAt.toISOString(),
    });

    renderCard(snapshot);

    await metricTitle("ProviderSessionLabel");
    expect(screen.queryByText(/On-pace budget/)).not.toBeInTheDocument();
    expect(screen.queryByText("now 20%")).not.toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /PaceChartAriaLabel/i })).not.toBeInTheDocument();
  });

  it("does not show pace budgets for a five-hour session window", async () => {
    const resetAt = new Date(Date.now() + 4 * 60 * 60 * 1000);
    const snapshot = provider(null, 31);
    snapshot.primary = rateWindow(31, {
      windowMinutes: 5 * 60,
      resetsAt: resetAt.toISOString(),
    });

    renderCard(snapshot);

    expect(await screen.findByText("69% left")).toBeInTheDocument();
    expect(screen.queryByText("On-pace budget")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("img", { name: /PaceChartAriaLabel/i }),
    ).not.toBeInTheDocument();
  });

  it("puts the percent after the title and the reset at the end of the same line", async () => {
    const snapshot = provider(null, 37);
    snapshot.primary.resetsAt = new Date(Date.now() + 90 * 60 * 1000).toISOString();

    const { container } = renderCard(snapshot);

    expect(await screen.findByText("63% left")).toHaveClass("menu-metric__percent");
    const head = container.querySelector(".menu-metric__head");
    expect(Array.from(head?.children ?? [], (node) => node.textContent)).toEqual([
      "ProviderSessionLabel 63% left",
      "Resets in 1h 30m",
    ]);
  });

  it("shows a reserve as a green pace stripe and a pace line under the bar", async () => {
    const snapshot = provider(null, 20);
    snapshot.secondaryLabel = "Weekly";
    snapshot.secondary = {
      ...rateWindow(40, {
        windowMinutes: 7 * 24 * 60,
        resetsAt: new Date(Date.now() + 3 * 24 * 60 * 60 * 1000).toISOString(),
      }),
      pace: {
        stage: "behind",
        deltaPercent: -12,
        expectedUsedPercent: 52,
        actualUsedPercent: 40,
        etaSeconds: null,
        willLastToReset: true,
      },
    };

    const { container } = renderCard(snapshot);

    expect(await screen.findByText("12% in reserve · Lasts until reset")).toHaveClass(
      "menu-metric__meta",
    );
    const stripes = container.querySelectorAll(".menu-metric__progress-pace");
    expect(stripes).toHaveLength(1);
    expect(stripes[0].getAttribute("x")).toBe("48%");
    expect(stripes[0].getAttribute("data-deficit")).toBe("false");
  });

  it("shows a deficit as a red pace stripe and says when the window runs out", async () => {
    const snapshot = provider(null, 20);
    snapshot.secondaryLabel = "Weekly";
    snapshot.secondary = {
      ...rateWindow(40, {
        windowMinutes: 7 * 24 * 60,
        resetsAt: new Date(Date.now() + 4 * 24 * 60 * 60 * 1000).toISOString(),
      }),
      pace: {
        stage: "ahead",
        deltaPercent: 9,
        expectedUsedPercent: 31,
        actualUsedPercent: 40,
        etaSeconds: 2.5 * 60 * 60,
        willLastToReset: false,
      },
    };

    const { container } = renderCard(snapshot, { showAsUsed: true });

    expect(await screen.findByText("9% in deficit · Runs out in 2h 30m")).toHaveClass(
      "menu-metric__meta",
    );
    const stripe = container.querySelector(".menu-metric__progress-pace");
    expect(stripe?.getAttribute("x")).toBe("31%");
    expect(stripe?.getAttribute("data-deficit")).toBe("true");
  });

  it.each([
    ["global thresholds on a % left bar", false, {}, ["5%", "20%"]],
    ["global thresholds on a % used bar", true, {}, ["80%", "95%"]],
    ["a Custom session quota warning", false, { claude: { session: { high: 60 } } }, ["5%", "40%"]],
    ["a session quota warning set to Off", false, { claude: { session: { enabled: false } } }, []],
  ])("draws quota warning markers from %s", async (_case, showAsUsed, overrides, expected) => {
    const { container } = renderCard(provider(null, 30), {
      showAsUsed,
      usageThresholds: {
        highUsageThreshold: 80,
        criticalUsageThreshold: 95,
        providerQuotaWarnings: overrides,
      },
    });

    await screen.findByText(showAsUsed ? "30% used" : "70% left");
    expect(
      Array.from(container.querySelectorAll(".menu-metric__progress-warning"), (node) =>
        node.getAttribute("x"),
      ),
    ).toEqual(expected);
    expect(container.querySelector(".menu-metric__progress")?.getAttribute("aria-valuetext")).toBe(
      expected.length
        ? `${showAsUsed ? 30 : 70}%. Quota warnings: ${expected.join(", ")}`
        : `${showAsUsed ? 30 : 70}%`,
    );
  });

  it("draws work day ticks on the weekly bar only", async () => {
    const snapshot = provider(null, 20, {});
    snapshot.primary.windowMinutes = 5 * 60;
    snapshot.secondaryLabel = "Weekly";
    snapshot.secondary = rateWindow(40, { windowMinutes: 7 * 24 * 60 });

    const { container } = renderCard(snapshot, { weeklyProgressWorkDays: 5 });

    await screen.findByText("60% left");
    const bars = container.querySelectorAll(".menu-metric__progress");
    expect(bars).toHaveLength(2);
    expect(bars[0].querySelectorAll(".menu-metric__progress-workday")).toHaveLength(0);
    expect(
      Array.from(bars[1].querySelectorAll(".menu-metric__progress-workday"), (node) =>
        node.getAttribute("x"),
      ),
    ).toEqual(["20%", "40%", "60%", "80%"]);
  });


  it("shows capped spend as an extra usage bar with percent and balance", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        ExtraUsage: "Extra usage",
        UsedPercent: "{}% used",
        DetailCostBalance: "Balance",
        CostPeriodThisMonth: "This month",
      }),
    );
    const snapshot = provider(null, 20);
    snapshot.cost = {
      used: 12.5,
      limit: 100,
      remaining: 87.5,
      currencyCode: "USD",
      period: "This month",
      resetsAt: null,
      formattedUsed: "$12.50",
      formattedLimit: "$100.00",
      balance: 25.5,
      formattedBalance: "$25.50",
    };

    const { container } = renderCard(snapshot);

    expect(await screen.findByText("Extra usage")).toHaveClass("card-extra__title");
    expect(container.querySelector(".card-extra__spend")).toHaveTextContent(
      "This month: $12.50 / $100.00",
    );
    expect(screen.getByText("13% used")).toBeInTheDocument();
    expect(screen.getByRole("progressbar", { name: "Extra usage" })).toHaveAttribute(
      "aria-valuenow",
      "12.5",
    );
    expect(screen.getByText("Balance: $25.50")).toBeInTheDocument();
  });

  it("renders balance-only cost as one inline extra usage row", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ ExtraUsage: "Extra usage", DetailCostBalance: "Balance" }),
    );
    const snapshot = provider(null, 20);
    snapshot.cost = {
      used: 0,
      limit: null,
      remaining: null,
      currencyCode: "USD",
      period: "Extra usage",
      resetsAt: null,
      formattedUsed: "$0.00",
      formattedLimit: null,
      balance: 25.5,
      formattedBalance: "$25.50",
    };

    const { container } = renderCard(snapshot);

    expect(await screen.findByText("Extra usage")).toBeInTheDocument();
    expect(
      container.querySelector(".card-extra--inline .card-extra__inline-value"),
    ).toHaveTextContent("Balance: $25.50");
    expect(container.querySelector(".card-extra__track")).toBeNull();
  });

  it("drops a balance-only cost row when the detail sections already show that balance", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ ExtraUsage: "Extra usage", DetailCostBalance: "Balance" }),
    );
    const snapshot = provider(null, 20);
    snapshot.cost = {
      used: 17.25,
      limit: null,
      remaining: null,
      currencyCode: "USD",
      period: "Team credits",
      resetsAt: null,
      formattedUsed: "$17.25",
      formattedLimit: null,
      balance: 42.5,
      formattedBalance: null,
    };
    snapshot.displayDetails = [
      {
        id: "team-balance",
        sectionTitle: "Team credits",
        title: "Available balance",
        value: "$42.50",
        secondaryValue: null,
        progress: null,
      },
    ];

    const { container } = renderCard(snapshot);

    expect(await screen.findByText("$42.50")).toBeInTheDocument();
    expect(screen.getAllByText(/\$42\.50/)).toHaveLength(1);
    expect(screen.queryByText("Extra usage")).not.toBeInTheDocument();
    expect(container.querySelector(".card-extra")).toBeNull();
  });

  describe("Claude reset credits, Extra usage and plan dates (upstream #4484)", () => {
    function claude(hidden: string[] = []): ProviderUsageSnapshot {
      const snapshot = provider(null, 20);
      snapshot.sourceLabel = "web";
      snapshot.hiddenUsageItemIds = hidden;
      snapshot.inventory = [
        { id: "reset-credits", title: "Limit Reset Credits", availableCount: 2, nextExpiresAt: null },
      ];
      snapshot.cost = {
        used: 0,
        limit: null,
        remaining: null,
        currencyCode: "USD",
        period: "Extra usage",
        resetsAt: null,
        formattedUsed: "$0.00",
        formattedLimit: null,
        balance: 5,
        formattedBalance: "$5.00",
      };
      snapshot.subscription = {
        startsAt: null,
        expiresAt: null,
        renewsAt: "2026-12-01T00:00:00+00:00",
        renewsAtIsDateOnly: true,
      };
      return snapshot;
    }

    beforeEach(() => {
      tauriMocks.getLocaleStrings.mockResolvedValue(
        buildBundle({
          ExtraUsage: "Extra usage",
          DetailCostBalance: "Balance",
          InventoryAvailableCount: "{} available",
          IntlLocale: "en-US",
          PlanRenewsOn: "Renews: {}",
          PlanExpiresOn: "Plan expires: {}",
        }),
      );
    });

    it("shows the plan date under the quota rows, before saved resets and Extra usage", async () => {
      const { container } = renderCard(claude());
      const note = await screen.findByText("Renews: Dec 1, 2026");
      const content = container.querySelector(".menu-card__content");
      const order = [...(content?.children ?? [])].map((el) => el.className);
      expect(order.indexOf("menu-card__metrics")).toBeLessThan(order.indexOf(note.className));
      expect(order.indexOf(note.className)).toBeLessThan(order.indexOf("card-kv"));
      expect(screen.getByText("2 available")).toBeInTheDocument();
      expect(container.querySelector(".card-extra")).not.toBeNull();
    });

    it("hides each section independently", async () => {
      const { container, unmount } = renderCard(claude(["section:subscription"]));
      expect(await screen.findByText("2 available")).toBeInTheDocument();
      expect(screen.queryByText(/Renews:/)).not.toBeInTheDocument();
      expect(container.querySelector(".card-extra")).not.toBeNull();
      unmount();

      const resets = renderCard(claude(["section:claude-reset-credits"]));
      expect(await screen.findByText("Renews: Dec 1, 2026")).toBeInTheDocument();
      expect(screen.queryByText("2 available")).not.toBeInTheDocument();
      expect(resets.container.querySelector(".card-extra")).not.toBeNull();
      resets.unmount();

      const extra = renderCard(claude(["section:claude-extra-usage"]));
      expect(await screen.findByText("Renews: Dec 1, 2026")).toBeInTheDocument();
      expect(screen.getByText("2 available")).toBeInTheDocument();
      expect(extra.container.querySelector(".card-extra")).toBeNull();
    });

    it("never hides Admin API spend through the Extra usage choice", async () => {
      const snapshot = claude(["section:claude-extra-usage"]);
      snapshot.sourceLabel = "admin-api";
      const { container } = renderCard(snapshot);
      await screen.findByText("Renews: Dec 1, 2026");
      expect(container.querySelector(".card-extra")).not.toBeNull();
    });

    it("keeps the plan date off the compact Overview when quota bars are present", async () => {
      const { container } = renderCard(claude(), { compactOverview: true });
      await waitFor(() => expect(container.querySelector(".menu-metric__title")).not.toBeNull());
      expect(screen.queryByText(/Renews:/)).not.toBeInTheDocument();
    });
  });

  describe("LithosAI shows its balance twice, as on the Mac", () => {
    function lithos(balance: number | null, shown: string): ProviderUsageSnapshot {
      const snapshot = provider(null, 0, { resetDescription: `Balance: ${shown}` });
      snapshot.providerId = "lithosai";
      snapshot.displayName = "LithosAI";
      snapshot.primary = { ...snapshot.primary, isInformational: true, hiddenOnCard: true };
      snapshot.cost =
        balance === null
          ? null
          : {
              used: 0,
              limit: null,
              remaining: null,
              currencyCode: "USD",
              period: "Prepaid credits",
              resetsAt: null,
              formattedUsed: "$0.00",
              formattedLimit: null,
              balance,
              formattedBalance: shown,
              titleKey: "CreditsTitle",
            };
      snapshot.displayDetails = [
        { id: "balance", sectionTitle: "Billing", title: "Balance", value: shown, secondaryValue: null, progress: null },
        { id: "payment-card", sectionTitle: "Billing", title: "Payment card", value: "Added", secondaryValue: null, progress: null },
      ];
      return snapshot;
    }

    beforeEach(() => {
      tauriMocks.getLocaleStrings.mockResolvedValue(
        buildBundle({ CreditsTitle: "Credits", ExtraUsage: "Extra usage", DetailCostBalance: "Balance" }),
      );
    });

    // Upstream: the Billing "Balance" row (lithosai.ts:63), then the cost block
    // (lithosai.ts:72-73, StatusItemController+Menu.swift:1307-1336,1377-1391),
    // titled "Credits" and reading "Balance: $X" (MenuCardView+Costs.swift:511-518).
    it("in the Billing row, then in the Credits block", async () => {
      const { container } = renderCard(lithos(42.37, "$42.37"));

      expect(await screen.findByText("Credits")).toHaveClass("card-extra__title");
      const shown = screen.getAllByText(/42\.37/);
      expect(shown).toHaveLength(2);
      expect(shown[0]).toHaveTextContent(/^\$42\.37$/);
      expect(shown[0]).toHaveClass("card-kv__value");
      expect(shown[1]).toHaveTextContent(/^Balance: \$42\.37$/);
      expect(shown[1]).toHaveClass("card-extra__inline-value");
      expect(
        shown[0].compareDocumentPosition(shown[1]) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(screen.getByText("Payment card")).toBeInTheDocument();
      // The headline balance stays off the card (tray and float bar only).
      expect(container.querySelector(".menu-metric")).toBeNull();
      expect(screen.queryByText("Extra usage")).not.toBeInTheDocument();
    });

    // Upstream compact Overview rows draw the provider details when a card has no
    // metrics (MenuCardView+ModelHelpers.swift:378-380) and never the cost block.
    it("once on the compact Overview, in the Billing rows", async () => {
      const { container } = renderCard(lithos(42.37, "$42.37"), { compactOverview: true });

      expect(await screen.findByText("Payment card")).toBeInTheDocument();
      const shown = screen.getAllByText(/42\.37/);
      expect(shown).toHaveLength(1);
      expect(shown[0]).toHaveTextContent(/^\$42\.37$/);
      expect(shown[0]).toHaveClass("card-kv__value");
      expect(container.querySelector(".menu-metric")).toBeNull();
      expect(container.querySelector(".card-extra")).toBeNull();
      expect(container.querySelector(".menu-card--header-only")).toBeNull();
    });

    // Windows deviation: the Mac `.prepaidCredits` block ignores the summary style.
    it("once, in the Billing row, when the cost summary is hidden", async () => {
      const { container } = renderCard(lithos(42.37, "$42.37"), { costSummaryDisplayStyle: "hidden" });

      expect(await screen.findByText("$42.37")).toBeInTheDocument();
      expect(screen.getAllByText(/42\.37/)).toHaveLength(1);
      expect(container.querySelector(".menu-metric")).toBeNull();
      expect(container.querySelector(".card-extra")).toBeNull();
    });

    // Windows deviation: the shared cost block has no negative balance, so a debt has no block.
    it("once, in the Billing row, for a debt", async () => {
      const { container } = renderCard(lithos(null, "-$1.00"));

      expect(await screen.findByText("-$1.00")).toBeInTheDocument();
      expect(screen.getAllByText(/1\.00/)).toHaveLength(1);
      expect(container.querySelector(".menu-metric")).toBeNull();
      expect(container.querySelector(".card-extra")).toBeNull();
    });
  });

  it("titles ClawRouter's capped cost block Monthly budget", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({ CostTitleClawRouterMonthlyBudget: "Monthly budget", ExtraUsage: "Extra usage" }),
    );
    const snapshot = provider(null, 37);
    snapshot.providerId = "clawrouter";
    snapshot.displayName = "ClawRouter";
    snapshot.cost = {
      used: 18.42,
      limit: 50,
      remaining: 31.58,
      currencyCode: "USD",
      period: "This month",
      resetsAt: null,
      formattedUsed: "$18.42",
      formattedLimit: "$50.00",
      titleKey: "CostTitleClawRouterMonthlyBudget",
    };

    renderCard(snapshot);

    expect(await screen.findByText("Monthly budget")).toHaveClass("card-extra__title");
    expect(screen.queryByText("Extra usage")).not.toBeInTheDocument();
  });

  it("localizes the relative updated-at time in Japanese without duplicated prefix", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        UpdatedJustNow: "たった今",
        UpdatedMinutesAgo: "{}分前",
        UpdatedHoursAgo: "{}時間前",
        UpdatedDaysAgo: "{}日前",
      }),
    );

    const snapshot = provider(null, 20);
    snapshot.updatedAt = new Date(Date.now() - 3 * 60 * 1000).toISOString();
    renderCard(snapshot);

    expect(await screen.findByText("3分前")).toBeInTheDocument();
  });
});

// Upstream keeps the card a plain column; the session forecast moved to
// Settings > Providers > Usage details.
const stylesSource = loadStyles();

describe("MenuCard column layout", () => {
  it("keeps the session forecast out of the card and lays the content out as a column", async () => {
    const snapshot = provider(null, 20);
    snapshot.secondary = rateWindow(35, { windowMinutes: 7 * 24 * 60 });
    snapshot.secondaryLabel = "Weekly";
    snapshot.sessionEquivalentForecast = {
      estimatedWindowsToExhaustWeekly: 123,
      windowsUntilReset: 4,
      availableWindowsUntilReset: 4,
      sampleCount: 8,
      weeklyResetsAt: "2026-06-01T00:00:00Z",
      weeklyUsedPercent: 35,
    };

    const { container } = renderCard(snapshot);
    expect(await screen.findByText("ProviderWeeklyLabel")).toBeInTheDocument();
    expect(screen.queryByText(/session quotas left/)).not.toBeInTheDocument();
    expect(container.querySelector(".menu-card__content")).toBeInTheDocument();

    const card = ruleBlock(stylesSource, ".menu-card");
    expect(card).toContain("align-items: stretch");
    const content = ruleBlock(stylesSource, ".menu-card__content");
    expect(content).toContain("display: flex");
    expect(content).toContain("flex-direction: column");
  });
});
