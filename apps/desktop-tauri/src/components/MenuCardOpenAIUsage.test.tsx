import { fireEvent, render, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getProviderChartData: vi.fn(),
  getDeepSeekPricingStatus: vi.fn(),
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
  claudeAccountsList: vi.fn(),
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
import { PROVIDER_CHART_MODES_STORAGE_KEY } from "../lib/openaiSpend";
import { buildBundle } from "../test/localeHarness";
import type { OpenAiApiUsageSnapshot, ProviderUsageSnapshot } from "../types/bridge";
import MenuCard from "./MenuCard";
import { describeCard } from "./card/describeCard";

const START = 1_788_220_800;

function usage(days = 2): OpenAiApiUsageSnapshot {
  return {
    historyDays: 30,
    projectId: null,
    daily: Array.from({ length: days }, (_, i) => ({
      startTime: START + i * 86_400,
      endTime: START + (i + 1) * 86_400,
      costUsd: 1 + i,
      requests: 10,
      inputTokens: 1000,
      cachedInputTokens: 100,
      outputTokens: 400,
      totalTokens: 1400,
      lineItems: [{ name: "Text tokens", costUsd: 1 + i }],
      models: [],
    })),
  };
}

function rateWindow() {
  return {
    usedPercent: 10,
    remainingPercent: 90,
    windowMinutes: null,
    resetsAt: null,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
    isInformational: false,
    descriptionIsDetail: false,
    reserveWillLastToReset: false,
    reserveEtaSeconds: null,
    pace: null,
    monthlyLimitBlock: null,
  };
}

function snapshot(
  providerId: string,
  openAiApiUsage: OpenAiApiUsageSnapshot | undefined,
  error: string | null = null,
): ProviderUsageSnapshot {
  return {
    providerId,
    displayName: providerId,
    primary: rateWindow(),
    selectedMetric: rateWindow(),
    primaryLabel: "Session",
    secondaryLabel: null,
    tertiaryLabel: null,
    accountLabel: null,
    subscription: null,
    hasSuccessfulClaudeCliQuota: false,
    wayfinderUsage: null,
    secondary: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "api",
    updatedAt: "2026-05-24T00:00:00Z",
    error,
    errorState: "unknown",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    fetchDurationMs: null,
    openAiApiUsage,
  };
}

function renderCard(
  snap: ProviderUsageSnapshot,
  compactOverview = false,
  hidePersonalInfo = false,
) {
  return render(
    <LocaleProvider>
      <MenuCard
        provider={snap}
        display={{ hideEmail: hidePersonalInfo, resetTimeRelative: true, compactOverview }}
      />
    </LocaleProvider>,
  );
}

describe("MenuCard OpenAI daily usage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.claudeAccountsList.mockResolvedValue([]);
    tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({}));
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue(null);
    tauriMocks.getProviderChartData.mockResolvedValue({
      providerId: "openaiapi",
      costHistory: [],
      creditsHistory: [],
      usageBreakdown: [],
      localUsage: null,
    });
    eventMocks.listen.mockResolvedValue(() => {});
  });

  // The per-day API chart lives in Settings > Providers (ChartsSection); the
  // card keeps the upstream anatomy only.
  it("keeps the per-day API chart out of the card", async () => {
    const { container } = renderCard(snapshot("openaiapi", usage()));
    await waitFor(() => expect(container.querySelector(".card-header__name")).not.toBeNull());
    expect(container.querySelector(".menu-card__daily-usage")).toBeNull();
    expect(container.querySelector('[role="option"]')).toBeNull();
    expect(container).not.toHaveTextContent("OpenAIChartTitle");
  });

  it("renders the Tokens/min grid in the full card", async () => {
    const { container } = renderCard(withThroughput(snapshot("openaiapi", undefined)));
    await waitFor(() => expect(container.querySelector(".card-header__name")).not.toBeNull());
    expect(container.querySelector(".openai-tpm")).not.toBeNull();
  });

  it("offers the Live · Spend switch only when both charts have points", async () => {
    window.localStorage.removeItem(PROVIDER_CHART_MODES_STORAGE_KEY);
    const onlyLive = renderCard(withThroughput(snapshot("openaiapi", undefined)));
    await waitFor(() => expect(onlyLive.container.querySelector(".openai-tpm")).not.toBeNull());
    expect(onlyLive.container.querySelector(".openai-live-spend__switch")).toBeNull();
    onlyLive.unmount();

    const { container } = renderCard(withThroughput(snapshot("openaiapi", usage())));
    await waitFor(() => expect(container.querySelector(".openai-live-spend__switch")).not.toBeNull());
    // Plain toggle buttons: both stay tabbable and need no arrow-key wiring.
    const switchGroup = container.querySelector(".openai-live-spend__switch");
    expect(switchGroup).toHaveAttribute("role", "group");
    const [live, spend] = Array.from(switchGroup!.querySelectorAll<HTMLButtonElement>("button"));
    expect(live).toHaveAttribute("aria-pressed", "true");
    expect(spend).toHaveAttribute("aria-pressed", "false");
    expect(container.querySelector('[data-testid="openai-spend-layer"]')).toHaveAttribute("aria-hidden", "true");

    fireEvent.click(spend);
    expect(spend).toHaveAttribute("aria-pressed", "true");
    expect(live).toHaveAttribute("aria-pressed", "false");
    expect(container.querySelector('[data-testid="openai-live-layer"]')).toHaveAttribute("aria-hidden", "true");
    expect(container.querySelector('[data-testid="openai-spend-layer"]')).toHaveAttribute("aria-hidden", "false");
    expect(container).toHaveTextContent("OpenAiSpendChartTitle");
    expect(JSON.parse(window.localStorage.getItem(PROVIDER_CHART_MODES_STORAGE_KEY) ?? "{}")).toEqual({
      openaiapi: "spend",
    });
    window.localStorage.removeItem(PROVIDER_CHART_MODES_STORAGE_KEY);
  });

  it("keeps the plain grid when the Tokens/min window has no reported traffic", async () => {
    window.localStorage.removeItem(PROVIDER_CHART_MODES_STORAGE_KEY);
    const provider = withThroughput(snapshot("openaiapi", usage()));
    const throughput = provider.throughput!;
    provider.throughput = {
      ...throughput,
      minutes: throughput.minutes.map((minute) => ({ ...minute, isPending: true })),
    };
    const { container } = renderCard(provider);
    await waitFor(() => expect(container.querySelector(".openai-tpm")).not.toBeNull());
    expect(container.querySelector(".openai-tpm__empty")).not.toBeNull();
    expect(container.querySelector(".openai-live-spend")).toBeNull();
    expect(container.querySelector(".openai-live-spend__switch")).toBeNull();
  });

  it("masks the Admin project id in the card identity when privacy is enabled", async () => {
    const provider = snapshot("openaiapi", usage());
    provider.planName = "Admin API: proj-private";
    const { container } = renderCard(provider, false, true);
    await waitFor(() => expect(container.querySelector(".card-header__plan")).not.toBeNull());
    expect(container.querySelector(".card-header__plan")).toHaveTextContent("Admin API: ••••");
    expect(container).not.toHaveTextContent("proj-private");
  });
});

function withThroughput(snap: ProviderUsageSnapshot): ProviderUsageSnapshot {
  snap.throughput = {
    minutes: [{ start: START, totalTokens: 1_000, requests: 1, isPending: false }],
    frontier: START,
    latestTokensPerMinute: 1_000,
    peakTokensPerMinute: 1_000,
    peakAt: START,
    fiveMinuteAverageTokensPerMinute: 1_000,
    models: [],
    windowMinutes: 60,
    fetchedAt: START,
    projectId: null,
  };
  return snap;
}

describe("describeCard OpenAI Tokens/min grid", () => {
  it("surfaces the grid as full-card content for the OpenAI API", () => {
    const presence = describeCard(withThroughput(snapshot("openaiapi", undefined)), null, []);
    expect(presence.openAiThroughput).not.toBeNull();
    expect(presence.hasDetails).toBe(true);
  });

  it("keeps the grid out of the compact Overview", () => {
    const presence = describeCard(withThroughput(snapshot("openaiapi", undefined)), null, [], "detailed", true);
    expect(presence.openAiThroughput).toBeNull();
  });

  it("keeps the grid out of other providers and errored cards", () => {
    expect(describeCard(withThroughput(snapshot("codex", undefined)), null, []).openAiThroughput).toBeNull();
    expect(
      describeCard(withThroughput(snapshot("openaiapi", undefined, "boom")), null, []).openAiThroughput,
    ).toBeNull();
  });
});

describe("describeCard", () => {
  it("does not count the per-day API history as card content", () => {
    const presence = describeCard(snapshot("openaiapi", usage()), null, []);
    expect(presence.hasDetails).toBe(false);
  });

  it("drops every section on error", () => {
    const errored = snapshot("openaiapi", usage(), "boom");
    errored.cost = {
      balanceUpdatedAt: null,
      accountId: null,
      used: 1,
      limit: null,
      remaining: null,
      currencyCode: "USD",
      currencySymbol: "$",
      period: "30 days",
      resetsAt: null,
      formattedUsed: "$1.00",
      formattedLimit: null,
      balance: null,
      formattedBalance: null,
      daily: [],
      alwaysVisible: true,
    };
    const presence = describeCard(errored, null, [
      { id: "primary", label: "Session", snap: errored.primary, lane: "primary" },
    ]);
    expect(presence).toMatchObject({
      hasMetrics: false,
      hasDetailSections: false,
      hasCost: false,
      localUsage: null,
      wayfinderUsage: null,
      hasDetails: false,
    });
  });

  // Upstream `showsOverviewSupplementalContent(compact:)` is `!compact || metrics.isEmpty`
  // (MenuCardView+ModelHelpers.swift:378-380).
  it("keeps compact Overview to quota rows, or details when there are none", () => {
    const snap = snapshot("openaiapi", usage());
    snap.displayDetails = [
      { id: "d", sectionTitle: null, title: "Plan", value: "Pro", secondaryValue: null, progress: null },
    ];
    const metrics = [{ id: "primary", label: "Session", snap: snap.primary, lane: "primary" as const }];
    expect(describeCard(snap, null, metrics, "detailed", false).showsDetailSections).toBe(true);
    expect(describeCard(snap, null, metrics, "detailed", true)).toMatchObject({
      hasDetails: true,
      showsDetailSections: false,
    });
    expect(describeCard(snap, null, [], "detailed", true)).toMatchObject({
      hasDetails: true,
      showsDetailSections: true,
    });
  });
});
