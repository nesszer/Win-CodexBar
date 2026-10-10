import { render, waitFor } from "@testing-library/react";
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
import { buildBundle } from "../test/localeHarness";
import type { OpenAiApiUsageSnapshot, ProviderUsageSnapshot } from "../types/bridge";
import MenuCard from "./MenuCard";
import { describeCard } from "./MenuCardDetails";

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
    reserveWillLastToReset: false,
    reserveEtaSeconds: null,
  };
}

function snapshot(
  providerId: string,
  openAiApiUsage: OpenAiApiUsageSnapshot | null,
  error: string | null = null,
): ProviderUsageSnapshot {
  return {
    providerId,
    displayName: providerId,
    primary: rateWindow(),
    selectedMetric: rateWindow(),
    primaryLabel: "Session",
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

describe("MenuCard OpenAI daily usage section", () => {
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

  it("renders a collapsed Daily usage section for openaiapi", async () => {
    const { container } = renderCard(snapshot("openaiapi", usage()));
    await waitFor(() => {
      expect(container.querySelector(".menu-card__daily-usage")).not.toBeNull();
    });
    const section = container.querySelector(".menu-card__daily-usage") as HTMLDetailsElement;
    expect(section.tagName).toBe("DETAILS");
    expect(section.open).toBe(false);
    expect(section.querySelector("summary")).toHaveTextContent("OpenAIChartTitle");
    expect(section.querySelectorAll('[role="option"]')).toHaveLength(2);
  });

  it("renders the Daily usage section for the Groq console history", async () => {
    const { container } = renderCard(snapshot("groq", usage(3)));
    await waitFor(() => {
      expect(container.querySelector(".menu-card__daily-usage")).not.toBeNull();
    });
    const section = container.querySelector(".menu-card__daily-usage") as HTMLDetailsElement;
    expect(section.querySelectorAll('[role="option"]')).toHaveLength(3);
  });

  it("does not show another provider's OpenAI payload (provider siloing)", async () => {
    const { container } = renderCard(snapshot("claude", usage()));
    await waitFor(() => {
      expect(tauriMocks.getProviderChartData).toHaveBeenCalled();
    });
    expect(container.querySelector(".menu-card__daily-usage")).toBeNull();
    expect(container.querySelector('[role="option"]')).toBeNull();
  });

  it("masks the Admin project id in the card identity when privacy is enabled", async () => {
    const provider = snapshot("openaiapi", usage());
    provider.planName = "Admin API: proj-private";
    const { container } = renderCard(provider, false, true);
    await waitFor(() => expect(container.querySelector(".menu-card__plan-badge")).not.toBeNull());
    expect(container.querySelector(".menu-card__plan-badge")).toHaveTextContent("Admin API: ••••");
    expect(container).not.toHaveTextContent("proj-private");
  });

  it("hides the section on error, in compact overview and with an empty window", async () => {
    const errored = renderCard(snapshot("openaiapi", usage(), "boom"));
    await waitFor(() => expect(tauriMocks.getLocaleStrings).toHaveBeenCalled());
    expect(errored.container.querySelector(".menu-card__daily-usage")).toBeNull();
    errored.unmount();

    const compact = renderCard(snapshot("openaiapi", usage()), true);
    expect(compact.container.querySelector(".menu-card__daily-usage")).toBeNull();
    compact.unmount();

    const empty = renderCard(snapshot("openaiapi", usage(0)));
    expect(empty.container.querySelector(".menu-card__daily-usage")).toBeNull();
  });
});

describe("describeCard OpenAI usage presence", () => {
  it("is present only for openaiapi with days and no error", () => {
    expect(describeCard(snapshot("openaiapi", usage()), null, []).openAiApiUsage).not.toBeNull();
    expect(describeCard(snapshot("claude", usage()), null, []).openAiApiUsage).toBeNull();
    expect(describeCard(snapshot("openaiapi", usage(0)), null, []).openAiApiUsage).toBeNull();
    expect(describeCard(snapshot("openaiapi", null), null, []).openAiApiUsage).toBeNull();
    expect(describeCard(snapshot("openaiapi", usage(), "boom"), null, []).openAiApiUsage).toBeNull();
  });

  it("is present for the Groq console daily history", () => {
    expect(describeCard(snapshot("groq", usage()), null, []).openAiApiUsage?.daily).toHaveLength(2);
    expect(describeCard(snapshot("groq", usage(), "boom"), null, []).openAiApiUsage).toBeNull();
  });

  it("makes a card that only has daily usage count as having details", () => {
    expect(describeCard(snapshot("openaiapi", usage()), null, []).hasDetails).toBe(true);
    expect(describeCard(snapshot("openaiapi", usage(0)), null, []).hasDetails).toBe(false);
  });

  it("keeps compact overview header-only when daily usage is the only content", () => {
    expect(describeCard(snapshot("openaiapi", usage()), null, [], "detailed", true, true).hasDetails).toBe(false);
  });
});
