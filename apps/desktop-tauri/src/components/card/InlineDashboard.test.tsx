import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../hooks/useLocale", () => ({
  useLocale: () => ({
    t: (key: string) =>
      ({ PanelRecentWindows: "Recent windows", PanelThirtyDayCostHistogram: "Daily cost" })[key] ??
      key,
    language: "english",
  }),
}));

import InlineDashboard, { barHeight, barOpacity } from "./InlineDashboard";
import type { InlineDashboardModel } from "../../lib/inlineDashboard";
import { inlineDashboardModel } from "../../lib/inlineDashboard";

const model: InlineDashboardModel = {
  kpis: [
    { title: "Today", value: "$0.80", emphasis: true },
    { title: "30d cost", value: "$25.42", emphasis: false },
  ],
  points: [
    { date: "2026-10-09", value: 2, hasUnpricedUsage: false, title: "2026-10-09: $2.00 · 10K tokens" },
    { date: "2026-10-10", value: null, hasUnpricedUsage: false, title: "2026-10-10" },
    { date: "2026-10-11", value: 8, hasUnpricedUsage: false, title: "2026-10-11: $8.00 · 40K tokens" },
  ],
  maxValue: 8,
  scaleLabel: "$8",
  windows: [
    {
      id: "0",
      title: "Current window",
      range: "Oct 6, 9:24 AM – Oct 13, 9:24 AM",
      value: "$2.65 · 1.8M",
      note: null,
    },
  ],
  detailLines: ["Top model: gpt-5", "Estimated from local logs"],
};

describe("bar geometry", () => {
  it("draws missing and zero values as a 1px line", () => {
    expect(barHeight(null, 8)).toBe(1);
    expect(barHeight(0, 8)).toBe(1);
    expect(barHeight(3, 0)).toBe(1);
  });

  it("scales bars to the 43px left under the scale line of the 58px chart, with a 3px floor", () => {
    expect(barHeight(8, 8)).toBe(43);
    expect(barHeight(4, 8)).toBe(21.5);
    expect(barHeight(0.01, 8)).toBe(3);
  });

  it("ramps the fill opacity from 0.42 + 0.18 × 0.58 to 1", () => {
    expect(barOpacity(null, 8)).toBe(0);
    expect(barOpacity(0.01, 8)).toBe(0.524);
    expect(barOpacity(8, 8)).toBe(1);
  });
});

describe("InlineDashboard", () => {
  it("keeps token usage visible when its day has no known price", () => {
    const unpriced = inlineDashboardModel({
      providerId: "codex",
      summary: {
        todayCost: null, thirtyDayCost: null, thirtyDayTokens: 1000,
        periodCost: null, periodTokens: 1000, reportingPeriod: "rolling:30",
        latestTokens: 1000, topModel: null, estimateNote: null, tokenCostUpdatedAtMs: 0,
      },
      costHistory: [{ date: "2026-10-21", value: null }],
      tokensHistory: [{ date: "2026-10-21", tokens: 1000 }],
      quotaHistory: null,
      t: (key) => key === "UsageSpendUnpriced" ? "Unpriced" : key,
    })!;
    const { container } = render(<InlineDashboard model={unpriced} />);
    const bar = container.querySelector<HTMLElement>(".card-dash__bar")!;
    expect(Number(bar.style.opacity)).toBeGreaterThan(0);
    expect(parseFloat(bar.style.height)).toBeGreaterThanOrEqual(3);
    expect(bar.title).toContain("Unpriced");
    expect(bar.title).not.toContain("$0");
  });

  it("renders the KPIs, the chart, the recent windows and the detail lines", () => {
    const { container } = render(<InlineDashboard model={model} />);
    expect(screen.getByText("Today")).toHaveClass("card-dash__kpi-title");
    expect(screen.getByText("$0.80")).toHaveClass("card-dash__kpi-value--emphasis");
    expect(screen.getByText("$25.42")).not.toHaveClass("card-dash__kpi-value--emphasis");
    expect(screen.getByRole("img", { name: "Daily cost" })).toBeInTheDocument();
    expect(container.querySelectorAll(".card-dash__bar")).toHaveLength(3);
    expect(screen.getByText("Recent windows")).toBeInTheDocument();
    expect(screen.getByText("$2.65 · 1.8M")).toBeInTheDocument();
    expect(screen.getByText("Oct 6, 9:24 AM – Oct 13, 9:24 AM")).toBeInTheDocument();
    expect(screen.getByText("Top model: gpt-5")).toBeInTheDocument();
  });

  it("swaps the scale label for the hovered day", () => {
    const { container } = render(<InlineDashboard model={model} />);
    const scale = container.querySelector(".card-dash__scale")!;
    expect(scale).toHaveTextContent("$8");
    const bars = container.querySelectorAll(".card-dash__bar");
    fireEvent.mouseEnter(bars[2]);
    expect(scale).toHaveTextContent("2026-10-11: $8.00 · 40K tokens");
    expect(bars[2]).toHaveClass("card-dash__bar--selected");
    fireEvent.mouseLeave(container.querySelector(".card-dash__chart")!);
    expect(scale).toHaveTextContent("$8");
  });

  it("omits the chart and windows when there are none", () => {
    const { container } = render(
      <InlineDashboard model={{ ...model, points: [], windows: [], detailLines: [] }} />,
    );
    expect(container.querySelector(".card-dash__chart")).toBeNull();
    expect(container.querySelector(".card-dash__windows")).toBeNull();
    expect(container.querySelector(".card-dash__details")).toBeNull();
  });
});
