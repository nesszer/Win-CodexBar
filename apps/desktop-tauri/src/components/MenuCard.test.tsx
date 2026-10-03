import { readFileSync } from "node:fs";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
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
import type { ProviderUsageSnapshot } from "../types/bridge";
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

function renderCard(
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

describe("MenuCard", () => {
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

  it("keeps Codex sign-in discoverable beside an authentication error with no accounts", async () => {
    tauriMocks.getCodexAccountsState.mockResolvedValueOnce({ accounts: [], accountOrdinals: {}, snapshots: {} });
    const codex = { ...provider("Authentication required"), providerId: "codex", displayName: "Codex", errorState: "needsAuthentication" as const };
    render(<LocaleProvider><MenuCard provider={codex} display={{ hideEmail: false, resetTimeRelative: true, showResetWhenExhausted: true }} /></LocaleProvider>);
    expect(await screen.findByText("Authentication required")).toBeDefined();
    const add = await screen.findByRole("button", { name: "CodexAccountsSignInButton" });
    await waitFor(() => expect(add).toBeEnabled());
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

    expect(await screen.findByText("API spend")).toBeInTheDocument();
    expect(document.querySelector(".menu-card__cost-line")).toHaveTextContent("$12.34");
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

  it("shows DeepSeek peak/off-peak pricing status", async () => {
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

    expect(await screen.findByText("DeepSeek pricing: Off-peak hours")).toBeInTheDocument();
    expect(screen.getByText("Current local time: 2026-08-17 05:00:00 UTC")).toBeInTheDocument();
    expect(screen.getByText("Next transition: 2026-08-17 06:00:00 UTC")).toBeInTheDocument();
  });

  it("can render metric bars as used instead of remaining", async () => {
    renderCard(provider(null, 35), { showAsUsed: true });

    expect(await screen.findByText("35% used")).toBeInTheDocument();
    expect(screen.queryByText("65% left")).not.toBeInTheDocument();

    const fill = document.querySelector<HTMLElement>(".menu-metric__bar-fill");
    expect(fill?.style.width).toBe("35%");
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
    expect(screen.getByText(/Resets in/)).toBeInTheDocument();
  });

  it("does not repeat a reset-phrase description as an extra detail", async () => {
    const snapshot = provider(null, 20, { resetDescription: "Resets in 3h" });
    const resetsAt = new Date(Date.now() + 5 * 60 * 60 * 1000).toISOString();
    snapshot.primary.resetsAt = resetsAt;
    snapshot.selectedMetric.resetsAt = resetsAt;

    renderCard(snapshot);

    await screen.findByText(/Resets in/);
    expect(document.querySelectorAll(".menu-metric__reset")).toHaveLength(1);
    expect(document.querySelectorAll(".menu-metric__detail")).toHaveLength(0);
    expect(screen.queryByText("Resets in 3h")).not.toBeInTheDocument();
  });

  it("displays over-quota usage without overflowing the bar", async () => {
    renderCard(provider(null, 115, { exhausted: true, resetDescription: "115% used" }), {
      showAsUsed: true,
    });

    expect(await screen.findAllByText("115% used")).not.toHaveLength(0);
    const fill = document.querySelector<HTMLElement>(".menu-metric__bar-fill");
    expect(fill?.style.width).toBe("100%");
  });

  it("replaces an exhausted percentage with a future reset countdown", async () => {
    const snapshot = provider(null, 100, { exhausted: true });
    snapshot.primary.resetsAt = new Date(Date.now() + 60 * 60 * 1000).toISOString();

    renderCard(snapshot, { showResetWhenExhausted: true });

    expect(await screen.findByText(/Resets in \d+m/)).toBeInTheDocument();
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

  it("shows every quota row in compact Overview (upstream 0.62.0 #2616)", async () => {
    const snapshot = provider(null, 20, { resetDescription: "Resets in 2h" });
    snapshot.secondary = rateWindow(42, { windowMinutes: 7 * 24 * 60 });
    snapshot.secondaryLabel = "Weekly";
    snapshot.tertiary = rateWindow(63, { windowMinutes: 30 * 24 * 60 });
    snapshot.tertiaryLabel = "Monthly";

    renderCard(snapshot, { compactOverview: true });

    expect(await screen.findByText("Session")).toBeInTheDocument();
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
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({ ClaudeScopedWeeklyLabel: "{} weekly" }));
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
    expect(title.parentElement?.querySelector(".menu-metric__bar")).toBeNull();
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
    expect(title.parentElement?.querySelector(".menu-metric__bar")).toBeNull();
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

  it("shows the formatted predicted exhaustion time", async () => {
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

    await waitFor(() => {
      expect(container.querySelector(".menu-card__pace-eta")).toHaveTextContent(
        "⚠ Runs out in 2h",
      );
    });
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
    snapshot.primary = rateWindow(31, {
      windowMinutes: 7 * 24 * 60,
      resetsAt: resetAt.toISOString(),
    });

    const { container } = renderCard(snapshot, { showPace: false });

    expect(await screen.findByText("69% left")).toBeInTheDocument();
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
    snapshot.secondary = rateWindow(23, {
      windowMinutes: 7 * 24 * 60,
      resetsAt: resetAt.toISOString(),
      reservePercent: 34,
      reserveWillLastToReset: true,
    });
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
        reservePercent: 30,
        reserveWillLastToReset: true,
      }),
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
      expect(row.querySelector(".menu-metric__bar")).toBeNull();
      expect(row.querySelector(".menu-metric__pct")).toBeNull();
      expect(row.querySelector(".menu-metric__reset")).toBeNull();
    }
    // The pool row keeps its own bar, percent, reset and exhausted label.
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
    expect(container.querySelector(".menu-card__pace")).toBeInTheDocument();
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
    expect(screen.getByText("Estimated from local logs")).toBeInTheDocument();
    const details = container.querySelector<HTMLDetailsElement>(".menu-card__more")!;
    expect(details.open).toBe(false);
    fireEvent.click(details.querySelector("summary")!);
    expect(details.open).toBe(true);
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
      const element = container.querySelector(".menu-card__local-chart span");
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
    expect(await screen.findByText("Available balance: $95.50")).toBeInTheDocument();
    expect(screen.getAllByText("Available balance: $95.50")).toHaveLength(1);
    detailed.unmount();

    renderCard(snapshot, { compactOverview: true });
    expect(screen.queryByText("Available balance: $95.50")).not.toBeInTheDocument();
  });

  it("places Claude accounts above metrics and the collapsed usage details", async () => {
    tauriMocks.claudeAccountsList.mockResolvedValue([
      { id: "a", email: "a@example.com", organization: "Personal", isActive: true, isSaved: true },
      { id: "b", email: "b@example.com", organization: "Work", isActive: false, isSaved: true },
    ]);
    const { container } = renderCard(provider(null));
    await screen.findByText("ClaudeAccountsTitle");
    const accounts = container.querySelector(".codex-menu-accounts")!;
    const metrics = container.querySelector(".menu-card__metrics")!;
    expect(accounts.compareDocumentPosition(metrics) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(container.querySelector<HTMLDetailsElement>(".menu-card__more")?.open).toBe(false);
  });

  it("shows on-pace budgets and expands projection details", async () => {
    const onLayoutChange = vi.fn();
    const resetAt = new Date(
      Date.now() + 0.6 * 7 * 24 * 60 * 60 * 1000,
    );
    const snapshot = provider(null, 20);
    snapshot.primary = rateWindow(20, {
      reservePercent: 20,
      reserveWillLastToReset: true,
      windowMinutes: 7 * 24 * 60,
      resetsAt: resetAt.toISOString(),
    });

    renderCard(snapshot, { onLayoutChange });

    const toggle = await screen.findByRole("button", { name: /On-pace budget/ });
    expect(screen.queryByText("now 20%")).not.toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /PaceChartAriaLabel/i })).not.toBeInTheDocument();

    fireEvent.click(toggle);

    expect(screen.getByText("now 20%")).toBeInTheDocument();
    expect(screen.getByText("1h 21%")).toBeInTheDocument();
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("img", { name: /PaceChartAriaLabel/i })).toBeInTheDocument();
    await waitFor(() => {
      expect(onLayoutChange).toHaveBeenCalled();
    });
  });

  it("shows on-pace budgets when timing exists without reserve metadata", async () => {
    const resetAt = new Date(Date.now() + 6 * 24 * 60 * 60 * 1000);
    const snapshot = provider(null, 31);
    snapshot.primary = rateWindow(31, {
      windowMinutes: 7 * 24 * 60,
      resetsAt: resetAt.toISOString(),
    });

    renderCard(snapshot);

    const toggle = await screen.findByRole("button", { name: /On-pace budget/ });
    expect(screen.queryByText("now 0%")).not.toBeInTheDocument();
    fireEvent.click(toggle);
    expect(screen.getByText("now 0%")).toBeInTheDocument();
    expect(screen.queryByText(/in reserve/)).not.toBeInTheDocument();
    expect(screen.queryByText("Lasts until reset")).not.toBeInTheDocument();
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

  it("keeps the reserve row when timing data is incomplete", async () => {
    const snapshot = provider(null, 20);
    snapshot.primary = rateWindow(20, {
        reservePercent: 12,
        reserveWillLastToReset: true,
      });

    renderCard(snapshot);

    expect(await screen.findByText("12% in reserve")).toBeInTheDocument();
    expect(screen.queryByText("On-pace budget")).not.toBeInTheDocument();
  });


  it("shows spend used/limit plus balance secondary line", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        DetailCostTitle: "Cost",
        DetailCostUsed: "Used",
        DetailCostBalance: "Balance",
        DetailCostRemaining: "Remaining",
      }),
    );
    const snapshot = provider(null, 20);
    snapshot.cost = {
      used: 12.5,
      limit: 100,
      remaining: 87.5,
      currencyCode: "USD",
      period: "Extra usage",
      resetsAt: null,
      formattedUsed: "$12.50",
      formattedLimit: "$100.00",
      balance: 25.5,
      formattedBalance: "$25.50",
    };

    renderCard(snapshot);

    expect(await screen.findByText(/Cost — Extra usage/)).toBeInTheDocument();
    expect(screen.getByText(/Used:\s*\$12\.50\s*\/\s*\$100\.00/)).toBeInTheDocument();
    expect(screen.getByText(/Balance:\s*\$25\.50/)).toBeInTheDocument();
  });

  it("renders balance-only cost as credits-style value", async () => {
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        CreditsLabel: "Credits",
        DetailCostTitle: "Cost",
        DetailCostUsed: "Used",
      }),
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

    renderCard(snapshot);

    expect(await screen.findByText("Extra usage")).toBeInTheDocument();
    expect(screen.getByText("$25.50")).toBeInTheDocument();
    expect(screen.queryByText(/Used:/)).not.toBeInTheDocument();
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

// The SwiftUI fix this regression came from protecting a cached native
// measurement. The Windows card has no cached measurement layer: its live
// forecast is a normal flex row whose width is recomputed by WebView2.
if (!import.meta.dirname) {
  throw new Error("import.meta.dirname unavailable to vitest runner");
}
const stylesSource = readFileSync(import.meta.dirname + "/../styles.css", "utf8");

function ruleBlock(source: string, selector: string): string {
  const escaped = selector.replace(/[^\w-]/g, "\\$&");
  const match = source.match(
    new RegExp("(?:^|\\r?\\n)" + escaped + "\\s*\\{([^}]*)\\}"),
  );
  expect(match).not.toBeNull();
  return match![1];
}

describe("MenuCard live forecast layout", () => {
  it("renders the changing forecast in the current full-width flex row", async () => {
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
    const forecast = await screen.findByText("Estimated: 123 session quotas left");
    const row = forecast.closest(".menu-metric__forecast");

    expect(row).toBeInTheDocument();
    expect(row).toHaveClass("menu-metric__row");
    expect(row?.parentElement).toHaveClass("menu-metric");
    expect(container.querySelector(".menu-card__content")).toBeInTheDocument();

    const card = ruleBlock(stylesSource, ".menu-card");
    expect(card).toContain("align-items: stretch");
    const content = ruleBlock(stylesSource, ".menu-card__content");
    expect(content).toContain("display: flex");
    expect(content).toContain("flex-direction: column");
    const metricRow = ruleBlock(stylesSource, ".menu-metric__row");
    expect(metricRow).toContain("min-width: 0");
    const forecastLabel = ruleBlock(
      stylesSource,
      ".menu-metric__forecast .menu-metric__pct",
    );
    expect(forecastLabel).toContain("flex: 1 1 auto");
    expect(forecastLabel).toContain("min-width: 0");
    expect(forecastLabel).toContain("overflow: hidden");
    expect(forecastLabel).toContain("text-overflow: ellipsis");
    expect(forecastLabel).toContain("white-space: nowrap");
  });
});
