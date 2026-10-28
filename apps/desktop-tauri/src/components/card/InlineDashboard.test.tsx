import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../hooks/useLocale", () => ({
  useLocale: () => ({
    t: (key: string) =>
      ({
        CostTitle: "Cost",
        PanelRecentWindows: "Recent windows",
        PanelThirtyDayCostHistogram: "Daily cost",
      })[key] ??
      key,
    language: "english",
  }),
}));

import InlineDashboard, { barHeight, barOpacity, summaryKpis } from "./InlineDashboard";
import type { InlineDashboardModel } from "../../lib/inlineDashboard";

const model: InlineDashboardModel = {
  kpis: [
    { title: "Today", value: "$0.80", emphasis: true },
    { title: "30d cost", value: "$25.42", emphasis: false },
  ],
  points: [
    { date: "2026-10-09", value: 2, title: "2026-10-09: $2.00 · 10K tokens" },
    { date: "2026-10-10", value: null, title: "2026-10-10" },
    { date: "2026-10-11", value: 8, title: "2026-10-11: $8.00 · 40K tokens" },
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

function renderExpanded(dashboard: InlineDashboardModel = model) {
  const view = render(<InlineDashboard model={dashboard} />);
  fireEvent.click(screen.getByRole("button", { name: /Cost/ }));
  return view;
}

describe("summaryKpis", () => {
  const kpi = (title: string) => ({ title, value: "1", emphasis: false });
  const titles = (list: ReturnType<typeof summaryKpis>) => list.map((entry) => entry.title);

  it("takes today and the second KPI in the four-KPI layouts", () => {
    expect(titles(summaryKpis([kpi("Today"), kpi("30d cost"), kpi("Latest"), kpi("30d tokens")]))).toEqual([
      "Today",
      "30d cost",
    ]);
  });

  it("takes the History KPI, not the quota window, in the six-KPI layout", () => {
    const six = [
      kpi("Today"),
      kpi("Current window"),
      kpi("Latest"),
      kpi("Window tokens"),
      kpi("30d cost"),
      kpi("30d tokens"),
    ];
    expect(titles(summaryKpis(six))).toEqual(["Today", "30d cost"]);
  });

  it("copes with a single KPI", () => {
    expect(titles(summaryKpis([kpi("Today")]))).toEqual(["Today"]);
  });
});

describe("InlineDashboard", () => {
  it("starts collapsed with the Cost title and a two-line summary", () => {
    const { container } = render(<InlineDashboard model={model} />);
    const toggle = screen.getByRole("button", { name: /Cost/ });
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(
      Array.from(container.querySelectorAll(".card-dash__summary-line"), (n) => n.textContent),
    ).toEqual(["Today: $0.80", "30d cost: $25.42"]);
    expect(container.querySelector(".card-dash__kpis")).toBeNull();
    expect(container.querySelector(".card-dash__chart")).toBeNull();
  });

  it("opens the full dashboard on toggle, drops the summary, and closes again", () => {
    const onToggle = vi.fn();
    const { container } = render(<InlineDashboard model={model} onToggle={onToggle} />);
    const toggle = screen.getByRole("button", { name: /Cost/ });
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    expect(container.querySelector(".card-dash__summary")).toBeNull();
    expect(container.querySelector(".card-dash__kpis")).not.toBeNull();
    expect(onToggle).toHaveBeenCalledTimes(1);
    fireEvent.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(container.querySelector(".card-dash__kpis")).toBeNull();
    expect(container.querySelector(".card-dash__summary")).not.toBeNull();
  });

  it("renders the KPIs, the chart, the recent windows and the detail lines", () => {
    const { container } = renderExpanded();
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
    const { container } = renderExpanded();
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
    const { container } = renderExpanded({
      ...model,
      points: [],
      windows: [],
      detailLines: [],
    });
    expect(container.querySelector(".card-dash__chart")).toBeNull();
    expect(container.querySelector(".card-dash__windows")).toBeNull();
    expect(container.querySelector(".card-dash__details")).toBeNull();
  });
});
