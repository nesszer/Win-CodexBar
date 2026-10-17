import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const tauriMocks = vi.hoisted(() => ({
  getDeepSeekPricingStatus: vi.fn(),
}));
vi.mock("../../../../lib/tauri", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../lib/tauri")>()),
  ...tauriMocks,
}));

const STRINGS: Record<string, string> = {
  PanelUsageDetails: "Usage details",
  PanelOnPaceBudget: "On-pace budget",
  PanelNow: "now",
  PanelOneHour: "1h",
  PanelFiveHours: "5h",
  PanelTodayBudget: "today",
  PanelExpected: "Expected",
  PanelActual: "Actual",
  DeepSeekPricingTitle: "DeepSeek pricing",
  DeepSeekPricingOffPeak: "Off-peak hours",
  DeepSeekPricingCurrent: "Current local time:",
  DeepSeekPricingNext: "Next transition:",
  DeepSeekPricingEffective: "Effective local time:",
  DeepSeekPricingAdvice: "Official schedule",
};
vi.mock("../../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => STRINGS[key] ?? key, language: "english" }),
}));

import type { ProviderUsageSnapshot, RateWindowSnapshot } from "../../../../types/bridge";
import {
  formatSessionEquivalentEstimate,
  UsageDetailsSection,
} from "./UsageDetailsSection";

const WEEK = 7 * 24 * 60;

function window(usedPercent: number, windowMinutes: number | null, resetsAt: string | null) {
  return {
    usedPercent,
    remainingPercent: 100 - usedPercent,
    windowMinutes,
    resetsAt,
    resetDescription: null,
    isExhausted: false,
    reservePercent: null,
    reserveDescription: null,
    reserveWillLastToReset: false,
    reserveEtaSeconds: null,
  } as RateWindowSnapshot;
}

function snapshot(overrides: Partial<ProviderUsageSnapshot> = {}): ProviderUsageSnapshot {
  const primary = window(20, 5 * 60, null);
  return {
    providerId: "claude",
    displayName: "Claude",
    primary,
    selectedMetric: primary,
    primaryLabel: "Session",
    secondary: null,
    modelSpecific: null,
    tertiary: null,
    extraRateWindows: [],
    cost: null,
    planName: null,
    accountEmail: null,
    sourceLabel: "oauth",
    updatedAt: "2026-10-11T00:00:00Z",
    error: null,
    errorState: "unknown",
    pace: null,
    accountOrganization: null,
    trayStatusLabel: null,
    fetchDurationMs: null,
    ...overrides,
  } as ProviderUsageSnapshot;
}

describe("formatSessionEquivalentEstimate", () => {
  it("rounds to one decimal and pluralizes", () => {
    const forecast = (n: number) =>
      ({ estimatedWindowsToExhaustWeekly: n }) as ProviderUsageSnapshot["sessionEquivalentForecast"];
    expect(formatSessionEquivalentEstimate(forecast(3.04))).toBe("Estimated: 3 session quotas left");
    expect(formatSessionEquivalentEstimate(forecast(0.5))).toBe("Estimated: 0.5 session quota left");
    expect(formatSessionEquivalentEstimate(null)).toBeNull();
  });
});

describe("UsageDetailsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue(null);
  });

  it("renders nothing without a live snapshot or any planning detail", () => {
    const { container, rerender } = render(<UsageDetailsSection provider={null} />);
    expect(container).toBeEmptyDOMElement();
    rerender(<UsageDetailsSection provider={snapshot()} />);
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the on-pace budget and projection chart for a weekly window", () => {
    const resetsAt = new Date(Date.now() + 0.6 * 7 * 24 * 60 * 60 * 1000).toISOString();
    render(
      <UsageDetailsSection
        provider={snapshot({ secondary: window(20, WEEK, resetsAt), secondaryLabel: "Weekly" })}
      />,
    );
    expect(screen.getByRole("heading", { name: "Usage details" })).toBeInTheDocument();
    expect(screen.getByText(/On-pace budget/)).toBeInTheDocument();
    expect(screen.getByText("now 20%")).toBeInTheDocument();
    expect(screen.getByText("1h 21%")).toBeInTheDocument();
    expect(screen.getByRole("img", { name: /PaceChartAriaLabel/i })).toBeInTheDocument();
  });

  it("skips budgets for windows shorter than a week", () => {
    const resetsAt = new Date(Date.now() + 4 * 60 * 60 * 1000).toISOString();
    const { container } = render(
      <UsageDetailsSection provider={snapshot({ primary: window(31, 5 * 60, resetsAt) })} />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows the forecast and expected versus actual pace bars", () => {
    const { container } = render(
      <UsageDetailsSection
        provider={snapshot({
          sessionEquivalentForecast: {
            estimatedWindowsToExhaustWeekly: 2.5,
          } as ProviderUsageSnapshot["sessionEquivalentForecast"],
          pace: {
            stage: "ahead",
            deltaPercent: 12.5,
            willLastToReset: false,
            etaSeconds: 3600,
            expectedUsedPercent: 40,
            actualUsedPercent: 52.5,
          },
        })}
      />,
    );
    expect(screen.getByText("Estimated: 2.5 session quotas left")).toBeInTheDocument();
    expect(screen.getByText("+12.5%")).toBeInTheDocument();
    const fills = container.querySelectorAll<HTMLElement>(".provider-usage-details__pace-fill");
    expect(fills[0].style.width).toBe("40%");
    expect(fills[1].style.width).toBe("52.5%");
  });

  it("follows the pace sign preference for the pace delta", () => {
    const pacedSnapshot = (deltaPercent: number) =>
      snapshot({
        pace: {
          stage: deltaPercent < 0 ? "behind" : "ahead",
          deltaPercent,
          willLastToReset: deltaPercent < 0,
          etaSeconds: null,
          expectedUsedPercent: 40,
          actualUsedPercent: 40 + deltaPercent,
        },
      });
    const { rerender } = render(<UsageDetailsSection provider={pacedSnapshot(-14)} />);
    expect(screen.getByText("-14.0%")).toBeInTheDocument();
    rerender(<UsageDetailsSection provider={pacedSnapshot(-14)} paceReservePositive />);
    expect(screen.getByText("+14.0%")).toBeInTheDocument();
    rerender(<UsageDetailsSection provider={pacedSnapshot(12.5)} paceReservePositive />);
    expect(screen.getByText("-12.5%")).toBeInTheDocument();
  });

  it("drops pace detail for providers whose pace is not meaningful", () => {
    const { container } = render(
      <UsageDetailsSection
        provider={snapshot({
          providerId: "opencodego",
          sourceLabel: "Local estimate",
          sessionEquivalentForecast: {
            estimatedWindowsToExhaustWeekly: 2,
          } as ProviderUsageSnapshot["sessionEquivalentForecast"],
        })}
      />,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it("shows DeepSeek pricing", async () => {
    tauriMocks.getDeepSeekPricingStatus.mockResolvedValue({
      period: "offPeak",
      currentLocalTime: "2026-08-17 05:00:00 UTC",
      nextTransitionLocalTime: "2026-08-17 06:00:00 UTC",
      effectiveLocalTime: "2026-08-16 18:00:00 EDT",
    });
    render(<UsageDetailsSection provider={snapshot({ providerId: "deepseek" })} />);
    expect(await screen.findByText("DeepSeek pricing: Off-peak hours")).toBeInTheDocument();
    expect(screen.getByText("Next transition: 2026-08-17 06:00:00 UTC")).toBeInTheDocument();
  });
});
