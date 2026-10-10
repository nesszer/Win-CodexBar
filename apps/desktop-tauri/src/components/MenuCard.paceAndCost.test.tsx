import { act, fireEvent, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { buildBundle } from "../test/localeHarness";
import { loadStyles, ruleBlock } from "../test/styles";
import { tauriMocks } from "../test/menuCardMocks";
import { rateWindow, provider, renderCard, setupMenuCardTests } from "../test/menuCardHarness";

vi.mock("../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../lib/tauri")>()),
  ...(await import("../test/menuCardMocks")).tauriMocks,
}));
vi.mock("@tauri-apps/api/event", async () => (await import("../test/menuCardMocks")).eventMocks);

describe("MenuCard", () => {
  setupMenuCardTests();

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
    // The usage details block renders once its async data settles.
    const more = await waitFor(() => {
      const el = container.querySelector<HTMLDetailsElement>(".menu-card__more");
      expect(el).not.toBeNull();
      return el!;
    });
    expect(more.open).toBe(false);
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
const stylesSource = loadStyles();

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
