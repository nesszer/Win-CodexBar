import { describe, expect, it, vi } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { ChartsSection } from "./ChartsSection";
import { getProviderChartData } from "../../../../../lib/tauri";
import type { OpenAiApiUsageSnapshot, ProviderChartData } from "../../../../../types/bridge";

vi.mock("../../../../../lib/tauri", () => ({
  getProviderChartData: vi.fn(),
  getSettingsSnapshot: vi.fn().mockResolvedValue({ enableAnimations: false }),
}));
vi.mock("../../../../../lib/providerCharts", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../../../../../lib/providerCharts")>()),
  providerSupportsChartData: () => true,
}));

const mockChart = vi.mocked(getProviderChartData);

function chartData(overrides: Partial<ProviderChartData>): ProviderChartData {
  return {
    providerId: "codex",
    costHistory: [{ date: "2026-08-16", value: 1.5 }],
    creditsHistory: [],
    usageBreakdown: [],
    localUsage: null,
    tokensHistory: [{ date: "2026-08-16", tokens: 9000 }],
    tokensIncomplete: false,
    ...overrides,
  };
}

describe("ChartsSection tokens mode (upstream 0.50.0 #2930)", () => {
  it("defaults Codex to the Tokens tab when exact token data exists", async () => {
    mockChart.mockResolvedValue(chartData({}));
    render(
      <ChartsSection
        providerId="codex"
        accountEmail={null}
        t={(key) => key}
      />,
    );
    await waitFor(() => {
      expect(screen.getByRole("tab", { selected: true }).textContent).toBe(
        "DetailChartTokens",
      );
    });
  });

  it("keeps Cost as the default for non-Codex providers", async () => {
    mockChart.mockResolvedValue(chartData({ providerId: "claude" }));
    render(
      <ChartsSection
        providerId="claude"
        accountEmail={null}
        t={(key) => key}
      />,
    );
    await waitFor(() => {
      expect(screen.getByRole("tab", { selected: true }).textContent).toBe(
        "DetailChartCost",
      );
    });
  });

  it("shows the Refreshing marker while local history backfill is incomplete", async () => {
    mockChart.mockResolvedValue(chartData({ tokensIncomplete: true }));
    render(
      <ChartsSection
        providerId="codex"
        accountEmail={null}
        t={(key) => key}
      />,
    );
    await waitFor(() => {
      expect(screen.getByText("DetailChartRefreshing")).toBeTruthy();
    });
  });

  it("hides the Tokens tab when no day carries token data", async () => {
    mockChart.mockResolvedValue(
      chartData({ tokensHistory: [{ date: "2026-08-16", tokens: 0 }] }),
    );
    render(
      <ChartsSection
        providerId="codex"
        accountEmail={null}
        t={(key) => key}
      />,
    );
    await waitFor(() => {
      expect(screen.getByRole("tab", { selected: true }).textContent).toBe(
        "DetailChartCost",
      );
    });
    expect(screen.queryByText("DetailChartTokens")).toBeNull();
  });
});

describe("ChartsSection OpenAI API daily usage (upstream 0.66.0)", () => {
  const openAiUsage: OpenAiApiUsageSnapshot = {
    historyDays: 30,
    projectId: null,
    daily: [
      {
        startTime: 1_788_220_800,
        endTime: 1_788_307_200,
        costUsd: 2.5,
        requests: 12,
        inputTokens: 1000,
        cachedInputTokens: 100,
        outputTokens: 400,
        totalTokens: 1400,
        lineItems: [{ name: "Text tokens", costUsd: 2.5 }],
        models: [],
      },
    ],
  };

  it("draws the per-day chart for openaiapi instead of the local-log tabs", async () => {
    mockChart.mockResolvedValue(chartData({ providerId: "openaiapi" }));
    render(
      <ChartsSection
        providerId="openaiapi"
        accountEmail={null}
        openAiApiUsage={openAiUsage}
        t={(key) => key}
      />,
    );
    expect(screen.getByText("OpenAIChartTitle", { selector: ".provider-detail-chart__title" })).toBeTruthy();
    expect(screen.getByRole("listbox")).toBeTruthy();
    expect(screen.getByText("Text tokens")).toBeTruthy();
    await waitFor(() => expect(mockChart).toHaveBeenCalled());
    expect(screen.queryByText("DetailChartCost")).toBeNull();
  });

  it("draws the per-day chart for the Groq console history", () => {
    mockChart.mockResolvedValue(chartData({ providerId: "groq" }));
    render(
      <ChartsSection
        providerId="groq"
        accountEmail={null}
        openAiApiUsage={openAiUsage}
        t={(key) => key}
      />,
    );
    expect(screen.getByText("OpenAIChartTitle", { selector: ".provider-detail-chart__title" })).toBeTruthy();
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("renders nothing for openaiapi without usage or with an empty window", () => {
    mockChart.mockResolvedValue(chartData({ providerId: "openaiapi" }));
    const { container, rerender } = render(
      <ChartsSection providerId="openaiapi" accountEmail={null} t={(key) => key} />,
    );
    expect(container.firstChild).toBeNull();
    rerender(
      <ChartsSection
        providerId="openaiapi"
        accountEmail={null}
        openAiApiUsage={{ ...openAiUsage, daily: [] }}
        t={(key) => key}
      />,
    );
    expect(container.firstChild).toBeNull();
  });

  it("ignores an OpenAI snapshot handed to another provider", async () => {
    mockChart.mockResolvedValue(chartData({ providerId: "claude" }));
    render(
      <ChartsSection
        providerId="claude"
        accountEmail={null}
        openAiApiUsage={openAiUsage}
        t={(key) => key}
      />,
    );
    await waitFor(() => {
      expect(screen.getByRole("tab", { selected: true }).textContent).toBe("DetailChartCost");
    });
    expect(screen.queryByText("OpenAIChartTitle")).toBeNull();
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
