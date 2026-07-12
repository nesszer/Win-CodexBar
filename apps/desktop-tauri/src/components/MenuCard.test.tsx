import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getProviderChartData: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
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
  opts: { exhausted?: boolean; resetDescription?: string | null } = {},
): ProviderUsageSnapshot {
  return {
    providerId: "claude",
    displayName: "Claude",
    primary: rateWindow(usedPercent, opts),
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
    onLayoutChange?: () => void;
  } = {},
) {
  return render(
    <LocaleProvider>
      <MenuCard
        provider={snapshot}
        hideEmail={false}
        resetTimeRelative={true}
        showAsUsed={opts.showAsUsed}
        showResetWhenExhausted={opts.showResetWhenExhausted}
        onLayoutChange={opts.onLayoutChange}
      />
    </LocaleProvider>,
  );
}

describe("MenuCard", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getLocaleStrings.mockResolvedValue(
      buildBundle({
        ActionCopyError: "Copy error",
        PanelEstimatedFromLocalLogs: "Estimated from local logs",
        PanelLeftSuffix: "left",
        PanelNow: "now",
        PanelOneHour: "1h",
        PanelFiveHours: "5h",
        PanelOnPaceBudget: "On-pace budget",
        PanelReserveSuffix: "in reserve",
        PanelThirtyDayCost: "30d cost",
        PanelThirtyDayTokens: "30d tokens",
        PanelTodayBudget: "today",
        PanelUsedSuffix: "used",
        ResetsInHoursMinutes: "Resets in {}h {}m",
        WayfinderGatewayStatus: "Gateway",
        WayfinderModels: "Models",
        WayfinderRequests: "Requests",
        WayfinderTokens: "Tokens",
        WayfinderSaved: "Saved",
        WayfinderOffline: "Gateway offline",
        WayfinderDryRun: "Dry run",
        WayfinderMissingKeys: "Missing keys",
      }),
    );
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "claude",
      costHistory: [{ date: "2026-05-24", value: 1.23 }],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: {
        todayCost: null,
        thirtyDayCost: 1.23,
        thirtyDayTokens: 584_000,
        latestTokens: null,
        topModel: "glim-4.6",
        estimateNote: "Estimated from local logs",
        tokenCostUpdatedAtMs: 1234,
      },
    });
    eventMocks.listen.mockResolvedValue(() => {});
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

  it("can render metric bars as used instead of remaining", async () => {
    renderCard(provider(null, 35), { showAsUsed: true });

    expect(await screen.findByText("35% used")).toBeInTheDocument();
    expect(screen.queryByText("65% left")).not.toBeInTheDocument();

    const fill = document.querySelector<HTMLElement>(".menu-metric__bar-fill");
    expect(fill?.style.width).toBe("35%");
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

    expect(await screen.findByText(/Resets in 0h/)).toBeInTheDocument();
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

  it("notifies the tray panel after async local usage data loads", async () => {
    const onLayoutChange = vi.fn();

    renderCard(provider(null), { onLayoutChange });

    await waitFor(() => {
      expect(onLayoutChange).toHaveBeenCalled();
    });
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
    expect(screen.getByText("now 20%")).toBeInTheDocument();
    expect(screen.getByText("1h 21%")).toBeInTheDocument();
    expect(screen.queryByRole("img", { name: /usage pace/i })).not.toBeInTheDocument();

    fireEvent.click(toggle);

    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(screen.getByRole("img", { name: /usage pace/i })).toBeInTheDocument();
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

    expect(
      await screen.findByRole("button", { name: /On-pace budget/ }),
    ).toBeInTheDocument();
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
      screen.queryByRole("img", { name: /usage pace/i }),
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
