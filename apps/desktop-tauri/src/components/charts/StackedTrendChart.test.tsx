import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { SpendTrend } from "../../types/bridge";
import { buildDailyTrendModel, dayKey, dayOrdinal, type TrendModel } from "../../lib/spendTrendModel";
import { StackedTrendChart, type StackedTrendChartProps } from "./StackedTrendChart";

function trend(startDay: string, endDay: string, daily: SpendTrend["daily"]): SpendTrend {
  return {
    startDay,
    endDay,
    timeZone: "UTC",
    sources: [
      { id: "claude", providerId: "claude", displayName: "Claude", knownZero: false },
      { id: "codex", providerId: "codex", displayName: "Codex", knownZero: false },
    ],
    daily,
    hourly: [],
    hourlyDays: [],
  };
}

const week = buildDailyTrendModel(
  trend("2026-03-01", "2026-03-07", [
    { sourceId: "claude", day: "2026-03-02", costUsd: 1 },
    { sourceId: "codex", day: "2026-03-02", costUsd: 2 },
    { sourceId: "codex", day: "2026-03-04", costUsd: 4 },
  ]),
) as TrendModel;

function renderChart(model: TrendModel, overrides: Partial<StackedTrendChartProps> = {}) {
  const props: StackedTrendChartProps = {
    model,
    ariaLabel: "Daily estimated spend",
    ariaDescription: "Day",
    ticks: [],
    colorFor: (id) => `var(--${id})`,
    sourceName: (id) => id,
    bucketLabel: (key) => dayKey(key),
    formatCost: (value) => `$${value}`,
    formatAxisCost: (value) => `$${value}`,
    activeKey: null,
    pinnedKey: null,
    onHover: vi.fn(),
    onPin: vi.fn(),
    onActivate: vi.fn(),
    ...overrides,
  };
  return { props, ...render(<StackedTrendChart {...props} />) };
}

describe("StackedTrendChart", () => {
  it("draws one option per bucket with its stacked sources and rounds only the top", () => {
    renderChart(week);
    const options = screen.getAllByRole("option");
    expect(options).toHaveLength(2);
    expect(options[0].getAttribute("aria-label")).toBe("2026-03-02: $3, claude $1, codex $2");
    const paths = options[0].querySelectorAll("path");
    expect([...paths].map((path) => path.getAttribute("data-rounded"))).toEqual(["false", "true"]);
    expect(paths[1].getAttribute("fill")).toBe("var(--codex)");
  });

  it("walks buckets with arrow keys and activates with Enter", () => {
    const { props } = renderChart(week, { pinnedKey: week.buckets[0].key });
    const chart = screen.getByTestId("spend-trend-chart");
    fireEvent.keyDown(chart, { key: "ArrowRight" });
    expect(props.onPin).toHaveBeenLastCalledWith(week.buckets[1].key);
    fireEvent.keyDown(chart, { key: "Enter" });
    expect(props.onActivate).toHaveBeenCalledWith(week.buckets[0].key);
  });

  it("maps pointer position to the slot under it, including empty slots", () => {
    const { props } = renderChart(week);
    const hit = screen.getByTestId("spend-trend-hit");
    // jsdom has no layout: the host measures as 560 px, leaving 508 px of plot and 472 px between the edge paddings.
    const slotWidth = 472 / 7;
    fireEvent.mouseMove(hit, { clientX: 18 + slotWidth * 2.5 });
    expect(props.onHover).toHaveBeenLastCalledWith(dayOrdinal("2026-03-03"));
    fireEvent.click(hit, { clientX: 18 + slotWidth * 1.5 });
    expect(props.onActivate).toHaveBeenLastCalledWith(dayOrdinal("2026-03-02"));
    fireEvent.mouseLeave(hit);
    expect(props.onHover).toHaveBeenLastCalledWith(null);
  });

  it("draws the rule mark for the active slot", () => {
    renderChart(week, { activeKey: dayOrdinal("2026-03-04") });
    expect(screen.getByTestId("spend-trend-rule")).toBeTruthy();
  });

  it("scrolls a daily chart longer than 32 days, sized to 31 visible days", () => {
    const long = buildDailyTrendModel(trend("2026-03-01", "2026-04-09", [])) as TrendModel;
    renderChart(long);
    expect(screen.getByTestId("spend-trend-scroller").getAttribute("data-scrolls")).toBe("true");
    expect(Number(screen.getByTestId("spend-trend-chart").getAttribute("width"))).toBeCloseTo((508 * 40) / 31);

    const short = buildDailyTrendModel(trend("2026-03-01", "2026-04-01", [])) as TrendModel;
    renderChart(short);
    expect(screen.getAllByTestId("spend-trend-scroller")[1].getAttribute("data-scrolls")).toBe("false");
  });
});
